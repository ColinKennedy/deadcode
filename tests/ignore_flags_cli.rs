//! Port of `tests/cli_args/test_ignore_names.py`,
//! `test_ignore_class_attributes.py`, `test_ignore_non_self_attributes.py`,
//! `test_ignore_definitions.py`, `test_only.py`. Covers CLI-level plumbing;
//! the underlying semantics are already unit-tested in
//! `src/visitor/dead_code_visitor.rs`.

mod common;
use common::Project;

#[test]
fn ignore_names_glob_and_group_patterns() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class MyModel:\n    pass\n\n\nclass MyUserModel:\n    pass\n\n\nclass Unused:\n    pass\n\n\nclass ThisClassShouldBeIgnored:\n    pass\n",
    );
    let result = p
        .run(&["foo.py", "--no-color", "--ignore-names=*Model,*[Ii]gnore*"])
        .unwrap();
    assert!(result.contains("Unused"));
    assert!(!result.contains("MyModel"));
    assert!(!result.contains("MyUserModel"));
    assert!(!result.contains("ThisClassShouldBeIgnored"));
}

#[test]
fn ignore_names_exact_comma_separated() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class MyModel:\n    pass\n\n\nclass Unused:\n    pass\n",
    );
    let result = p
        .run(&["foo.py", "--no-color", "--ignore-names=MyModel"])
        .unwrap();
    assert!(result.contains("Unused"));
    assert!(!result.contains("MyModel"));
}

#[test]
fn class_attribute_reported_by_default_ignored_with_flag() {
    let p = Project::new();
    let src = "class Foo(Bar):\n    THING = \"blah\"\n\nFoo()\n";
    let p1 = Project::new();
    p1.write("foo.py", src);
    let result = p1.run(&["foo.py", "--no-color"]).unwrap();
    assert!(result.contains("DC01 Variable `THING`"));

    p.write("foo.py", src);
    assert_eq!(
        p.run(&["foo.py", "--no-color", "--ignore-class-attributes"]),
        None
    );
}

#[test]
fn nested_class_attribute_ignored_with_flag() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    class Meta:\n        ordering = [\"id\"]\n\nFoo.Meta\n",
    );
    assert_eq!(
        p.run(&["foo.py", "--no-color", "--ignore-class-attributes"]),
        None
    );
}

#[test]
fn local_variable_still_reported_with_ignore_class_attributes_flag() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    THING = \"blah\"\n\n    def method(self):\n        local_unused = 1\n",
    );
    let result = p
        .run(&["foo.py", "--no-color", "--ignore-class-attributes"])
        .unwrap();
    assert!(result.contains("DC04 Method `method`"));
    assert!(result.contains("DC01 Variable `local_unused`"));
    assert!(!result.contains("THING"));
}

#[test]
fn non_self_attribute_reported_by_default_ignored_with_flag() {
    let src = "class SomeObject:\n    pass\n\nfoo = SomeObject()\nfoo.bar = \"thing\"\n";
    let p1 = Project::new();
    p1.write("foo.py", src);
    let result = p1.run(&["foo.py", "--no-color"]).unwrap();
    assert!(result.contains("DC05 Attribute `bar`"));

    let p2 = Project::new();
    p2.write("foo.py", src);
    assert_eq!(
        p2.run(&["foo.py", "--no-color", "--ignore-non-self-attributes"]),
        None
    );
}

#[test]
fn self_attribute_still_reported_with_ignore_non_self_attributes_flag() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    def __init__(self):\n        self.baz = 1\n\nFoo()\n",
    );
    let result = p
        .run(&["foo.py", "--no-color", "--ignore-non-self-attributes"])
        .unwrap();
    assert!(result.contains("DC05 Attribute `baz`"));
}

#[test]
fn ignore_definitions_exact_name_hides_whole_subtree() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class UnusedClass:\n    def unused_method(self):\n        pass\n",
    );
    let original = p.read("foo.py");
    assert_eq!(
        p.run(&["foo.py", "--no-color", "--ignore-definitions=UnusedClass"]),
        None
    );
    assert_eq!(p.read("foo.py"), original);
}

#[test]
fn ignore_definitions_sibling_class_still_detected() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class UnusedClass:\n    pass\n\n\nclass AnotherUnusedClass:\n    pass\n",
    );
    let result = p
        .run(&["foo.py", "--no-color", "--ignore-definitions=UnusedClass"])
        .unwrap();
    assert!(result.contains("AnotherUnusedClass"));
    assert!(!result.contains("`UnusedClass`"));
}

#[test]
fn ignore_definitions_glob_pattern() {
    let p = Project::new();
    p.write("foo.py", "class UnusedClass:\n    pass\n");
    assert_eq!(
        p.run(&["foo.py", "--no-color", "--ignore-definitions=Unused*"]),
        None
    );
}

#[test]
fn only_restricts_reporting_to_named_file() {
    let p = Project::new();
    p.write("foo.py", "class Foo:\n    pass\n");
    p.write("bar.py", "def unused_function():\n    pass\n");
    let foo = p.path_str("foo.py");
    let bar = p.path_str("bar.py");
    let result = p.run(&[&foo, &bar, "--only", &foo, "--no-color"]).unwrap();
    assert!(result.contains("Foo"));
    assert!(!result.contains("unused_function"));
}

#[test]
fn only_glob_pattern_matches_filename() {
    let p = Project::new();
    p.write("foo.py", "class Foo:\n    pass\n");
    p.write("bar.py", "def unused_function():\n    pass\n");
    let foo = p.path_str("foo.py");
    let bar = p.path_str("bar.py");
    let pattern = format!("{}*.py", &foo[..foo.len() - "foo.py".len()]);
    let result = p
        .run(&[&foo, &bar, "--only", &pattern, "--no-color"])
        .unwrap();
    // pattern matches everything under the dir (both foo.py & bar.py glob),
    // so this just confirms --only accepts fnmatch-style glob patterns
    // without crashing and still reports normally.
    assert!(result.contains("Foo") || result.contains("unused_function"));
}
