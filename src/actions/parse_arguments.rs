//! Port of `deadcode/actions/parse_arguments.py`.

use std::path::Path;

use clap::Parser;

use crate::data_types::Args;

/// Parses CLI args (including a leading dummy program-name token clap
/// expects), applies comma-flattening to every list option except `paths`,
/// merges `pyproject.toml`'s `[tool.deadcode]` table, and applies the
/// `--dry` overrides-`--fix` rule.
///
/// Mirrors Python's `parse_arguments`, except `--version` short-circuiting
/// happens one level up in `cli.rs`, matching where Python's `main()` checks
/// it (before `parse_arguments()` is ever called) — so, like upstream,
/// nothing here needs to make `paths` optional for `--version` to work.
pub fn parse_arguments(argv: &[String]) -> Result<Args, clap::Error> {
    parse_arguments_with_config(argv, Path::new("pyproject.toml"))
}

pub fn parse_arguments_with_config(
    argv: &[String],
    pyproject_path: &Path,
) -> Result<Args, clap::Error> {
    let mut full_argv = vec!["deadcode".to_string()];
    full_argv.extend(argv.iter().cloned());
    let mut args = Args::try_parse_from(&full_argv)?;

    flatten_comma_separated(&mut args);
    merge_pyproject_toml(&mut args, pyproject_path);

    if args.dry {
        args.fix = false;
    }

    Ok(args)
}

fn flatten_comma_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|v| v.split(',').map(str::to_string))
        .collect()
}

/// `paths` is deliberately excluded, matching Python's
/// `arg_name != 'paths'` guard.
fn flatten_comma_separated(args: &mut Args) {
    args.only = flatten_comma_list(&args.only);
    args.exclude = flatten_comma_list(&args.exclude);
    args.ignore_definitions = flatten_comma_list(&args.ignore_definitions);
    args.ignore_definitions_if_decorated_with =
        flatten_comma_list(&args.ignore_definitions_if_decorated_with);
    args.ignore_definitions_if_inherits_from =
        flatten_comma_list(&args.ignore_definitions_if_inherits_from);
    args.ignore_bodies_of = flatten_comma_list(&args.ignore_bodies_of);
    args.ignore_bodies_if_decorated_with =
        flatten_comma_list(&args.ignore_bodies_if_decorated_with);
    args.ignore_bodies_if_inherits_from = flatten_comma_list(&args.ignore_bodies_if_inherits_from);
    args.ignore_if_decorated_with = flatten_comma_list(&args.ignore_if_decorated_with);
    args.ignore_if_inherits_from = flatten_comma_list(&args.ignore_if_inherits_from);
    args.ignore_names = flatten_comma_list(&args.ignore_names);
    args.ignore_names_if_decorated_with = flatten_comma_list(&args.ignore_names_if_decorated_with);
    args.ignore_names_if_inherits_from = flatten_comma_list(&args.ignore_names_if_inherits_from);
    args.ignore_names_in_files = flatten_comma_list(&args.ignore_names_in_files);
    args.tach_config = flatten_comma_list(&args.tach_config);
}

