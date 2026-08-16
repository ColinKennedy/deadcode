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

/// A file that cannot be parsed contributes no findings, so this run has zero
/// dead code — but it also verified nothing. Exiting 0 here made an
/// unanalysable codebase look clean, which is how the Python 3.14 parser gap
/// stayed hidden.
#[test]
fn exits_with_status_1_when_a_file_cannot_be_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("broken.py");
    std::fs::write(&file, "def f(:\n").unwrap();
    match action(&[file.to_str().unwrap(), "--no-color"]) {
        ExitAction::Print { code, stdout } => {
            assert_eq!(code, 1);
            // The detail already went to stderr; stdout stays empty.
            assert!(stdout.is_empty(), "unexpected stdout: {stdout:?}");
        }
        other => panic!("expected Print{{code:1}}, got {other:?}"),
    }
}

/// `--quiet` and `--count` silence findings on stdout; they must not silence
/// the failure signal, since that is precisely the combination CI runs under.
#[test]
fn parse_failure_still_fails_the_run_under_quiet_and_count() {
    for flag in ["--quiet", "--count"] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.py");
        std::fs::write(&file, "def f(:\n").unwrap();
        match action(&[file.to_str().unwrap(), flag]) {
            ExitAction::Print { code, .. } => assert_eq!(code, 1, "flag {flag}"),
            other => panic!("expected Print{{code:1}} for {flag}, got {other:?}"),
        }
    }
}

/// A parse failure alongside real findings must still report those findings
/// normally — the new failure path must not swallow the report.
#[test]
fn parse_failure_does_not_suppress_real_findings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("broken.py"), "def f(:\n").unwrap();
    std::fs::write(dir.path().join("good.py"), "unused_var = 1\n").unwrap();
    match action(&[dir.path().to_str().unwrap(), "--no-color"]) {
        ExitAction::Print { code, stdout } => {
            assert_eq!(code, 1);
            assert!(stdout.contains("unused_var"), "got: {stdout:?}");
        }
        other => panic!("expected Print{{code:1}}, got {other:?}"),
    }
}
