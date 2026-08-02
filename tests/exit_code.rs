//! Port of `tests/test_exit_code.py`. Rust's `std::process::exit()` can't be
//! caught in-process the way Python's `SystemExit` can via `pytest`, so
//! these test the extracted `ExitAction` decision (see `cli.rs`'s doc
//! comment on `print_main_action`) rather than spawning a real subprocess.

mod common;

use std::path::Path;

use deadcode::cli::{print_main_action, ExitAction};

const NO_PYPROJECT: &str = "/definitely/does/not/exist/pyproject.toml";

fn action(argv: &[&str]) -> ExitAction {
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    print_main_action(Some(&owned), Path::new(NO_PYPROJECT))
}

#[test]
fn exits_with_status_1_when_dead_code_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("foo.py");
    std::fs::write(&file, "unused_var = 1\n").unwrap();
    match action(&[file.to_str().unwrap(), "--no-color"]) {
        ExitAction::Print { code, .. } => assert_eq!(code, 1),
        other => panic!("expected Print{{code:1}}, got {other:?}"),
    }
}

#[test]
fn exits_with_status_0_when_no_dead_code_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("foo.py");
    std::fs::write(&file, "used_var = 1\nprint(used_var)\n").unwrap();
    match action(&[file.to_str().unwrap(), "--no-color"]) {
        ExitAction::Clean => {}
        other => panic!("expected Clean, got {other:?}"),
    }
}

/// Regression: `--quiet` makes `main()` return `''` (falsy but non-`None`);
/// the exit-code decision must check "was something found" (`Some`), not
/// truthiness of the string, or this would wrongly look like "nothing
/// found."
#[test]
fn exits_with_status_1_when_quiet_and_dead_code_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("foo.py");
    std::fs::write(&file, "unused_var = 1\n").unwrap();
    match action(&[file.to_str().unwrap(), "--quiet"]) {
        ExitAction::Print { code, stdout } => {
            assert_eq!(code, 1);
            assert!(stdout.is_empty());
        }
        other => panic!("expected Print{{code:1, stdout:\"\"}}, got {other:?}"),
    }
}

#[test]
fn does_not_exit_with_error_status_for_version_flag() {
    match action(&["--version"]) {
        ExitAction::Print { code, .. } => assert_eq!(code, 0),
        other => panic!("expected Print{{code:0}}, got {other:?}"),
    }
}
