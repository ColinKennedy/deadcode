//! Port of `tests/fix/test_scope.py`'s non-skipped `TestScopeTracking` class.

mod common;
use common::Project;

#[test]
fn class_is_removed_when_unused() {
    let p = Project::new();
    p.write("foo.py", "class Foo:\n    pass\n");
    p.run(&["foo.py", "--no-color", "--fix"]);
    assert!(!p.exists("foo.py"));
}

#[test]
fn class_and_its_method_removed_when_unused() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    def bar(self):\n        variable = 123\n",
    );
    p.run(&["foo.py", "--no-color", "--fix"]);
    assert!(!p.exists("foo.py"));
}

#[test]
fn name_shadowing_tracks_each_occurrence_independently() {
    let p = Project::new();
    p.write("foo.py", "class Foo:\n    pass\n\n\nclass Foo:\n    pass\n");
    p.run(&["foo.py", "--no-color", "--fix"]);
    assert!(!p.exists("foo.py"));
}

#[test]
fn multi_level_inheritance_chain_is_tracked_for_ignore_flag() {
    let p = Project::new();
    let src =
        "class Foo:\n    pass\n\n\nclass Bar(Foo):\n    pass\n\n\nclass Spam(Bar):\n    pass\n";
    p.write("foo.py", src);
    let result = p.run(&[
        "foo.py",
        "--no-color",
        "--ignore-definitions-if-inherits-from=Foo",
    ]);
    assert_eq!(result, None);
    assert_eq!(p.read("foo.py"), src);
}

#[test]
fn parent_scopes_considered_when_resolving_nested_class_bases() {
    let p = Project::new();
    let src = "class Foo:\n    pass\n\n\nclass Bar(Foo):\n    class Spam(Foo):\n        class Eggs(Spam):\n            pass\n";
    p.write("foo.py", src);
    let result = p.run(&[
        "foo.py",
        "--no-color",
        "--ignore-definitions-if-inherits-from=Foo",
    ]);
    assert_eq!(result, None);
    assert_eq!(p.read("foo.py"), src);
}
