//! Port of `deadcode/actions/parse_abstract_syntax_tree.py` — the single place
//! Python source is turned into an AST.
//!
//! The parser is `ruff_python_parser` rather than `rustpython-parser`. The
//! original port used `rustpython-parser` 0.4.0, which is the newest release on
//! crates.io and tops out at roughly Python 3.11 syntax. Anything newer failed
//! to parse, and a parse failure here is *silent* (the file is skipped, the run
//! still exits 0), so modern files were quietly dropped from analysis rather
//! than reported. Concretely, `rustpython-parser` 0.4.0 rejects:
//!
//! - PEP 701 f-strings — nested same-quotes (`f"{d["k"]}"`) and multi-line
//!   interpolations (Python 3.12)
//! - PEP 696 type-parameter defaults (`def f[T = int]()`) (Python 3.13)
//! - PEP 750 t-strings (`t"..."`) (Python 3.14)
//! - PEP 758 parenthesis-free `except A, B:` (Python 3.14)
//!
//! `target_version` is pinned to [`PythonVersion::latest`] rather than left at
//! ruff's `PythonVersion::default()` (3.10). deadcode analyses whatever source
//! it's pointed at and has no `--target-version` flag to learn the caller's
//! runtime from, so the newest grammar is the only choice that doesn't
//! spuriously reject valid input. This only widens what parses; it never
//! changes the AST produced for source that already parsed.

use ruff_python_ast::{Mod, PythonVersion, Suite};
use ruff_python_parser::{Mode, ParseError, ParseOptions};

/// Parses a whole module, returning its top-level statements.
///
/// Errors are returned rather than reported: callers decide how loud to be
/// (see `DeadCodeVisitor::visit_files`, which mirrors the Python original's
/// print-and-skip behavior).
pub fn parse_abstract_syntax_tree(source: &str) -> Result<Suite, ParseError> {
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::latest());
    let parsed = ruff_python_parser::parse(source, options)?;
    match parsed.into_syntax() {
        Mod::Module(module) => Ok(module.body),
        // `Mode::Module` can only ever yield `Mod::Module`.
        Mod::Expression(_) => unreachable!("Mode::Module always parses to Mod::Module"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each of these is valid modern Python that `rustpython-parser` 0.4.0
    /// rejected outright, which is what made deadcode silently skip whole
    /// files. They are grouped by the PEP/version that introduced them.
    #[test]
    fn parses_syntax_newer_than_the_previous_parser_supported() {
        let cases = [
            // PEP 701 (3.12): nested same-quote f-string interpolation.
            ("pep701 nested quotes", "s = f\"value {d[\"key\"]}\"\n"),
            // PEP 701 (3.12): multi-line f-string interpolation.
            ("pep701 multiline", "s = f\"result {\n    x + 1\n}\"\n"),
            // PEP 696 (3.13): type-parameter defaults.
            (
                "pep696 typeparam default",
                "def f[T = int](x: T) -> T:\n    return x\n",
            ),
            // PEP 750 (3.14): t-strings.
            ("pep750 t-string", "s = t\"Hello {name}\"\n"),
            // PEP 758 (3.14): parenthesis-free multi-exception except.
            (
                "pep758 except without parens",
                "try:\n    pass\nexcept ValueError, TypeError:\n    pass\n",
            ),
        ];
        for (label, source) in cases {
            assert!(
                parse_abstract_syntax_tree(source).is_ok(),
                "expected `{label}` to parse: {source:?}"
            );
        }
    }

    #[test]
    fn still_parses_older_syntax() {
        let cases = [
            ("match", "match x:\n    case [1, *rest]:\n        pass\n"),
            ("except*", "try:\n    pass\nexcept* ValueError:\n    pass\n"),
            ("pep695 generics", "def f[T](v: T) -> T:\n    return v\n"),
            ("walrus", "if (n := f()) > 0:\n    pass\n"),
        ];
        for (label, source) in cases {
            assert!(
                parse_abstract_syntax_tree(source).is_ok(),
                "expected `{label}` to parse: {source:?}"
            );
        }
    }

    #[test]
    fn genuinely_invalid_syntax_is_still_an_error() {
        assert!(parse_abstract_syntax_tree("this is not valid python !!! ===\n").is_err());
    }

    #[test]
    fn returns_top_level_statements() {
        let suite = parse_abstract_syntax_tree("a = 1\nb = 2\n").unwrap();
        assert_eq!(suite.len(), 2);
    }
}
