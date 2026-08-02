//! Rust port of Python's `fnmatch` module semantics (glob-style `*`/`?`/`[seq]`/
//! `[!seq]` patterns), used everywhere the Python code called `fnmatch.fnmatch`/
//! `fnmatch.fnmatchcase` via `deadcode/visitor/ignore.py`'s `_match()`.
//!
//! Deliberately simpler than CPython's `fnmatch.translate()`: CPython's version
//! contains a backtracking-avoidance optimization for long runs of `*fixed*`
//! that only affects *performance* under its own (backtracking) regex engine,
//! not match semantics. Rust's `regex` crate is automaton-based and never
//! backtracks catastrophically, so a direct translation produces identical
//! match results without needing that optimization. Multi-hyphen character-class
//! chunk-merging (an obscure CPython edge case, e.g. `[a-c-e-g]`) is not
//! replicated — untested anywhere in deadcode's own patterns or test suite.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use regex::Regex;

/// Translates a glob pattern into an anchored regex pattern string,
/// matching CPython's `fnmatch.translate()` observable behavior for the
/// subset of syntax deadcode's own patterns use (`*`, `?`, `[seq]`, `[!seq]`).
fn translate(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from("(?s)^");
    let mut i = 0;
    let n = chars.len();

    while i < n {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => {
                let mut j = i;
                if j < n && chars[j] == '!' {
                    j += 1;
                }
                if j < n && chars[j] == ']' {
                    j += 1;
                }
                while j < n && chars[j] != ']' {
                    j += 1;
                }
                if j >= n {
                    // Unterminated class: literal '['.
                    out.push_str("\\[");
                } else {
                    let mut stuff: String = chars[i..j].iter().collect();
                    let negate = stuff.starts_with('!');
                    if negate {
                        stuff = stuff[1..].to_string();
                    }
                    // Escape backslashes; leave '-' alone so ranges keep working.
                    let stuff = stuff.replace('\\', "\\\\");
                    i = j + 1;
                    if stuff.is_empty() {
                        // Empty range: never match. Rust's `regex` crate has no
                        // lookaround (unlike Python's `(?!)`), so use the
                        // standard backtracking-free "impossible class" trick:
                        // a character that is neither whitespace nor
                        // non-whitespace can never exist.
                        out.push_str(if negate { "." } else { "[^\\s\\S]" });
                    } else {
                        out.push('[');
                        if negate {
                            out.push('^');
                        }
                        // '^' or ']' as the first literal char needs escaping
                        // so it isn't read as a class negation/terminator.
                        if stuff.starts_with('^') || stuff.starts_with(']') {
                            out.push('\\');
                        }
                        out.push_str(&stuff);
                        out.push(']');
                    }
                }
            }
            other => out.push_str(&regex::escape(&other.to_string())),
        }
    }
    out.push('$');
    out
}

static REGEX_CACHE: Lazy<Mutex<HashMap<(String, bool), Regex>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn compiled(pattern: &str, case_sensitive: bool) -> Regex {
    let key = (pattern.to_string(), case_sensitive);
    let mut cache = REGEX_CACHE.lock().unwrap();
    if let Some(re) = cache.get(&key) {
        return re.clone();
    }
    let mut translated = translate(pattern);
    if !case_sensitive {
        translated = format!("(?i){translated}");
    }
    let re = Regex::new(&translated)
        .unwrap_or_else(|_| Regex::new("[^\\s\\S]").expect("literal never-match regex"));
    cache.insert(key, re.clone());
    re
}

/// Equivalent of `fnmatch.fnmatchcase(name, pattern)` (always case-sensitive).
pub fn fnmatchcase(name: &str, pattern: &str) -> bool {
    compiled(pattern, true).is_match(name)
}

/// Equivalent of `fnmatch.fnmatch(name, pattern)` (case-insensitive).
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    compiled(pattern, false).is_match(name)
}

/// Port of `deadcode/visitor/ignore.py`'s `_match(name, patterns, case=True)`:
/// true if `name` matches ANY of `patterns`.
pub fn match_any<S: AsRef<str>>(name: &str, patterns: &[S], case_sensitive: bool) -> bool {
    patterns.iter().any(|p| {
        if case_sensitive {
            fnmatchcase(name, p.as_ref())
        } else {
            fnmatch(name, p.as_ref())
        }
    })
}

/// Port of `_match_many`: true if ANY of `names` matches ANY of `patterns`.
pub fn match_many<S: AsRef<str>, T: AsRef<str>>(
    names: &[S],
    patterns: &[T],
    case_sensitive: bool,
) -> bool {
    names
        .iter()
        .any(|name| match_any(name.as_ref(), patterns, case_sensitive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_match() {
        assert!(fnmatchcase("foo.py", "foo.py"));
        assert!(!fnmatchcase("foo.py", "bar.py"));
    }

    #[test]
    fn star_glob() {
        assert!(fnmatchcase("MyModel", "*Model"));
        assert!(fnmatchcase(
            "f*.py".replace('*', "oo").as_str(),
            "f*.py".replace('*', "oo").as_str()
        ));
        assert!(fnmatchcase("foo.py", "f*.py"));
        assert!(!fnmatchcase("bar.py", "f*.py"));
    }

    #[test]
    fn bracket_class_and_group() {
        assert!(fnmatchcase("ThisClassShouldBeIgnored", "*[Ii]gnore*"));
        assert!(fnmatchcase("Unused", "Unused"));
        assert!(!fnmatchcase("Unused", "*[Ii]gnore*"));
    }

    #[test]
    fn negated_class() {
        assert!(fnmatchcase("a", "[!b]"));
        assert!(!fnmatchcase("b", "[!b]"));
    }

    #[test]
    fn case_sensitivity() {
        assert!(!fnmatchcase("FOO.PY", "foo.py"));
        assert!(fnmatch("FOO.PY", "foo.py"));
    }

    #[test]
    fn question_mark_matches_single_char() {
        assert!(fnmatchcase("foo.py", "fo?.py"));
        assert!(!fnmatchcase("foo.py", "fo?"));
    }
}
