//! Shared test-support helpers for the Rust integration test suite (mirrors
//! `deadcode/utils/base_test_case.py`'s role, but backed by a real temp
//! directory rather than mocked file I/O — this port always does real
//! filesystem I/O, so there's nothing to mock).
//!
//! Not every helper is used by every test binary that `mod common;`s this
//! file (each is compiled separately), so unused-method warnings here are
//! expected noise, not a signal — allowed at the module level.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use deadcode::cli::main_with_config;

/// A pyproject.toml path guaranteed not to exist, so tests never
/// accidentally inherit this repo's own `[tool.deadcode]` config (see
/// `cli.rs`'s `main_with_config` doc comment for why that matters).
pub const NO_PYPROJECT: &str = "/definitely/does/not/exist/pyproject.toml";

pub struct Project {
    pub dir: tempfile::TempDir,
}

impl Project {
    pub fn new() -> Self {
        Project {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Writes a file relative to the project root, creating parent dirs.
    pub fn write(&self, relative_path: &str, content: &str) -> PathBuf {
        let path = self.dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    pub fn read(&self, relative_path: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(relative_path)).unwrap()
    }

    pub fn exists(&self, relative_path: &str) -> bool {
        self.dir.path().join(relative_path).exists()
    }

    pub fn path(&self, relative_path: &str) -> PathBuf {
        self.dir.path().join(relative_path)
    }

    /// Forward-slash-normalized absolute path string, for constructing CLI
    /// argv values (e.g. `--only <path>`) — everything internally compares
    /// against `Path::as_posix()`-style strings, so a native
    /// (backslash-on-Windows) path would silently never match.
    pub fn path_str(&self, relative_path: &str) -> String {
        deadcode::visitor::code_item::path_as_posix(&self.path(relative_path))
    }

    /// Runs `deadcode <argv>` against this project's directory, with an
    /// isolated (nonexistent) pyproject.toml unless the project itself wrote
    /// one at its root.
    ///
    /// Any argv token that names an existing file/dir under the project root
    /// (relative path) is automatically rewritten to its absolute path —
    /// `cargo test` runs with CWD at the crate root, not the tempdir, so a
    /// bare relative path like `"foo.py"` would otherwise resolve against
    /// the wrong directory entirely.
    pub fn run(&self, argv: &[&str]) -> Option<String> {
        let resolved: Vec<String> = argv
            .iter()
            .map(|token| {
                let candidate = self.dir.path().join(token);
                if candidate.exists() {
                    candidate.to_string_lossy().into_owned()
                } else {
                    token.to_string()
                }
            })
            .collect();
        let borrowed: Vec<&str> = resolved.iter().map(String::as_str).collect();
        self.run_from(self.dir.path(), &borrowed)
    }

    /// Like `run`, but resolves `pyproject.toml` relative to an explicit
    /// directory (for tests that need `[tool.deadcode]` config merging).
    pub fn run_with_pyproject_in(&self, dir: &Path, argv: &[&str]) -> Option<String> {
        self.run_from(dir, argv)
    }

    fn run_from(&self, pyproject_dir: &Path, argv: &[&str]) -> Option<String> {
        let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let pyproject = pyproject_dir.join("pyproject.toml");
        let pyproject_path = if pyproject.exists() {
            pyproject
        } else {
            PathBuf::from(NO_PYPROJECT)
        };
        main_with_config(Some(&owned), &pyproject_path).unwrap()
    }
}

pub fn strip_ansi(s: &str) -> String {
    s.replace("\x1b[91m", "")
        .replace("\x1b[1m", "")
        .replace("\x1b[0m", "")
        .replace("\x1b[31m", "")
        .replace("\x1b[32m", "")
}
