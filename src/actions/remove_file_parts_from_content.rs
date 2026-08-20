//! Port of `deadcode/actions/remove_file_parts_from_content.py`.
//!
//! The port is byte-exact with the Python original except for two bugs that
//! were deliberately fixed rather than reproduced, because both make `--fix`
//! emit source that is not valid Python:
//!
//! 1. **Dangling import stubs.** Removing the last name from an import left
//!    `import `, `from foo import `, or `from foo import (\n)` behind. The
//!    Python original carries this as a known defect
//!    (`# TODO: empty imports statements should be removed as well.` in
//!    `tests/fix/test_unused_imports.py`); see
//!    `remove_dangling_import_statements`.
//! 2. **Only one removal per line.** The main loop advanced
//!    `unused_part_index` at most once per source line, so given
//!    `import os, sys` with both unused it removed only `os` and then
//!    misaligned every later part. Single-line parts sharing a line are now
//!    all applied, right-to-left.
//!
//! Every other documented quirk (the `pass`-insertion rules, the blank-line
//! bookkeeping, Python's forgiving slice semantics) is still reproduced
//! exactly.

use crate::data_types::Part;

/// Matches Python `bytes.strip()`'s default whitespace set (ASCII
/// space/tab/CR/LF/VT/FF), not the wider Unicode `char::is_whitespace()` set.
fn is_py_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn lstrip(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|&b| !is_py_whitespace(b))
        .unwrap_or(line.len());
    &line[start..]
}

fn rstrip(line: &[u8]) -> &[u8] {
    let end = line
        .iter()
        .rposition(|&b| !is_py_whitespace(b))
        .map_or(0, |i| i + 1);
    &line[..end]
}

fn strip(line: &[u8]) -> &[u8] {
    rstrip(lstrip(line))
}

/// Clamped equivalent of Python's forgiving `line[:idx]` slicing.
fn slice_to(line: &[u8], idx: u32) -> Vec<u8> {
    let idx = (idx as usize).min(line.len());
    line[..idx].to_vec()
}

/// Clamped equivalent of Python's forgiving `line[idx:]` slicing.
fn slice_from(line: &[u8], idx: u32) -> Vec<u8> {
    let idx = (idx as usize).min(line.len());
    line[idx..].to_vec()
}

fn ends_with_semicolon(line: &[u8]) -> bool {
    strip(line).ends_with(b":")
}

fn get_indentation(line: &[u8]) -> Vec<u8> {
    let end = line
        .iter()
        .position(|&b| !is_py_whitespace(b))
        .unwrap_or(line.len());
    line[..end].to_vec()
}

fn indentation_is_not_childs(previous_line: &[u8], current_line: &[u8]) -> bool {
    get_indentation(previous_line).len() >= get_indentation(current_line).len()
}

fn remove_as_from_end(line: &[u8]) -> Vec<u8> {
    let line_rstrip = rstrip(line);
    if !line_rstrip.ends_with(b"as") {
        return line.to_vec();
    }
    let truncated = &line_rstrip[..line_rstrip.len() - 2];
    let line_with_removed_as = rstrip(truncated);
    if truncated != line_with_removed_as {
        line_with_removed_as.to_vec()
    } else {
        line.to_vec()
    }
}

fn remove_comma_from_beginning(line: &[u8]) -> Vec<u8> {
    let stripped = lstrip(line);
    if !stripped.starts_with(b",") {
        return line.to_vec();
    }
    lstrip(&stripped[1..]).to_vec()
}

fn concat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out
}

/// Cuts `[from_col, to_col)` out of a single line, applying the original's
/// two repair rules: a leftover leading `=` means the whole assignment tail
/// goes, and otherwise a trailing `as` / leading `,` left stranded by the cut
/// is tidied away.
fn splice_out(line: &[u8], from_col: u32, to_col: u32) -> Vec<u8> {
    let combined = concat(&slice_to(line, from_col), &slice_from(line, to_col));
    if strip(&combined).starts_with(b"=") {
        slice_to(line, from_col)
    } else {
        concat(
            &remove_as_from_end(&slice_to(line, from_col)),
            &remove_comma_from_beginning(&slice_from(line, to_col)),
        )
    }
}

