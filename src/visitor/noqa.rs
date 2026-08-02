//! Port of `deadcode/visitor/noqa.py`.

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::bytes::RegexBuilder;

static NOQA_REGEXP: Lazy<regex::bytes::Regex> = Lazy::new(|| {
    RegexBuilder::new(r"# noqa(?::[\s]?(?P<codes>([A-Z]+[0-9]+(?:[,\s]+)?)+))?")
        .case_insensitive(true)
        .build()
        .expect("valid noqa regex")
});

fn noqa_code_map(code: &str) -> String {
    match code {
        // flake8 F401: module imported but unused.
        "F401" => "DC07",
        // flake8 F841: local variable is assigned to but never used.
        "F841" => "DC01",
        "DC01" | "DC02" | "DC03" | "DC04" | "DC05" | "DC06" | "DC07" | "DC08" | "DC09" | "DC11"
        | "DC12" | "DC13" => code,
        // Backward-compat 3-digit aliases.
        "DC001" => "DC01",
        "DC002" => "DC02",
        "DC003" => "DC03",
        "DC004" => "DC04",
        "DC005" => "DC05",
        "DC006" => "DC06",
        "DC007" => "DC07",
        "DC008" => "DC08",
        "DC009" => "DC09",
        "DC011" => "DC11",
        "DC012" => "DC12",
        "DC013" => "DC13",
        other => return other.to_string(),
    }
    .to_string()
}

fn parse_error_codes(codes_capture: Option<&[u8]>) -> Vec<String> {
    let raw = codes_capture.unwrap_or(b"all");
    raw.split(|&b| b == b',')
        .map(|chunk| String::from_utf8_lossy(chunk).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Maps error code (e.g. "DC01", or "all") -> set of 1-indexed line numbers
/// on which a `# noqa[: CODE[,CODE...]]` comment suppresses that code.
pub fn parse_noqa(code: &[u8]) -> HashMap<String, HashSet<u32>> {
    let mut noqa_lines: HashMap<String, HashSet<u32>> = HashMap::new();
    for (i, line) in code.split(|&b| b == b'\n').enumerate() {
        let lineno = (i + 1) as u32;
        if let Some(captures) = NOQA_REGEXP.captures(line) {
            let codes_group = captures.name("codes").map(|m| m.as_bytes());
            for error_code in parse_error_codes(codes_group) {
                let mapped = noqa_code_map(&error_code);
                noqa_lines.entry(mapped).or_default().insert(lineno);
            }
        }
    }
    noqa_lines
}

/// Checks if the reported line is annotated with `# noqa` (bare, or for this
/// specific `error_code`).
pub fn ignore_line(
    noqa_lines: &HashMap<String, HashSet<u32>>,
    lineno: u32,
    error_code: &str,
) -> bool {
    noqa_lines
        .get(error_code)
        .is_some_and(|s| s.contains(&lineno))
        || noqa_lines.get("all").is_some_and(|s| s.contains(&lineno))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_noqa_suppresses_all() {
        let parsed = parse_noqa(b"x = 1  # noqa\n");
        assert!(ignore_line(&parsed, 1, "DC01"));
        assert!(ignore_line(&parsed, 1, "DC99"));
    }

    #[test]
    fn specific_code_only_suppresses_that_code() {
        let parsed = parse_noqa(b"x = 1  # noqa: DC01\n");
        assert!(ignore_line(&parsed, 1, "DC01"));
        assert!(!ignore_line(&parsed, 1, "DC02"));
    }

    #[test]
    fn flake8_alias_codes_map_through() {
        let parsed = parse_noqa(b"import os  # noqa: F401\n");
        assert!(ignore_line(&parsed, 1, "DC07"));
    }

    #[test]
    fn multiple_codes_comma_separated() {
        let parsed = parse_noqa(b"x = 1  # noqa: DC01,DC02\n");
        assert!(ignore_line(&parsed, 1, "DC01"));
        assert!(ignore_line(&parsed, 1, "DC02"));
        assert!(!ignore_line(&parsed, 1, "DC03"));
    }

    #[test]
    fn case_insensitive_noqa_keyword() {
        let parsed = parse_noqa(b"x = 1  # NoQA: DC01\n");
        assert!(ignore_line(&parsed, 1, "DC01"));
    }

    #[test]
    fn no_comment_means_no_suppression() {
        let parsed = parse_noqa(b"x = 1\n");
        assert!(!ignore_line(&parsed, 1, "DC01"));
    }
}
