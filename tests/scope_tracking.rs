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

mod usage_graph {
    use super::*;

    #[test]
    fn a_definition_used_only_by_dead_code_is_reported_in_the_same_run() {
        let p = Project::new();
        p.write("x.py", "def foo():\n    return 1\n");
        p.write(
            "y.py",
            "from x import foo\n\n\ndef relay():\n    return foo()\n",
        );
        let result = p.run(&[".", "--no-color"]).unwrap();
        assert!(result.contains("x.py:1:0: DC02 Function `foo` is never used"));
        assert!(result.contains("y.py:1:14: DC07 Import `foo` is never used"));
        assert!(result.contains("y.py:4:0: DC02 Function `relay` is never used"));
    }

    #[test]
    fn a_from_import_keeps_only_the_module_it_names_alive() {
        let p = Project::new();
        p.write("a.py", "def run():\n    return 1\n");
        p.write("b.py", "def run():\n    return 2\n");
        p.write("c.py", "from a import run\nrun()\n");
        let result = p.run(&[".", "--no-color"]).unwrap();
        assert!(result.contains("b.py:1:0: DC02 Function `run` is never used"));
        assert!(!result.contains("a.py"), "{result}");
    }

    #[test]
    fn decorators_and_ignored_definitions_are_root_usages() {
        let p = Project::new();
        p.write(
            "foo.py",
            "def register(function):\n    return function\n\n\ndef helper():\n    return 1\n\n\n@register\ndef unused():\n    pass\n\n\nclass Unused:\n    def __init__(self):\n        helper()\n",
        );
        let result = common::strip_ansi(&p.run(&["foo.py", "--no-color"]).unwrap());
        // `register` is used by a decorator, `helper` by the body of an
        // ignored `__init__`: both are root usages, alive although their
        // owners are unused.
        let path = p.path_str("foo.py");
        assert_eq!(
            result,
            format!(
                "{path}:10:0: DC02 Function `unused` is never used\n\
                 {path}:14:0: DC03 Class `Unused` is never used"
            )
        );
    }
}
