//! End-to-end coverage for Python syntax newer than the original
//! `rustpython-parser` backend could handle.
//!
//! Every source file here was rejected outright by that parser, and because a
//! parse failure is non-fatal (the file is skipped and the run still exits 0),
//! the symptom was silence rather than an error: whole files dropped out of
//! analysis and the run still reported success. So these tests assert on the
//! *findings*, not merely that parsing succeeded — a regression that stopped
//! understanding the syntax would otherwise show up as "no unused code found",
//! which is exactly what the original bug looked like.
//!
//! Fixtures are inline rather than in `tests/files/` so this suite stays
//! runnable on toolchains whose linters/formatters predate the syntax.

mod common;

use common::Project;

/// Asserts that `source` is understood well enough to find `expected_finding`
/// in it, keeping each case a real semantic check rather than a parse smoke test.
fn assert_reports(label: &str, source: &str, expected_finding: &str) {
    let project = Project::new();
    project.write("mod.py", source);
    let output = project.run(&["mod.py", "--no-color"]);
    let output = output.unwrap_or_else(|| {
        panic!("[{label}] nothing reported at all — the file was probably skipped as unparseable")
    });
    assert!(
        output.contains(expected_finding),
        "[{label}] expected {expected_finding:?} in output, got:\n{output}"
    );
}

/// Asserts that nothing at all is reported — i.e. every name in `source` was
/// correctly seen as used.
fn assert_clean(label: &str, source: &str) {
    let project = Project::new();
    project.write("mod.py", source);
    let output = project.run(&["mod.py", "--no-color"]);
    assert_eq!(output, None, "[{label}] expected no findings");
}

// ---------------------------------------------------------------------------
// PEP 750 — t-strings (Python 3.14)
// ---------------------------------------------------------------------------

#[test]
fn tstring_file_is_analyzed() {
    assert_reports(
        "t-string present",
        "unused_var = 1\n\n\ndef render(name):\n    return t\"Hello {name}\"\n\n\nrender(\"x\")\n",
        "DC01 Variable `unused_var` is never used",
    );
}

#[test]
fn name_used_only_inside_a_tstring_counts_as_used() {
    assert_clean(
        "t-string interpolation is a usage",
        "greeting = \"hello\"\nprint(t\"{greeting}\")\n",
    );
}

#[test]
fn name_used_only_inside_a_tstring_format_spec_counts_as_used() {
    assert_clean(
        "t-string nested format spec is a usage",
        "width = 3\nvalue = 1\nprint(t\"{value:{width}}\")\n",
    );
}

// ---------------------------------------------------------------------------
// PEP 758 — `except A, B:` without parentheses (Python 3.14)
// ---------------------------------------------------------------------------

#[test]
fn except_without_parentheses_is_analyzed() {
    assert_reports(
        "PEP 758 except",
        "unused_var = 1\n\n\ndef handler():\n    try:\n        pass\n    except ValueError, TypeError:\n        pass\n\n\nhandler()\n",
        "DC01 Variable `unused_var` is never used",
    );
}

#[test]
fn exception_types_in_a_parenthesis_free_except_count_as_used() {
    assert_clean(
        "PEP 758 except handler names are usages",
        "class MyError(Exception):\n    pass\n\n\nclass OtherError(Exception):\n    pass\n\n\ntry:\n    pass\nexcept MyError, OtherError:\n    pass\n",
    );
}

// ---------------------------------------------------------------------------
// PEP 696 — type-parameter defaults (Python 3.13)
// ---------------------------------------------------------------------------

#[test]
fn type_parameter_defaults_are_analyzed() {
    assert_reports(
        "PEP 696 type param default",
        "unused_var = 1\n\n\ndef identity[T = int](value: T) -> T:\n    return value\n\n\nidentity(1)\n",
        "DC01 Variable `unused_var` is never used",
    );
}

// ---------------------------------------------------------------------------
// PEP 701 — f-string grammar (Python 3.12)
//
// Not 3.14 features, but rejected by the same outdated parser, so a 3.12+
// codebase was already being silently skipped before this was fixed.
// ---------------------------------------------------------------------------

#[test]
fn fstring_reusing_the_outer_quote_style_is_analyzed() {
    assert_reports(
        "PEP 701 nested same quotes",
        "unused_var = 1\nmapping = {\"key\": 2}\nprint(f\"value is {mapping[\"key\"]}\")\n",
        "DC01 Variable `unused_var` is never used",
    );
}

#[test]
fn name_used_only_inside_a_nested_quote_fstring_counts_as_used() {
    assert_clean(
        "PEP 701 nested quotes interpolation is a usage",
        "mapping = {\"key\": 2}\nprint(f\"value is {mapping[\"key\"]}\")\n",
    );
}

#[test]
fn multiline_fstring_interpolation_is_analyzed() {
    assert_reports(
        "PEP 701 multiline interpolation",
        "unused_var = 1\nnumber = 2\nprint(f\"result {\n    number + 1\n}\")\n",
        "DC01 Variable `unused_var` is never used",
    );
}

#[test]
fn name_used_only_inside_a_multiline_fstring_counts_as_used() {
    assert_clean(
        "PEP 701 multiline interpolation is a usage",
        "number = 2\nprint(f\"result {\n    number + 1\n}\")\n",
    );
}

// ---------------------------------------------------------------------------
// Unused code *inside* modern constructs is still found
// ---------------------------------------------------------------------------

#[test]
fn unused_function_is_still_found_in_a_file_using_modern_syntax() {
    assert_reports(
        "modern syntax does not mask findings",
        "def unused_function():\n    pass\n\n\ndef used_function(name):\n    return t\"Hi {name}\"\n\n\nused_function(\"x\")\n",
        "DC02 Function `unused_function` is never used",
    );
}