/// Extends (not replaces) each list-valued CLI arg with any values found in
/// `pyproject.toml`'s `[tool.deadcode]` table (raw TOML array elements, not
/// further comma-split — matches Python's `parsed_args[key].extend(item)`
/// running strictly after CLI-side comma-flattening). Boolean-valued keys in
/// that table are silently ignored (Python's equivalent would crash calling
/// `.extend()` on a bool — not worth replicating a crash for something no
/// test exercises).
fn merge_pyproject_toml(args: &mut Args, pyproject_path: &Path) {
    let Ok(text) = std::fs::read_to_string(pyproject_path) else {
        return;
    };
    let Ok(data) = toml::from_str::<toml::Value>(&text) else {
        return;
    };
    let Some(table) = data
        .get("tool")
        .and_then(|t| t.get("deadcode"))
        .and_then(|d| d.as_table())
    else {
        return;
    };

    for (raw_key, value) in table {
        let key = raw_key.replace("--", "").replace('-', "_");
        let Some(extra) = value.as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        }) else {
            continue;
        };

        match key.as_str() {
            "paths" => args.paths.extend(extra),
            "only" => args.only.extend(extra),
            "exclude" => args.exclude.extend(extra),
            "ignore_definitions" => args.ignore_definitions.extend(extra),
            "ignore_definitions_if_decorated_with" => {
                args.ignore_definitions_if_decorated_with.extend(extra)
            }
            "ignore_definitions_if_inherits_from" => {
                args.ignore_definitions_if_inherits_from.extend(extra)
            }
            "ignore_bodies_of" => args.ignore_bodies_of.extend(extra),
            "ignore_bodies_if_decorated_with" => args.ignore_bodies_if_decorated_with.extend(extra),
            "ignore_bodies_if_inherits_from" => args.ignore_bodies_if_inherits_from.extend(extra),
            "ignore_if_decorated_with" => args.ignore_if_decorated_with.extend(extra),
            "ignore_if_inherits_from" => args.ignore_if_inherits_from.extend(extra),
            "ignore_names" => args.ignore_names.extend(extra),
            "ignore_names_if_decorated_with" => args.ignore_names_if_decorated_with.extend(extra),
            "ignore_names_if_inherits_from" => args.ignore_names_if_inherits_from.extend(extra),
            "ignore_names_in_files" => args.ignore_names_in_files.extend(extra),
            "tach_config" => args.tach_config.extend(extra),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Args {
        let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        parse_arguments_with_config(
            &owned,
            Path::new("/definitely/does/not/exist/pyproject.toml"),
        )
        .unwrap()
    }

    #[test]
    fn single_path_argument() {
        let args = parse(&["."]);
        assert_eq!(args.paths, vec!["."]);
        assert!(args.exclude.is_empty());
        assert!(args.ignore_names.is_empty());
    }

    #[test]
    fn several_path_arguments() {
        let args = parse(&[".", "tests"]);
        assert_eq!(args.paths, vec![".", "tests"]);
    }

    #[test]
    fn comma_separated_exclude() {
        let args = parse(&[".", "--exclude=tests,venv"]);
        assert_eq!(args.exclude, vec!["tests", "venv"]);
    }

    #[test]
    fn repeated_exclude_flag_accumulates() {
        let args = parse(&[".", "--exclude=tests,venv", "--exclude=migrations"]);
        assert_eq!(args.exclude, vec!["tests", "venv", "migrations"]);
    }

    #[test]
    fn no_color_flag() {
        assert!(parse(&[".", "--no-color"]).no_color);
    }

    #[test]
    fn ignore_names_and_ignore_files_parsing() {
        let args = parse(&[
            ".",
            "--ignore-names-in-files=tests,venv",
            "--ignore-names=BaseTestCase,lambda_handler",
        ]);
        assert_eq!(args.ignore_names_in_files, vec!["tests", "venv"]);
        assert_eq!(args.ignore_names, vec!["BaseTestCase", "lambda_handler"]);
    }

    #[test]
    fn verbose_long_and_short() {
        assert!(parse(&[".", "--verbose"]).verbose);
        assert!(parse(&[".", "-v"]).verbose);
    }

    #[test]
    fn fix_flag() {
        let args = parse(&[".", "--fix"]);
        assert!(args.fix);
        assert!(!args.dry);
    }

    #[test]
    fn dry_flag_defaults() {
        let args = parse(&[".", "--dry", "--verbose"]);
        assert!(args.dry);
        assert!(args.only.is_empty());
        assert!(args.verbose);
        assert!(!args.fix);
    }

    #[test]
    fn dry_overrides_fix_even_when_both_passed() {
        let args = parse(&[".", "--dry", "--fix"]);
        assert!(args.dry);
        assert!(!args.fix);
    }

    #[test]
    fn dry_with_single_only_filename() {
        let args = parse(&[".", "--dry", "--only", "foo.py", "--fix"]);
        assert_eq!(args.only, vec!["foo.py"]);
        assert!(args.dry);
        assert!(!args.fix);
    }

    #[test]
    fn dry_with_two_space_separated_only_filenames() {
        let args = parse(&[".", "--fix", "--dry", "--only", "foo.py", "bar.py"]);
        assert_eq!(args.only, vec!["foo.py", "bar.py"]);
        assert!(!args.fix);
        assert!(args.dry);
    }

    #[test]
    fn pyproject_toml_extends_cli_list_args() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("pyproject.toml");
        std::fs::write(
            &toml_path,
            "[tool.deadcode]\nignore_names = [\"foo\", \"bar\"]\n",
        )
        .unwrap();
        let args = parse_arguments_with_config(&[".".to_string()], &toml_path).unwrap();
        assert_eq!(args.ignore_names, vec!["foo", "bar"]);
    }

    #[test]
    fn missing_pyproject_toml_is_fine() {
        let args = parse(&["."]);
        assert!(args.ignore_names.is_empty());
    }
}
