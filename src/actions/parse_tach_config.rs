//! Port of `deadcode/actions/parse_tach_config.py`. No memoization cache here
//! (unlike the Python original's `@lru_cache`d `load_tach_index`) — that
//! existed only because Python's `find_python_filenames()` and
//! `DeadCodeVisitor.__init__()` independently called `load_tach_index` with
//! no shared state. This port's caller (`cli.rs`) loads the index once and
//! passes a shared reference to both, so there's nothing to memoize.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::utils::fnmatch;
use crate::utils::path_utils::{resolve_path, strict_ancestors};

#[derive(Debug, Clone)]
pub struct TachInterface {
    pub expose: Vec<String>,
    pub from_patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct TachModule {
    pub path_patterns: Vec<String>,
    pub unchecked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TachConfig {
    pub source_roots: Vec<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub modules: Vec<TachModule>,
    pub interfaces: Vec<TachInterface>,
}

#[derive(Debug, Default)]
pub struct TachIndex {
    configs: Vec<TachConfig>,
}

enum Relation {
    Inside,
    Ancestor,
    Unrelated,
}

fn relation_to_project_root(path: &Path, project_root: &Path) -> Relation {
    if path == project_root || strict_ancestors(path).any(|a| a == project_root) {
        Relation::Inside
    } else if strict_ancestors(project_root).any(|a| a == path) {
        Relation::Ancestor
    } else {
        Relation::Unrelated
    }
}

fn within_or_towards_source_root(path: &Path, source_root: &Path) -> bool {
    path == source_root
        || strict_ancestors(path).any(|a| a == source_root)
        || strict_ancestors(source_root).any(|a| a == path)
}

impl TachIndex {
    pub fn new(configs: Vec<TachConfig>) -> Self {
        TachIndex { configs }
    }

    /// True if `path` belongs to a tach project (is its project root or
    /// nested under it) but falls outside every one of that project's
    /// `source_roots`, and outside them for every other tach project it
    /// might also belong to. Paths unrelated to any tach project are
    /// unaffected.
    pub fn is_outside_source_roots(&self, path: &Path) -> bool {
        if self.configs.is_empty() {
            return false;
        }
        let path = resolve_path(path);

        let mut related_to_any_config = false;
        for config in &self.configs {
            let Some(project_root) = &config.project_root else {
                continue;
            };
            match relation_to_project_root(&path, project_root) {
                Relation::Unrelated => continue,
                Relation::Ancestor => return false,
                Relation::Inside => {
                    related_to_any_config = true;
                    if config
                        .source_roots
                        .iter()
                        .any(|root| within_or_towards_source_root(&path, root))
                    {
                        return false;
                    }
                }
            }
        }
        related_to_any_config
    }

    pub fn is_unchecked(&self, file: &Path) -> bool {
        for config in &self.configs {
            let Some(module_path) = dotted_module_path(file, &config.source_roots) else {
                continue;
            };
            for module in &config.modules {
                if module.unchecked && match_any_dotted_glob(&module.path_patterns, &module_path) {
                    return true;
                }
            }
        }
        false
    }

    /// `class_name` is the name of the class a method/property/attribute is
    /// directly defined in (`None` for module-level definitions). It only
    /// affects `expose` entries written as `ClassName.member` — see
    /// `match_any_dotted_expose`. Plain (undotted) `expose` entries keep
    /// matching by bare `name` alone, regardless of `class_name`, exactly as
    /// before this was supported.
    pub fn is_exposed(&self, file: &Path, class_name: Option<&str>, name: &str) -> bool {
        for config in &self.configs {
            let Some(module_path) = dotted_module_path(file, &config.source_roots) else {
                continue;
            };
            for interface in &config.interfaces {
                let adopts_interface = match &interface.from_patterns {
                    None => true,
                    Some(patterns) => match_any_dotted_glob(patterns, &module_path),
                };
                if adopts_interface
                    && (match_any_regex(&interface.expose, name)
                        || match_any_dotted_expose(&interface.expose, class_name, name))
                {
                    return true;
                }
            }
        }
        false
    }
}

pub fn load_tach_index(tach_config_paths: &[String]) -> TachIndex {
    let mut configs = Vec::new();
    for raw_path in tach_config_paths {
        let path = resolve_path(Path::new(raw_path));
        if !path.is_file() {
            eprintln!("Error: tach config {} could not be found.", path.display());
            continue;
        }
        match parse_tach_toml(&path) {
            Ok(config) => configs.push(config),
            Err(e) => eprintln!("Error: failed to parse tach config {}: {e}", path.display()),
        }
    }
    TachIndex::new(configs)
}

fn parse_tach_toml(path: &Path) -> Result<TachConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let data: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;

    let project_root = resolve_path(path.parent().unwrap_or_else(|| Path::new(".")));
    let raw_source_roots: Vec<String> = data
        .get("source_roots")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .unwrap_or_else(|| vec![".".to_string()]);
    let source_roots: Vec<PathBuf> = raw_source_roots
        .iter()
        .map(|root| resolve_path(&project_root.join(root)))
        .collect();

    let mut config = TachConfig {
        source_roots: source_roots.clone(),
        project_root: Some(project_root),
        modules: extract_modules(&data),
        interfaces: extract_interfaces(&data),
    };

    for source_root in &source_roots {
        if !source_root.is_dir() {
            continue;
        }
        let mut domain_files = find_domain_tomls(source_root);
        domain_files.sort();
        for domain_file in domain_files {
            if let Err(e) = merge_domain_toml(&mut config, &domain_file, source_root) {
                eprintln!("Error: failed to parse {}: {e}", domain_file.display());
            }
        }
    }

    Ok(config)
}

fn find_domain_tomls(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("tach.domain.toml") {
                found.push(path);
            }
        }
    }
    found
}

