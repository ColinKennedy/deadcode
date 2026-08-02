//! Port of `deadcode/utils/fix_indent.py`. Test-support utility (used by the
//! Python test suite's `BaseTestCase.assertFiles`/`assertUpdatedFiles` to
//! dedent fixture strings before comparison) — not used by the production
//! scan/fix pipeline itself, but kept as a first-class ported module since
//! the Rust test harness (Phase 9) needs the exact same normalization.

/// Finds the indentation of a first line and removes it from all following
/// lines. Mirrors `inspect.cleandoc`-style dedenting while keeping trailing
/// lines. Returns `None` only to mirror the Python signature's `Optional`
/// (the Python version returns `None` on a `UnicodeError` from `.expandtabs()`,
/// which cannot occur on already-decoded Rust `&[u8]`/`&str` input).
pub fn fix_indent(doc: &[u8]) -> Option<Vec<u8>> {
    let expanded = expand_tabs(doc);
    let mut lines: Vec<Vec<u8>> = split_lines(&expanded);

    // Find minimum indentation of any non-blank line (including the first).
    let mut margin = usize::MAX;
    for line in &lines {
        let content_len = line.len() - leading_whitespace_len(line);
        if content_len > 0 {
            let indent = line.len() - content_len;
            margin = margin.min(indent);
        }
    }

    if let Some(first) = lines.first_mut() {
        let lead = leading_whitespace_len(first);
        *first = first[lead..].to_vec();
    }
    if margin < usize::MAX {
        for line in lines.iter_mut().skip(1) {
            if margin <= line.len() {
                *line = line[margin..].to_vec();
            } else {
                line.clear();
            }
        }
    }

    // Remove leading blank lines only (trailing lines are kept, matching
    // the Python implementation's commented-out trailing-strip).
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }

    Some(lines.join(&b'\n'))
}

fn expand_tabs(doc: &[u8]) -> Vec<u8> {
    // Python's bytes.expandtabs() default tabsize is 8, expanding to the
    // next multiple-of-8 column, tracked per logical line (reset at '\n').
    let mut out = Vec::with_capacity(doc.len());
    let mut col = 0usize;
    for &b in doc {
        match b {
            b'\t' => {
                let spaces = 8 - (col % 8);
                out.extend(std::iter::repeat(b' ').take(spaces));
                col += spaces;
            }
            b'\n' => {
                out.push(b);
                col = 0;
            }
            _ => {
                out.push(b);
                col += 1;
            }
        }
    }
    out
}

fn split_lines(doc: &[u8]) -> Vec<Vec<u8>> {
    doc.split(|&b| b == b'\n').map(|s| s.to_vec()).collect()
}

fn leading_whitespace_len(line: &[u8]) -> usize {
    line.len() - line_lstrip(line).len()
}

/// Matches Python `bytes.lstrip()`'s default whitespace set exactly (ASCII
/// space/tab/CR/LF/VT/FF only — NOT the wider Unicode `char::is_whitespace()`
/// set, which would wrongly strip bytes like 0xA0).
fn is_py_bytes_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn line_lstrip(line: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < line.len() && is_py_bytes_whitespace(line[i]) {
        i += 1;
    }
    &line[i..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_is_not_removed_from_second_line() {
        let input = b"class MyTest:\n    pass\n";
        assert_eq!(fix_indent(input).unwrap(), input.to_vec());
    }
}