/// What an import statement looks like once every one of its names has been
/// removed. Both forms are *invalid* Python, so they can only ever be removal
/// debris — a file deadcode parsed successfully cannot have contained them.
enum ImportStub {
    /// `import` / `from x import` with nothing following: always dead.
    Bare,
    /// `from x import (` — dead only if the parenthesized list is now empty.
    OpenParen,
    NotAStub,
}

/// Drops everything from the first `#` onward.
///
/// Only ever applied to candidate import lines, where this is exact: an
/// import statement cannot contain a string literal, so the first `#` always
/// begins a real comment.
fn strip_trailing_comment(line: &[u8]) -> &[u8] {
    match line.iter().position(|&b| b == b'#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn classify_import_stub(line: &[u8]) -> ImportStub {
    let stripped = strip(strip_trailing_comment(line));
    if stripped == b"import" {
        return ImportStub::Bare;
    }
    if !stripped.starts_with(b"from") {
        return ImportStub::NotAStub;
    }
    // A surviving name always follows `import`, so ending there means the
    // list was emptied. `import` is a keyword and can never be a name.
    if stripped.ends_with(b"import") {
        return ImportStub::Bare;
    }
    if stripped.ends_with(b"import(") || stripped.ends_with(b"import (") {
        return ImportStub::OpenParen;
    }
    ImportStub::NotAStub
}

/// For a `from x import (` opened at `open_index`, returns the index of its
/// closing `)` when the list between them holds no names, or `None` when a
/// name survived (so the statement is still valid and must be kept).
///
/// Comment-only lines inside an emptied block are treated as part of the dead
/// statement: the whole thing is being deleted, and the comment documented
/// imports that no longer exist.
fn empty_paren_block_end(lines: &[Vec<u8>], open_index: usize) -> Option<usize> {
    for (index, line) in lines.iter().enumerate().skip(open_index + 1) {
        let stripped = strip(strip_trailing_comment(line));
        if stripped.is_empty() {
            continue;
        }
        if stripped.starts_with(b")") {
            // Trailing code after the `)` (e.g. `) ; x = 1`) means this is not
            // a clean standalone statement — leave it alone.
            return if strip(&stripped[1..]).is_empty() {
                Some(index)
            } else {
                None
            };
        }
        return None;
    }
    None
}

/// Removes import statements whose names were all deleted, which would
/// otherwise be left behind as syntactically invalid stubs.
fn remove_dangling_import_statements(lines: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut kept: Vec<Vec<u8>> = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        match classify_import_stub(&lines[index]) {
            ImportStub::Bare => index += 1,
            ImportStub::OpenParen => match empty_paren_block_end(&lines, index) {
                Some(close_index) => index = close_index + 1,
                None => {
                    kept.push(lines[index].clone());
                    index += 1;
                }
            },
            ImportStub::NotAStub => {
                kept.push(lines[index].clone());
                index += 1;
            }
        }
    }
    kept
}

