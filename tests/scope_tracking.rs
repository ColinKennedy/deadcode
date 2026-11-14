//! Port of `tests/fix/test_scope.py`'s non-skipped `TestScopeTracking` class.

mod common;
use common::Project;

#[test]
fn absolute_import_alias_resolves_for_inheritance_ignore_rules() {
    let p = Project::new();
    p.write(
        "foo.py",
        "from django.db import models\n\nclass Ignored(models.Model):\n    pass\n\ndef sentinel():\n    pass\n",
    );
    let output = p
        .run(&[
            "foo.py",
            "--no-color",
            "--ignore-definitions-if-inherits-from=django.db.models.Model",
        ])
        .unwrap();
    assert!(output.contains("Function `sentinel` is never used"));
    assert!(!output.contains("Class `Ignored` is never used"));
}

#[test]
fn absolute_import_alias_resolves_for_decorator_ignore_rules() {
    let p = Project::new();
    p.write(
        "foo.py",
        "from django.contrib.admin import register\n\n@register\nclass Ignored:\n    pass\n\ndef sentinel():\n    pass\n",
    );
    let output = p
        .run(&[
            "foo.py",
            "--no-color",
            "--ignore-definitions-if-decorated-with=@django.contrib.admin.register",
        ])
        .unwrap();
    assert!(output.contains("Function `sentinel` is never used"));
    assert!(!output.contains("Class `Ignored` is never used"));
}

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
