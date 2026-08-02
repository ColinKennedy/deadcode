//! Port of tests the Python original marks `@skip` — these document
//! deliberately-unimplemented deep type/scope-flow tracking, not bugs.
//! Kept as `#[ignore]`d tests (not deleted) so future re-enablement is a
//! one-line diff, and so nobody accidentally "fixes" one of these into
//! passing without registering it's an intentional feature addition.
//!
//! Sources: `tests/fix/test_scope.py`'s skipped `TestVariableScopeTracking`
//! (duplicated verbatim in `tests/nested_scope/test_nested_scope.py`),
//! `tests/test_accurate_usage_detection.py`,
//! `tests/cli_args/test_ignore_definitions_if_inherits_from.py`'s
//! multi-class-inheritance-tree case, `tests/fix/test_assign.py`'s
//! type-hinted/tuple-unpack removal cases, `tests/fix/test_class_def.py`'s
//! cross-file scope / method-name-disambiguation cases.

mod common;
use common::Project;

#[test]
#[ignore = "unimplemented: parameter types aren't tracked through call sites"]
fn instance_passed_into_function_marks_its_methods_used() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Foo:\n    def bar(self):\n        pass\n\ndef spam(f):\n    f.bar()\n\nspam(Foo())\n",
    );
    // If implemented: Foo.bar would be recognized as used via the call chain
    // spam(Foo()) -> f.bar(). Today, Foo.bar is reported as unused.
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}

#[test]
#[ignore = "unimplemented: call-chain return-value types aren't tracked"]
fn chained_call_expression_result_type_is_not_inferred() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Bar:\n    def spam(self):\n        pass\n\nBar().spam()\n",
    );
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}

#[test]
#[ignore = "unimplemented: cross-file class usage through re-export/indirection isn't tracked"]
fn class_used_only_via_import_in_a_third_file_is_not_recognized() {
    let p = Project::new();
    p.write("bar.py", "class MyTest:\n    pass\n");
    p.write("spam.py", "from bar import MyTest\n\ninstance = MyTest()\n");
    assert_eq!(p.run(&["bar.py", "spam.py", "--no-color"]), None);
}

#[test]
#[ignore = "unimplemented: same-named methods on different classes aren't disambiguated"]
fn same_named_methods_on_different_classes_are_not_conflated() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class Bar:\n    def foo(self):\n        pass\n\nclass Spam:\n    def foo(self):\n        pass\n\nBar().foo()\n",
    );
    let result = p.run(&["foo.py", "--no-color"]).unwrap();
    assert!(result.contains("Spam") && result.contains("foo"));
}

#[test]
#[ignore = "unimplemented: only direct/first-level inherited-from is considered for the -if-decorated-with-style multi-hop case"]
fn three_level_inheritance_tree_with_intermediate_interface_class() {
    let p = Project::new();
    let src = "class Base:\n    pass\n\n\nclass Interface(Base):\n    pass\n\n\nclass UnusedClass(Interface):\n    pass\n";
    p.write("foo.py", src);
    let result = p.run(&[
        "foo.py",
        "--no-color",
        "--ignore-definitions-if-inherits-from=Base",
    ]);
    assert_eq!(result, None);
}

#[test]
#[ignore = "unimplemented: function-call argument types don't flow into called functions"]
fn function_call_with_one_argument_marks_used_via_type_flow() {
    let p = Project::new();
    p.write(
        "foo.py",
        "class X:\n    def used_method(self):\n        pass\n\ndef foo(y):\n    y.used_method()\n\nfoo(X())\n",
    );
    assert_eq!(p.run(&["foo.py", "--no-color"]), None);
}