pub fn remove_file_parts_from_content(
    content_lines: &[Vec<u8>],
    unused_file_parts: &[Part],
) -> Vec<Vec<u8>> {
    // The loop below walks lines and parts in lockstep, so parts must be in
    // source order. `merge_overlapping_file_parts` pushes merged parts onto
    // the end of its result without re-sorting, so its output is not reliably
    // ordered; sorting here is a no-op in the common case and removes that
    // coupling. `Part`'s `Ord` is lexicographic over
    // (line_start, line_end, col_start, col_end).
    let mut unused_file_parts = unused_file_parts.to_vec();
    unused_file_parts.sort();
    let unused_file_parts = &unused_file_parts[..];

    let mut updated_content_lines: Vec<Vec<u8>> = Vec::new();
    let mut unused_part_index: usize = 0;

    let mut previous_non_removed_line: Vec<u8> = Vec::new();
    let mut was_block_removed = false;
    let mut indentation_of_first_removed_line: Vec<u8> = Vec::new();
    let mut empty_lines_in_a_row: Vec<Vec<u8>> = Vec::new();
    let mut empty_lines_before_removed_block: Vec<Vec<u8>> = Vec::new();

    for (i, raw_line) in content_lines.iter().enumerate() {
        let current_lineno = (i + 1) as u32;
        let mut line = raw_line.clone();

        let (from_line, to_line, from_col, to_col) = match unused_file_parts.get(unused_part_index)
        {
            Some(p) => (p.line_start, p.line_end, p.col_start, p.col_end),
            None => (0, 0, 0, 0),
        };

        if current_lineno > from_line && current_lineno < to_line {
            continue;
        } else if current_lineno == from_line {
            indentation_of_first_removed_line = get_indentation(&line);

            if from_line == to_line {
                // Consume *every* single-line part on this line, not just the
                // first: `import os, sys` with both unused yields two parts
                // here, and stopping after one both left `sys` in place and
                // misaligned all later parts against later lines.
                let mut spans: Vec<(u32, u32)> = Vec::new();
                while let Some(part) = unused_file_parts.get(unused_part_index) {
                    if part.line_start == current_lineno && part.line_end == current_lineno {
                        spans.push((part.col_start, part.col_end));
                        unused_part_index += 1;
                    } else {
                        break;
                    }
                }

                // Right-to-left, so each cut leaves the byte offsets of the
                // spans still to its left untouched.
                spans.sort_by_key(|(col_start, _)| std::cmp::Reverse(*col_start));
                for (span_from_col, span_to_col) in spans {
                    line = splice_out(&line, span_from_col, span_to_col);
                }

                if strip(&line).is_empty() {
                    std::mem::swap(
                        &mut empty_lines_before_removed_block,
                        &mut empty_lines_in_a_row,
                    );
                    was_block_removed = true;
                }
            } else {
                line = slice_to(&line, from_col);
            }

            if !strip(&line).is_empty() && !line.starts_with(b"#") {
                previous_non_removed_line = line.clone();
                updated_content_lines.push(line);
            }
        } else if current_lineno == to_line {
            line = slice_from(&line, to_col);
            if lstrip(&line).starts_with(b",") {
                line = lstrip(&lstrip(&line)[1..]).to_vec();
            }
            unused_part_index += 1;
            if !strip(&line).is_empty() && !line.starts_with(b"#") {
                updated_content_lines.push(line);
            }
            std::mem::swap(
                &mut empty_lines_before_removed_block,
                &mut empty_lines_in_a_row,
            );
            was_block_removed = true;
        } else if strip(&line).is_empty() {
            empty_lines_in_a_row.push(line);
            continue;
        } else {
            if !was_block_removed {
                updated_content_lines.append(&mut empty_lines_in_a_row);
                empty_lines_in_a_row.clear();
            } else {
                let next_line_after_removed_block = line.clone();
                if ends_with_semicolon(&previous_non_removed_line) {
                    if indentation_is_not_childs(
                        &previous_non_removed_line,
                        &next_line_after_removed_block,
                    ) {
                        updated_content_lines
                            .push(concat(&indentation_of_first_removed_line, b"pass\n"));
                    }
                    if indentation_is_not_childs(
                        &previous_non_removed_line,
                        &next_line_after_removed_block,
                    ) {
                        updated_content_lines.append(&mut empty_lines_in_a_row);
                        empty_lines_in_a_row.clear();
                        empty_lines_before_removed_block.clear();
                    } else {
                        updated_content_lines.append(&mut empty_lines_before_removed_block);
                        empty_lines_before_removed_block.clear();
                        empty_lines_in_a_row.clear();
                    }
                } else {
                    updated_content_lines.append(&mut empty_lines_before_removed_block);
                    empty_lines_before_removed_block.clear();
                    empty_lines_in_a_row.clear();
                }
            }
            was_block_removed = false;
            previous_non_removed_line = line.clone();
            updated_content_lines.push(line);
        }
    }

    if was_block_removed && ends_with_semicolon(&previous_non_removed_line) {
        updated_content_lines.push(concat(&indentation_of_first_removed_line, b"pass\n"));
    }

    remove_dangling_import_statements(updated_content_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<Vec<u8>> {
        source
            .split_inclusive('\n')
            .map(|l| l.as_bytes().to_vec())
            .collect()
    }

    fn render(result: &[Vec<u8>]) -> String {
        String::from_utf8(result.concat()).unwrap()
    }

    /// `import os` -> the whole statement, not `import `.
    #[test]
    fn emptied_single_line_import_is_dropped() {
        let content = lines("import os\nprint(1)\n");
        let result = remove_file_parts_from_content(&content, &[Part::new(1, 1, 7, 9)]);
        assert_eq!(render(&result), "print(1)\n");
    }

    /// `from foo import bar` -> gone, not `from foo import `.
    #[test]
    fn emptied_from_import_is_dropped() {
        let content = lines("from foo import bar\nprint(1)\n");
        let result = remove_file_parts_from_content(&content, &[Part::new(1, 1, 16, 19)]);
        assert_eq!(render(&result), "print(1)\n");
    }

    /// A partially-emptied import keeps its surviving names.
    #[test]
    fn partially_emptied_import_is_kept() {
        let content = lines("import os, sys\nprint(sys)\n");
        let result = remove_file_parts_from_content(&content, &[Part::new(1, 1, 7, 9)]);
        assert_eq!(render(&result), "import sys\nprint(sys)\n");
    }

    /// Both names on one line must go — this is the bug where only the first
    /// part was consumed and every later part then misaligned.
    #[test]
    fn every_part_on_one_line_is_applied() {
        let content = lines("import os, sys\nprint(1)\n");
        let result = remove_file_parts_from_content(
            &content,
            &[Part::new(1, 1, 7, 9), Part::new(1, 1, 11, 14)],
        );
        assert_eq!(render(&result), "print(1)\n");
    }

    /// Parts are applied right-to-left, so an earlier span's byte offsets stay
    /// valid after a later one is cut. Reversed input must give the same answer.
    #[test]
    fn same_line_parts_are_order_independent() {
        let content = lines("import os, sys\nprint(1)\n");
        let forward = remove_file_parts_from_content(
            &content,
            &[Part::new(1, 1, 7, 9), Part::new(1, 1, 11, 14)],
        );
        let reversed = remove_file_parts_from_content(
            &content,
            &[Part::new(1, 1, 11, 14), Part::new(1, 1, 7, 9)],
        );
        assert_eq!(render(&forward), render(&reversed));
    }

    #[test]
    fn emptied_parenthesized_import_block_is_dropped() {
        let content = lines("from foo import (\n    a,\n)\nprint(1)\n");
        let result = remove_file_parts_from_content(&content, &[Part::new(2, 2, 4, 5)]);
        assert_eq!(render(&result), "print(1)\n");
    }

    /// A parenthesized block that still holds a name is left alone.
    #[test]
    fn parenthesized_import_with_survivor_is_kept() {
        let content = lines("from foo import (\n    a,\n    b,\n)\nprint(b)\n");
        let result = remove_file_parts_from_content(&content, &[Part::new(2, 2, 4, 5)]);
        assert!(render(&result).contains("from foo import ("));
        assert!(render(&result).contains("b,"));
    }

    #[test]
    fn classify_recognizes_only_genuine_stubs() {
        assert!(matches!(
            classify_import_stub(b"import\n"),
            ImportStub::Bare
        ));
        assert!(matches!(
            classify_import_stub(b"from foo import\n"),
            ImportStub::Bare
        ));
        assert!(matches!(
            classify_import_stub(b"from . import\n"),
            ImportStub::Bare
        ));
        assert!(matches!(
            classify_import_stub(b"from foo import (\n"),
            ImportStub::OpenParen
        ));
        // Real statements and unrelated code are never stubs.
        assert!(matches!(
            classify_import_stub(b"import os\n"),
            ImportStub::NotAStub
        ));
        assert!(matches!(
            classify_import_stub(b"from foo import bar\n"),
            ImportStub::NotAStub
        ));
        assert!(matches!(
            classify_import_stub(b"x = 1\n"),
            ImportStub::NotAStub
        ));
        // `importlib` must not be mistaken for the `import` keyword.
        assert!(matches!(
            classify_import_stub(b"from importlib import util\n"),
            ImportStub::NotAStub
        ));
    }

    /// A trailing comment must not hide a stub from detection.
    #[test]
    fn stub_with_trailing_comment_is_recognized() {
        assert!(matches!(
            classify_import_stub(b"import   # leftover\n"),
            ImportStub::Bare
        ));
    }

    /// Non-import content is never touched by the stub pass.
    #[test]
    fn unrelated_lines_survive_the_stub_pass() {
        let content = lines("x = 1\ny = 2\n");
        let result = remove_file_parts_from_content(&content, &[]);
        assert_eq!(render(&result), "x = 1\ny = 2\n");
    }
}
