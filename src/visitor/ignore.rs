//! Port of `deadcode/visitor/ignore.py`.

use std::path::Path;

use ruff_python_ast::Expr;

use crate::utils::fnmatch;
use crate::visitor::code_item::path_as_posix;

pub const IGNORED_VARIABLE_NAMES: &[&str] = &["object", "self"];
pub const PYTEST_FUNCTION_NAMES: &[&str] = &[
    "setup_module",
    "teardown_module",
    "setup_function",
    "teardown_function",
];
pub const PYTEST_METHOD_NAMES: &[&str] = &[
    "setup_class",
    "teardown_class",
    "setup_method",
    "teardown_method",
];
pub const PYTEST_FIXTURE_DECORATOR_NAMES: &[&str] =
    &["@pytest.fixture", "@pytest_asyncio.fixture", "@fixture"];
pub const PYTEST_USEFIXTURES_DECORATOR_NAMES: &[&str] = &[
    "@pytest.mark.usefixtures",
    "@mark.usefixtures",
    "@usefixtures",
];
pub const OVERRIDE_DECORATOR_NAMES: &[&str] = &[
    "@typing.override",
    "@typing_extensions.override",
    "@override",
];
/// Glob patterns, not literal names: the field name (`some_property` in
/// `@some_property.default`) is arbitrary user code, unrelated to how
/// `attrs`/`attr`'s `field`/`attrib`/`ib`/`define`/`attrs`/`s` were imported
/// or aliased — see `ignore_attrs_field_hook`.
pub const ATTRS_FIELD_HOOK_DECORATOR_PATTERNS: &[&str] = &["@*.default", "@*.validator"];

