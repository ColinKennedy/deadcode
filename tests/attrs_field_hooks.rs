//! `attrs`/`attr` field hooks: `@some_field.default` and
//! `@some_field.validator` are how `attrs` lets a field declare its default
//! value or validate assignments. `attrs` calls these itself via the field's
//! descriptor protocol at class-creation/instantiation time, never by their
//! own name, so they must never be reported as dead code — regardless of how
//! `field`/`define` (or the legacy `attr.ib`/`attr.s`) were imported or
//! aliased, since the decorator's shape doesn't depend on that at all.

mod common;
use common::Project;

#[test]
fn default_and_validator_hooks_are_never_flagged() {
    let p = Project::new();
    p.write(
        "models.py",
        "import attrs\n\n\
@attrs.define\n\
class Foo:\n    \
    some_property = attrs.field()\n\n    \
    @some_property.default\n    \
    def _get_stuff(self):\n        \
        return []\n\n    \
    @some_property.validator\n    \
    def _validate_stuff(self, attribute, value):\n        \
        pass\n\n\
Foo()\n",
    );
    assert_eq!(p.run(&["models.py", "--no-color"]), None);
}

#[test]
fn aliased_define_and_field_imports_still_work() {
    let p = Project::new();
    p.write(
        "models.py",
        "from attrs import foo, define as blah, field as fizz, bar\n\n\
@blah\n\
class Foo:\n    \
    some_property = fizz(list)\n\n    \
    @some_property.default\n    \
    def _get_stuff(self):\n        \
        return []\n\n    \
    @some_property.validator\n    \
    def _validate_stuff(self, attribute, value):\n        \
        pass\n",
    );
    let result = p.run(&["models.py", "--no-color"]);
    // `foo`/`bar` are genuinely unused imports and should still be reported;
    // the field hooks must not be.
    let result = result.unwrap();
    assert!(result.contains("foo"));
    assert!(result.contains("bar"));
    assert!(!result.contains("_get_stuff"));
    assert!(!result.contains("_validate_stuff"));
}

#[test]
fn legacy_attr_ib_style_still_works() {
    let p = Project::new();
    p.write(
        "models.py",
        "import attr\n\n\
@attr.s\n\
class Foo:\n    \
    some_property = attr.ib()\n\n    \
    @some_property.default\n    \
    def _get_stuff(self):\n        \
        return []\n\n    \
    @some_property.validator\n    \
    def _validate_stuff(self, attribute, value):\n        \
        pass\n\n\
Foo()\n",
    );
    assert_eq!(p.run(&["models.py", "--no-color"]), None);
}

#[test]
fn unrelated_method_named_like_a_hook_is_still_reported() {
    let p = Project::new();
    p.write(
        "models.py",
        "class Foo:\n    \
    def unused_method(self):\n        \
        return 1\n",
    );
    let result = p.run(&["models.py", "--no-color"]).unwrap();
    assert!(result.contains("unused_method"));
    assert!(result.contains("DC04"));
}