fn extract_modules(data: &toml::Value) -> Vec<TachModule> {
    let mut modules = Vec::new();
    let Some(raw_modules) = data.get("modules").and_then(|v| v.as_array()) else {
        return modules;
    };
    for raw in raw_modules {
        let patterns: Vec<String> = if let Some(paths) = raw.get("paths").and_then(|v| v.as_array())
        {
            paths
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        } else if let Some(path) = raw.get("path").and_then(|v| v.as_str()) {
            vec![path.to_string()]
        } else {
            continue;
        };
        let unchecked = raw
            .get("unchecked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        modules.push(TachModule {
            path_patterns: patterns,
            unchecked,
        });
    }
    modules
}

fn extract_interfaces(data: &toml::Value) -> Vec<TachInterface> {
    let mut interfaces = Vec::new();
    let Some(raw_interfaces) = data.get("interfaces").and_then(|v| v.as_array()) else {
        return interfaces;
    };
    for raw in raw_interfaces {
        let expose: Vec<String> = raw
            .get("expose")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if expose.is_empty() {
            continue;
        }
        let from_patterns = raw.get("from").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        });
        interfaces.push(TachInterface {
            expose,
            from_patterns,
        });
    }
    interfaces
}

fn merge_domain_toml(
    config: &mut TachConfig,
    domain_file: &Path,
    source_root: &Path,
) -> Result<(), String> {
    let text = std::fs::read_to_string(domain_file).map_err(|e| e.to_string())?;
    let data: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;

    let domain_root_dotted = dotted_dir_path(
        domain_file.parent().unwrap_or_else(|| Path::new(".")),
        source_root,
    );

    let resolve = |entry: &str| -> String {
        if let Some(stripped) = entry.strip_prefix("//") {
            stripped.to_string()
        } else if entry.is_empty() {
            domain_root_dotted.clone()
        } else if !domain_root_dotted.is_empty() {
            format!("{domain_root_dotted}.{entry}")
        } else {
            entry.to_string()
        }
    };

    let mut modules = extract_modules(&data);
    for module in &mut modules {
        module.path_patterns = module.path_patterns.iter().map(|p| resolve(p)).collect();
    }
    config.modules.extend(modules);

    let mut interfaces = extract_interfaces(&data);
    for interface in &mut interfaces {
        if let Some(patterns) = &interface.from_patterns {
            interface.from_patterns = Some(patterns.iter().map(|p| resolve(p)).collect());
        }
    }
    config.interfaces.extend(interfaces);

    if data
        .get("root")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("unchecked"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        config.modules.push(TachModule {
            path_patterns: vec![domain_root_dotted],
            unchecked: true,
        });
    }

    Ok(())
}

