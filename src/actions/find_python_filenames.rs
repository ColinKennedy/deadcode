//! Port of `deadcode/actions/find_python_filenames.py`.

use std::path::PathBuf;

use crate::actions::parse_tach_config::TachIndex;
use crate::data_types::Args;
use crate::utils::fnmatch;
use crate::visitor::code_item::path_as_posix;

/// `tach_index` is loaded once by the caller and shared with the visitor
/// (see `actions/parse_tach_config.rs`'s module doc comment for why there's
/// no per-call memoization here, unlike Python's `@lru_cache`d
/// `load_tach_index`).
pub fn find_python_filenames(args: &Args, tach_index: &TachIndex) -> Vec<String> {
    let mut filenames = Vec::new();
    let mut paths: Vec<PathBuf> = args.paths.iter().map(PathBuf::from).collect();

    while let Some(path) = paths.pop() {
        if fnmatch::match_any(&path_as_posix(&path), &args.exclude, true) {
            if args.verbose {
                eprintln!("Ignoring: {}", path.display());
            }
            continue;
        }

        if tach_index.is_outside_source_roots(&path) {
            if args.verbose {
                eprintln!("Ignoring (outside tach source roots): {}", path.display());
            }
            continue;
        }

        let is_py_file = path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("py");

        if is_py_file && tach_index.is_unchecked(&path) {
            if args.verbose {
                eprintln!("Ignoring (unchecked tach module): {}", path.display());
            }
            continue;
        }

        if is_py_file {
            // .as_posix() (not the OS-native separator) so filenames stay
            // forward-slash on every host OS, matching paths given on the
            // command line.
            filenames.push(path_as_posix(&path));
        } else if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    paths.push(entry.path());
                }
            }
        } else if !path.exists() {
            eprintln!("Error: {} could not be found.", path.display());
        }
    }

    if args.verbose {
        eprintln!(
            "Files to be checked for dead code:\n  - {}",
            filenames.join("\n  - ")
        );
    }

    filenames
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn args(paths: &[&str], exclude: &[&str]) -> Args {
        let mut a = Args::default();
        a.paths = paths.iter().map(|s| s.to_string()).collect();
        a.exclude = exclude.iter().map(|s| s.to_string()).collect();
        a
    }

    fn write(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn finds_single_file() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cli.py"), "");
        let a = args(&[dir.path().join("cli.py").to_str().unwrap()], &[]);
        let index = TachIndex::new(vec![]);
        let found = find_python_filenames(&a, &index);
        assert_eq!(found, vec![path_as_posix(&dir.path().join("cli.py"))]);
    }

    #[test]
    fn discovers_py_files_in_directory_recursively() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cli.py"), "");
        write(&dir.path().join("utils/helpers.py"), "");
        write(&dir.path().join("README.md"), "");
        let a = args(&[dir.path().to_str().unwrap()], &[]);
        let index = TachIndex::new(vec![]);
        let mut found = find_python_filenames(&a, &index);
        found.sort();
        assert_eq!(
            found,
            vec![
                path_as_posix(&dir.path().join("cli.py")),
                path_as_posix(&dir.path().join("utils/helpers.py")),
            ]
        );
    }

    #[test]
    fn exclude_skips_whole_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cli.py"), "");
        write(&dir.path().join("utils/helpers.py"), "");
        // Exclude patterns are matched against the posix-normalized candidate
        // path (matching upstream, which always compares `Path.as_posix()`),
        // so the pattern itself must be forward-slash form regardless of host
        // OS -- not `to_str()`, which gives the native (backslash-on-Windows)
        // separator.
        let a = args(
            &[dir.path().to_str().unwrap()],
            &[&path_as_posix(&dir.path().join("utils"))],
        );
        let index = TachIndex::new(vec![]);
        let found = find_python_filenames(&a, &index);
        assert_eq!(found, vec![path_as_posix(&dir.path().join("cli.py"))]);
    }

    #[test]
    fn exclude_skips_specific_file() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("cli.py"), "");
        write(&dir.path().join("keep.py"), "");
        let a = args(
            &[dir.path().to_str().unwrap()],
            &[&path_as_posix(&dir.path().join("cli.py"))],
        );
        let index = TachIndex::new(vec![]);
        let found = find_python_filenames(&a, &index);
        assert_eq!(found, vec![path_as_posix(&dir.path().join("keep.py"))]);
    }

    #[test]
    fn nonexistent_path_logs_and_continues_without_crashing() {
        let a = args(&["/definitely/does/not/exist.py"], &[]);
        let index = TachIndex::new(vec![]);
        assert!(find_python_filenames(&a, &index).is_empty());
    }

    #[test]
    fn non_py_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "");
        write(&dir.path().join("data.json"), "");
        let a = args(&[dir.path().to_str().unwrap()], &[]);
        let index = TachIndex::new(vec![]);
        assert!(find_python_filenames(&a, &index).is_empty());
    }
}
