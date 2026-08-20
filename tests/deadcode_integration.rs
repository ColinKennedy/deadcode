//! Port of `tests/test_deadcode.py`'s `DeadCodeIntegrationTests` — runs
//! against the REAL fixture files already checked into this repo at
//! `tests/files/*.py` (not copies), so their line numbers stay byte-exact
//! with what these assertions expect. `CARGO_MANIFEST_DIR` gives an
//! absolute path to the crate root regardless of `cargo test`'s CWD.

use std::path::PathBuf;

use deadcode::cli::main_with_config;

const NO_PYPROJECT: &str = "/definitely/does/not/exist/pyproject.toml";

fn fixture(relative: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push(relative);
    // Output is always forward-slash normalized regardless of host OS (the
    // ported Windows-path fix), so the expected string must be too — not
    // `PathBuf`'s native (backslash-on-Windows) `Display`.
    deadcode::visitor::code_item::path_as_posix(&p)
}

fn run(argv: &[&str]) -> Option<String> {
    let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    main_with_config(Some(&owned), std::path::Path::new(NO_PYPROJECT)).unwrap()
}

#[test]
fn unused_variable_names_found_in_real_fixture_file() {
    let file = fixture("tests/files/variables.py");
    let result = run(&[&file, "--no-color"]).unwrap();
    let expected = format!(
        "{f}:1:0: DC01 Variable `unused_global_variable` is never used\n{f}:3:0: DC01 Variable `ANOTHER_GLOBAL_VARIABLE` is never used\n{f}:5:0: DC01 Variable `third_global_varialbe` is never used",
        f = file
    );
    assert_eq!(result, expected);
}

#[test]
fn unused_function_names_found_in_real_fixture_file() {
    let file = fixture("tests/files/functions.py");
    let result = run(&[&file, "--no-color"]).unwrap();
    let expected = format!(
        "{f}:1:0: DC02 Function `unused_function` is never used\n{f}:13:0: DC02 Function `another_unused_function` is never used\n{f}:14:4: DC02 Function `this_is_unused_closure` is never used",
        f = file
    );
    assert_eq!(result, expected);
}

#[test]
fn unused_class_names_found_in_real_fixture_file() {
    let file = fixture("tests/files/classes.py");
    let result = run(&[&file, "--no-color"]).unwrap();
    let expected = format!(
        "{f}:1:0: DC03 Class `UnusedClass` is never used\n{f}:13:0: DC03 Class `AnotherUnusedClass` is never used",
        f = file
    );
    assert_eq!(result, expected);
}

#[test]
fn a_nonexistent_second_path_does_not_affect_the_first() {
    let file = fixture("tests/files/variables.py");
    let bogus = fixture("deadcode/tests/files/variables.py"); // deliberately wrong nested path
    let result = run(&[&file, &bogus, "--no-color"]).unwrap();
    assert!(result.contains("unused_global_variable"));
}

/// An unparseable file must not crash the run — but it must not pass silently
/// either. `Some("")` is the "nothing more to print, but exit non-zero" shape,
/// so CI can tell a skipped file apart from a clean one. It previously
/// returned `None`, i.e. exit 0, which is what let the Python 3.14 parser gap
/// go unnoticed.
#[test]
fn invalid_python_file_fails_the_run_rather_than_passing_silently() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("invalid_file.py");
    std::fs::write(&file, "This is invalid python file content.").unwrap();
    let result = run(&[file.to_str().unwrap(), "--no-color"]);
    assert_eq!(result, Some(String::new()));
}
