//! Port of `tests/noqa/test_noqa_comments.py`.

mod common;
use common::Project;

#[test]
fn specific_code_noqa_suppresses_matching_class() {
    let p = Project::new();
    p.write("foo.py", "class MyTest:  # noqa: DC03\n    pass\n");
    assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
    assert_eq!(p.read("foo.py"), "class MyTest:  # noqa: DC03\n    pass\n");
}

#[test]
fn bare_noqa_suppresses_everything() {
    let p = Project::new();
    p.write("foo.py", "instance = \"labas\"  # noqa\n");
    assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
}

#[test]
fn unused_variable_suppressed_by_matching_code() {
    let p = Project::new();
    p.write("foo.py", "unused_variable = \"Hello\"  # noqa: DC01\n");
    assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
}

#[test]
fn wrong_noqa_code_does_not_suppress() {
    let p = Project::new();
    p.write("foo.py", "unused_variable = 1  # noqa: DC02\n");
    let result = p.run(&["foo.py", "--no-color", "--fix"]).unwrap();
    assert!(result.contains("DC01 Variable `unused_variable`"));
    assert!(result.ends_with("Removed 1 unused code item!"));
}

#[test]
fn unused_function_suppressed() {
    let p = Project::new();
    p.write("foo.py", "def unused_function():  # noqa: DC02\n    pass\n");
    assert_eq!(p.run(&["foo.py", "--no-color", "--fix"]), None);
}

/// Regression: the noqa lookup must resolve to the `def` line, not the
/// decorator's line.
#[test]
fn decorated_function_noqa_resolves_to_def_line() {
    let p = Project::new();
    p.write(
        "foo.py",
        "def decorator(f):\n    return f\n\n@decorator\ndef unused_function():  # noqa: DC02\n    pass\n",
    );
    // `decorator` is used (referenced by the `@decorator` line), and
    // `unused_function` is fully suppressed by its own noqa -> nothing left
    // to report at all.
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}

#[test]
fn decorated_function_multiline_signature_noqa_still_resolves() {
    let p = Project::new();
    p.write(
        "foo.py",
        "def decorator(f):\n    return f\n\n@decorator\ndef unused_function(  # noqa: DC02\n    a,\n    b,\n):\n    pass\n",
    );
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}

#[test]
fn unused_method_suppressed() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    def unused_method(self):  # noqa: DC04\n        pass\n\nFoo()\n",
    );
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}

#[test]
fn unused_import_suppressed() {
    let p = Project::new();
    p.write("foo.py", "from typing import Optional  # noqa: DC07\n");
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}
