//! Port of `deadcode/actions/find_python_filenames.py`.
//!
//! One deliberate perf improvement over the Python original (flagged as an
//! unaddressed opportunity in `profiling_report_round2.md` and confirmed as
//! the largest remaining bottleneck in this port's own profiling too, see
//! `profiling_rust_vs_python.md`): `std::fs::read_dir` already returns each
//! entry's file type as part of the directory read itself on every platform
//! that matters here (Windows always; Linux via `d_type` in the common
//! case), so a subsequently-popped entry doesn't need its own separate
//! `stat()` call via `Path::is_file()`/`is_dir()` — that's a second syscall
//! per file/dir, doing over again what `read_dir` already told us. Only the
//! initial `args.paths` entries (which never went through a directory
//! listing) still need a fresh stat. Symlinks are the one case
//! `DirEntry::file_type()` can't resolve on its own (it reports the link
//! itself, not its target) — `resolve_file_type` falls back to a real
//! `stat()` only for those, so symlink-following behavior (which Python's
//! `pathlib.Path.is_file()`/`is_dir()` do by default) is preserved exactly,
//! not silently dropped for a speed win.

use std::fs::{DirEntry, FileType};
use std::path::PathBuf;

use crate::actions::parse_tach_config::TachIndex;
use crate::data_types::Args;
use crate::utils::fnmatch;
use crate::visitor::code_item::path_as_posix;

struct PendingPath {
    path: PathBuf,
    /// `None` for entries seeded from `args.paths` directly (never listed
    /// from a parent directory, so their type is still unknown) — those
    /// fall back to a `Path::is_file()`/`is_dir()` stat, same as before.
    file_type: Option<FileType>,
}

fn resolve_file_type(entry: &DirEntry) -> Option<FileType> {
    match entry.file_type() {
        Ok(ft) if ft.is_symlink() => entry.path().metadata().ok().map(|m| m.file_type()),
        Ok(ft) => Some(ft),
        Err(_) => None,
    }
}

/// `tach_index` is loaded once by the caller and shared with the visitor
/// (see `actions/parse_tach_config.rs`'s module doc comment for why there's
/// no per-call memoization here, unlike Python's `@lru_cache`d
/// `load_tach_index`).
pub fn find_python_filenames(args: &Args, tach_index: &TachIndex) -> Vec<String> {
    let mut filenames = Vec::new();
    let mut paths: Vec<PendingPath> = args
        .paths
        .iter()
        .map(|p| PendingPath {
            path: PathBuf::from(p),
            file_type: None,
        })
        .collect();

    while let Some(pending) = paths.pop() {
        let path = pending.path;

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

        let is_file = pending
            .file_type
            .map_or_else(|| path.is_file(), |ft| ft.is_file());
        let is_py_file = is_file && path.extension().and_then(|e| e.to_str()) == Some("py");

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
            continue;
        }

        let is_dir = pending
            .file_type
            .map_or_else(|| path.is_dir(), |ft| ft.is_dir());
        if is_dir {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let file_type = resolve_file_type(&entry);
                    paths.push(PendingPath {
                        path: entry.path(),
                        file_type,
                    });
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

    /// Confirms the file-type fast path (from `DirEntry::file_type()`) still
    /// follows symlinks correctly via the `resolve_file_type` fallback,
    /// matching `Path::is_file()`/`is_dir()`'s (and Python `pathlib`'s)
    /// default symlink-following behavior. Skips (doesn't fail) if this
    /// process/host doesn't have permission to create symlinks (e.g.
    /// non-admin, non-developer-mode Windows) rather than reporting a false
    /// failure unrelated to the code under test.
    #[test]
    fn symlinked_py_file_and_directory_are_followed() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("real/target.py"), "");

        #[cfg(unix)]
        let link_result =
            std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("linked"));
        #[cfg(windows)]
        let link_result =
            std::os::windows::fs::symlink_dir(dir.path().join("real"), dir.path().join("linked"));

        if link_result.is_err() {
            eprintln!("skipping: no permission to create symlinks on this host");
            return;
        }

        let a = args(&[dir.path().join("linked").to_str().unwrap()], &[]);
        let index = TachIndex::new(vec![]);
        let found = find_python_filenames(&a, &index);
        assert_eq!(
            found,
            vec![path_as_posix(&dir.path().join("linked/target.py"))]
        );
    }
}
