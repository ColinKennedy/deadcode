//! Port of `deadcode/actions/remove_file_parts_from_content.py`. Byte-exact,
//! including quirks (e.g. the known dangling-empty-import-stub behavior
//! documented in `tests/fix/test_unused_imports.py`) — deliberately not
//! "improved" during the port.

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

pub fn remove_file_parts_from_content(
    content_lines: &[Vec<u8>],
    unused_file_parts: &[Part],
) -> Vec<Vec<u8>> {
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
                let combined = concat(&slice_to(&line, from_col), &slice_from(&line, to_col));
                if strip(&combined).starts_with(b"=") {
                    line = slice_to(&line, from_col);
                } else {
                    line = concat(
                        &remove_as_from_end(&slice_to(&line, from_col)),
                        &remove_comma_from_beginning(&slice_from(&line, to_col)),
                    );
                }
                unused_part_index += 1;

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

    updated_content_lines
}