fn is_special_name(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

/// Port of `_is_test_file`. No `@lru_cache` here (unlike Python): our
/// `resolve_path` is a pure in-memory lexical normalization, not a
/// filesystem syscall, so there's no repeated-syscall cost to amortize.
pub fn is_test_file(filename: &Path) -> bool {
    let resolved = crate::utils::path_utils::resolve_path(filename);
    fnmatch::match_any(
        &path_as_posix(&resolved),
        &["*/test/*", "*/tests/*", "*/test*.py", "*[-_]test.py"],
        false,
    )
}

pub fn is_conftest_file(filename: &Path) -> bool {
    filename.file_name().and_then(|n| n.to_str()) == Some("conftest.py")
}

pub fn assigns_special_variable_all(targets: &[Expr]) -> bool {
    targets
        .iter()
        .any(|t| matches!(t, Expr::Name(n) if n.id.as_str() == "__all__"))
}

pub fn ignore_class(filename: &Path, class_name: &str) -> bool {
    is_test_file(filename) && class_name.contains("Test")
}

/// Ignore star-imported names (can't detect usage) and imports from
/// `__init__.py` files (commonly used to re-export/collect a package's API).
pub fn ignore_import(filename: &Path, import_name: &str) -> bool {
    filename.file_name().and_then(|n| n.to_str()) == Some("__init__.py") || import_name == "*"
}

pub fn ignore_function(filename: &Path, function_name: &str) -> bool {
    ((PYTEST_FUNCTION_NAMES.contains(&function_name) || function_name.starts_with("test_"))
        && is_test_file(filename))
        || ignore_pytest_hook(filename, function_name)
}

/// `pytest_*` functions in conftest.py (hooks pytest calls by name).
pub fn ignore_pytest_hook(filename: &Path, function_name: &str) -> bool {
    is_conftest_file(filename) && function_name.starts_with("pytest_")
}

/// Pytest fixtures are consumed by name-matching, never called directly, so
/// they'd otherwise look unused. Trust any fixture defined in conftest.py or
/// a recognized test file; fixtures elsewhere must be exempted explicitly.
pub fn ignore_pytest_fixture(filename: &Path, decorator_names: &[String]) -> bool {
    (is_conftest_file(filename) || is_test_file(filename))
        && fnmatch::match_many(decorator_names, PYTEST_FIXTURE_DECORATOR_NAMES, true)
}

/// `@typing.override` marks a method as overriding a base-class method — the
/// base's method is what's actually called, so this is used even though
/// nothing calls it by its own name.
pub fn ignore_override(decorator_names: &[String]) -> bool {
    fnmatch::match_many(decorator_names, OVERRIDE_DECORATOR_NAMES, true)
}

/// `@some_field.default` / `@some_field.validator` register an `attrs` field's
/// default-value factory or validator: `attrs` calls these itself, by the
/// field's own descriptor protocol, at class-creation/instantiation time —
/// never by the method's own name — so they'd otherwise look unused.
///
/// The field variable's name (`some_field`) never depends on how `field`/
/// `attrib`/`ib` (or the class decorator `define`/`attrs`/`s`) were imported
/// or aliased, so matching the decorator's shape alone — an attribute access
/// ending in `.default`/`.validator` — needs no import-alias tracking at all;
/// `some_field = attrs.field(...)`, `from attrs import field as fizz` +
/// `some_field = fizz(...)`, and every other spelling all produce the exact
/// same `@some_field.default`/`@some_field.validator` decorator.
pub fn ignore_attrs_field_hook(decorator_names: &[String]) -> bool {
    fnmatch::match_many(decorator_names, ATTRS_FIELD_HOOK_DECORATOR_PATTERNS, true)
}

pub fn ignore_method(filename: &Path, method_name: &str) -> bool {
    is_special_name(method_name)
        || ((PYTEST_METHOD_NAMES.contains(&method_name) || method_name.starts_with("test_"))
            && is_test_file(filename))
}

/// Whether an attribute assignment target is `self.attr` (as opposed to e.g.
/// `foo.attr`, where `foo` is some other object).
pub fn is_self_attribute_target(value: &Expr) -> bool {
    matches!(value, Expr::Name(n) if n.id.as_str() == "self")
}

/// Ignore `_` (Python idiom), `_x` (pylint convention), and `__x__` (special
/// variable/method), but not `__x`.
pub fn ignore_variable(varname: &str) -> bool {
    IGNORED_VARIABLE_NAMES.contains(&varname)
        || (varname.starts_with('_') && !varname.starts_with("__"))
        || is_special_name(varname)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_file_detection() {
        assert!(is_test_file(&PathBuf::from("tests/test_foo.py")));
        assert!(is_test_file(&PathBuf::from("app/test_foo.py")));
        assert!(!is_test_file(&PathBuf::from("app/foo.py")));
    }

    #[test]
    fn conftest_detection() {
        assert!(is_conftest_file(&PathBuf::from(
            "tests/integration/conftest.py"
        )));
        assert!(!is_conftest_file(&PathBuf::from("tests/foo.py")));
    }

    #[test]
    fn variable_ignore_rules() {
        assert!(ignore_variable("_"));
        assert!(ignore_variable("_private"));
        assert!(ignore_variable("__dunder__"));
        assert!(!ignore_variable("__partial"));
        assert!(!ignore_variable("normal_name"));
    }

    #[test]
    fn attrs_field_hook_rules() {
        assert!(ignore_attrs_field_hook(&[
            "@some_property.default".to_string()
        ]));
        assert!(ignore_attrs_field_hook(&[
            "@some_property.validator".to_string()
        ]));
        // The field's own name is arbitrary — no fixed list to match against.
        assert!(ignore_attrs_field_hook(&["@fizz.default".to_string()]));
        assert!(!ignore_attrs_field_hook(&["@property".to_string()]));
        assert!(!ignore_attrs_field_hook(&[
            "@some_property.other".to_string()
        ]));
    }

    #[test]
    fn method_ignore_rules() {
        assert!(ignore_method(&PathBuf::from("foo.py"), "__init__"));
        assert!(ignore_method(
            &PathBuf::from("tests/test_foo.py"),
            "test_something"
        ));
        assert!(!ignore_method(
            &PathBuf::from("app/foo.py"),
            "test_something"
        ));
    }
}