fn dotted_dir_path(directory: &Path, source_root: &Path) -> String {
    let directory = resolve_path(directory);
    let source_root = resolve_path(source_root);
    if directory == source_root {
        return String::new();
    }
    match directory.strip_prefix(&source_root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("."),
        Err(_) => String::new(),
    }
}

fn dotted_module_path(file: &Path, source_roots: &[PathBuf]) -> Option<String> {
    let file = resolve_path(file);

    let mut best_root: Option<&PathBuf> = None;
    for source_root in source_roots {
        let source_root_resolved = resolve_path(source_root);
        if strict_ancestors(&file).any(|a| a == source_root_resolved)
            && (best_root.is_none()
                || source_root.components().count() > best_root.unwrap().components().count())
        {
            best_root = Some(source_root);
        }
    }
    let best_root = resolve_path(best_root?);

    let rel = file.strip_prefix(&best_root).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    if parts.last().map(String::as_str) == Some("__init__.py") {
        parts.pop();
    } else if parts.last().is_some_and(|p| p.ends_with(".py")) {
        let last = parts.last_mut().unwrap();
        last.truncate(last.len() - ".py".len());
    } else {
        return None;
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn dotted_glob_to_regex(pattern: &str) -> Regex {
    let mut regex_str = String::from("^(?:");
    let segments: Vec<&str> = pattern.split('.').collect();
    for (i, segment) in segments.iter().enumerate() {
        if i > 0 {
            regex_str.push_str("\\.");
        }
        if *segment == "**" {
            regex_str.push_str(".*");
        } else {
            // escape then turn escaped '*' into a single-segment wildcard
            regex_str.push_str(&regex::escape(segment).replace("\\*", "[^.]*"));
        }
    }
    regex_str.push_str(")$");
    Regex::new(&regex_str).unwrap_or_else(|_| Regex::new("[^\\s\\S]").unwrap())
}

static DOTTED_GLOB_CACHE: Lazy<std::sync::Mutex<HashMap<String, Regex>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn match_any_dotted_glob(patterns: &[String], dotted_path: &str) -> bool {
    let mut cache = DOTTED_GLOB_CACHE.lock().unwrap();
    patterns.iter().any(|pattern| {
        let re = cache
            .entry(pattern.clone())
            .or_insert_with(|| dotted_glob_to_regex(pattern));
        re.is_match(dotted_path)
    })
}

fn try_compile_regex(pattern: &str) -> Option<Regex> {
    let anchored = format!("^(?:{pattern})$");
    match Regex::new(&anchored) {
        Ok(re) => Some(re),
        Err(e) => {
            eprintln!("Error: invalid tach interface regex pattern {pattern:?}: {e}");
            None
        }
    }
}

fn match_any_regex(patterns: &[String], value: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| try_compile_regex(pattern).is_some_and(|re| re.is_match(value)))
}

/// Matches `expose` entries of the form `ClassPattern.MemberPattern`
/// (e.g. `"MyClassInterface._some_method"`, `"MyClassInterface.*"`,
/// `"*.get_data"`), letting class methods, classmethods, staticmethods and
/// properties be exposed per-class rather than by bare name alone. Both
/// halves are glob patterns (`fnmatch` semantics: `*`, `?`, `[seq]`), split
/// on the first `.` in the entry. Entries without a `.`, or without a
/// current class scope (`class_name` is `None`, i.e. the definition isn't
/// directly inside a class body), never match here — they fall back to the
/// existing bare-name regex matching in `is_exposed`.
fn match_any_dotted_expose(patterns: &[String], class_name: Option<&str>, name: &str) -> bool {
    let Some(class_name) = class_name else {
        return false;
    };
    patterns.iter().any(|pattern| {
        let Some((class_pattern, member_pattern)) = pattern.split_once('.') else {
            return false;
        };
        if class_pattern.is_empty() || member_pattern.is_empty() {
            return false;
        }
        fnmatch::fnmatchcase(class_name, class_pattern)
            && fnmatch::fnmatchcase(name, member_pattern)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_glob_single_star_matches_one_segment() {
        assert!(match_any_dotted_glob(
            &["libs.*".to_string()],
            "libs.module"
        ));
        assert!(!match_any_dotted_glob(
            &["libs.*".to_string()],
            "libs.module.sub"
        ));
    }

    #[test]
    fn dotted_glob_double_star_matches_any_depth() {
        assert!(match_any_dotted_glob(
            &["libs.**".to_string()],
            "libs.module.sub"
        ));
        assert!(match_any_dotted_glob(
            &["libs.**".to_string()],
            "libs.module"
        ));
        assert!(!match_any_dotted_glob(&["libs.**".to_string()], "libs"));
        assert!(!match_any_dotted_glob(
            &["libs.**".to_string()],
            "other.module"
        ));
    }

    #[test]
    fn dotted_glob_literal_exact_match_only() {
        assert!(match_any_dotted_glob(
            &["libs.module".to_string()],
            "libs.module"
        ));
        assert!(!match_any_dotted_glob(
            &["libs.module".to_string()],
            "libs.module2"
        ));
    }

    #[test]
    fn dotted_module_path_basic() {
        let roots = vec![PathBuf::from("/src")];
        assert_eq!(
            dotted_module_path(Path::new("/src/pkg/mod.py"), &roots),
            Some("pkg.mod".to_string())
        );
    }

    #[test]
    fn dotted_module_path_init_collapses() {
        let roots = vec![PathBuf::from("/src")];
        assert_eq!(
            dotted_module_path(Path::new("/src/pkg/__init__.py"), &roots),
            Some("pkg".to_string())
        );
    }

    #[test]
    fn dotted_module_path_outside_roots_is_none() {
        let roots = vec![PathBuf::from("/src")];
        assert_eq!(dotted_module_path(Path::new("/other/mod.py"), &roots), None);
    }

    #[test]
    fn dotted_module_path_longest_root_wins() {
        let roots = vec![PathBuf::from("/proj"), PathBuf::from("/proj/src")];
        assert_eq!(
            dotted_module_path(Path::new("/proj/src/pkg/mod.py"), &roots),
            Some("pkg.mod".to_string())
        );
    }

    #[test]
    fn empty_index_never_restricts() {
        let index = TachIndex::new(vec![]);
        assert!(!index.is_outside_source_roots(Path::new("/anything")));
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn source_roots_default_to_tach_toml_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("tach.toml"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        assert!(!index.is_outside_source_roots(dir.path()));
    }

    #[test]
    fn file_next_to_source_root_is_outside() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("tach.toml"), "source_roots = [\"src\"]\n");
        write(&dir.path().join("src/core.py"), "");
        write(&dir.path().join("package.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        assert!(!index.is_outside_source_roots(&dir.path().join("src/core.py")));
        assert!(index.is_outside_source_roots(&dir.path().join("package.py")));
    }

    #[test]
    fn interface_exposure_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tach.toml"),
            "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"get_data\"]\nfrom = [\"core\"]\n",
        );
        write(&dir.path().join("core.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        assert!(index.is_exposed(&dir.path().join("core.py"), None, "get_data"));
        assert!(!index.is_exposed(&dir.path().join("core.py"), None, "helper"));
    }

    #[test]
    fn class_method_interface_exposure_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tach.toml"),
            "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"MyClassInterface._some_method\", \"MyClassInterface.get_data\"]\nfrom = [\"core\"]\n",
        );
        write(&dir.path().join("core.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        let file = dir.path().join("core.py");
        assert!(index.is_exposed(&file, Some("MyClassInterface"), "_some_method"));
        assert!(index.is_exposed(&file, Some("MyClassInterface"), "get_data"));
        // Same method name on an unlisted class isn't exposed by a dotted entry.
        assert!(!index.is_exposed(&file, Some("OtherClass"), "get_data"));
        // A method not listed for the class isn't exposed either.
        assert!(!index.is_exposed(&file, Some("MyClassInterface"), "other_method"));
        // Module-level (no enclosing class) never matches a dotted entry.
        assert!(!index.is_exposed(&file, None, "get_data"));
    }

    #[test]
    fn class_method_interface_exposure_supports_glob_on_both_segments() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tach.toml"),
            "source_roots = [\".\"]\n\n[[interfaces]]\nexpose = [\"*Interface.get_*\"]\nfrom = [\"core\"]\n",
        );
        write(&dir.path().join("core.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        let file = dir.path().join("core.py");
        assert!(index.is_exposed(&file, Some("MyInterface"), "get_data"));
        assert!(index.is_exposed(&file, Some("OtherInterface"), "get_value"));
        assert!(!index.is_exposed(&file, Some("MyInterface"), "set_data"));
        assert!(!index.is_exposed(&file, Some("MyClass"), "get_data"));
    }

    #[test]
    fn unchecked_module_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tach.toml"),
            "source_roots = [\".\"]\n\n[[modules]]\npath = \"legacy\"\nunchecked = true\n",
        );
        write(&dir.path().join("legacy.py"), "");
        write(&dir.path().join("core.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        assert!(index.is_unchecked(&dir.path().join("legacy.py")));
        assert!(!index.is_unchecked(&dir.path().join("core.py")));
    }

    #[test]
    fn domain_toml_merging_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("tach.toml"), "source_roots = [\".\"]\n");
        write(
            &dir.path().join("tach/filesystem/tach.domain.toml"),
            "[[modules]]\npath = \"service\"\nunchecked = true\n\n[[interfaces]]\nexpose = [\"api\"]\nfrom = [\"\"]\n\n[root]\nunchecked = true\n",
        );
        write(&dir.path().join("tach/filesystem/service.py"), "");
        write(&dir.path().join("tach/filesystem/__init__.py"), "");
        let index = load_tach_index(&[dir.path().join("tach.toml").to_string_lossy().to_string()]);
        // module path "service" is prefixed by the domain's dotted path -> "tach.filesystem.service",
        // and its own `unchecked = true` marks it unchecked.
        assert!(index.is_unchecked(&dir.path().join("tach/filesystem/service.py")));
        // [root] unchecked=true separately marks the domain root's OWN exact dotted
        // path unchecked (its __init__.py) — it does NOT propagate to nested modules,
        // that's a plain (non-glob) exact-path module entry.
        assert!(index.is_unchecked(&dir.path().join("tach/filesystem/__init__.py")));
        // from=[""] resolves to the domain's own dotted root ("tach.filesystem")
        // — an exact (non-glob) pattern, so it exposes "api" for that module
        // itself (__init__.py) but not for nested child modules like service.py.
        assert!(index.is_exposed(&dir.path().join("tach/filesystem/__init__.py"), None, "api"));
        assert!(!index.is_exposed(&dir.path().join("tach/filesystem/service.py"), None, "api"));
    }

    #[test]
    fn missing_tach_config_degrades_gracefully() {
        let index = load_tach_index(&["/definitely/does/not/exist/tach.toml".to_string()]);
        assert!(!index.is_unchecked(Path::new("/anything.py")));
        assert!(!index.is_exposed(Path::new("/anything.py"), None, "anything"));
    }
}
