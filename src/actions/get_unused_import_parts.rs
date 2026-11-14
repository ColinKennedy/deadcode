//! Port of `deadcode/actions/get_unused_import_parts.py`.
//!
//! A finding's own code part covers only its alias (`os` in
//! `import os, sys`), so cutting it out leaves the separating commas, an
//! emptied `from x import (\n)`, or a dangling `;` behind. This module turns
//! unused-import findings into removal parts that keep the surrounding
//! statement valid:
//!
//! * every non-wildcard alias of a statement unused -> the whole statement,
//!   plus a neighbouring `;` separator and any trailing comment;
//! * otherwise -> each unused alias together with one adjacent comma.
//!
//! Findings this cannot place (the file no longer parses, or no comma is
//! found where one must be) fall back to their own code parts, as before.

use std::collections::HashSet;

use ruff_python_ast::statement_visitor::{walk_stmt, StatementVisitor};
use ruff_python_ast::{Alias, Stmt};
use ruff_text_size::{Ranged, TextRange};

use crate::actions::parse_abstract_syntax_tree::parse_abstract_syntax_tree;
use crate::data_types::Part;
use crate::utils::line_index::LineIndex;
use crate::visitor::code_item::CodeItem;

/// (local name, line, column) -- how a finding names the alias it reports.
type AliasKey = (String, u32, u32);

/// A source position as (1-indexed line, 0-indexed byte column).
type Position = (u32, u32);

struct Source<'a> {
    lines: Vec<&'a [u8]>,
    index: LineIndex,
}

impl<'a> Source<'a> {
    fn new(content: &'a str) -> Self {
        Source {
            lines: content
                .as_bytes()
                .split_inclusive(|&b| b == b'\n')
                .collect(),
            index: LineIndex::new(content),
        }
    }

    fn position(&self, offset: ruff_text_size::TextSize) -> Position {
        let (line, column) = self.index.line_col(offset);
        (line as u32, column as u32)
    }

    fn start(&self, range: TextRange) -> Position {
        self.position(range.start())
    }

    fn end(&self, range: TextRange) -> Position {
        self.position(range.end())
    }

    fn line(&self, line_number: u32) -> &'a [u8] {
        line_number
            .checked_sub(1)
            .and_then(|index| self.lines.get(index as usize))
            .copied()
            .unwrap_or(b"")
    }

    /// Length of the line without its terminator.
    fn content_end(&self, line_number: u32) -> u32 {
        let line = self.line(line_number);
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.len() as u32
    }

    fn first_non_blank_from(&self, line_number: u32, start: u32) -> Option<u32> {
        let line = self.line(line_number);
        (start..self.content_end(line_number)).find(|&column| !is_blank(line[column as usize]))
    }

    fn last_non_blank_before(&self, line_number: u32, end: u32) -> Option<u32> {
        let line = self.line(line_number);
        (0..end.min(line.len() as u32))
            .rev()
            .find(|&column| !is_blank(line[column as usize]))
    }

    fn byte_at(&self, line_number: u32, column: u32) -> Option<u8> {
        self.line(line_number).get(column as usize).copied()
    }

    /// The first comma between two positions.
    fn comma_between(&self, start: Position, end: Position) -> Option<Position> {
        (start.0..=end.0).find_map(|line_number| {
            let line = self.line(line_number);
            let from = if line_number == start.0 { start.1 } else { 0 };
            let to = if line_number == end.0 {
                end.1
            } else {
                line.len() as u32
            };
            (from..to.min(line.len() as u32))
                .find(|&column| line[column as usize] == b',')
                .map(|column| (line_number, column))
        })
    }

    /// The column just past a comma and any blanks following it.
    fn after_comma_and_blanks(&self, comma: Position) -> u32 {
        let line = self.line(comma.0);
        let mut column = comma.1 + 1;
        while (column as usize) < line.len() && is_blank(line[column as usize]) {
            column += 1;
        }
        column
    }
}

fn is_blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn alias_key(source: &Source, alias: &Alias) -> AliasKey {
    let name = alias.asname.as_ref().map_or_else(
        || alias.name.split('.').next().unwrap_or_default().to_string(),
        |asname| asname.to_string(),
    );
    let (line, column) = source.start(alias.range());
    (name, line, column)
}

#[derive(Default)]
struct ImportStatements<'a> {
    found: Vec<(TextRange, &'a [Alias])>,
}

impl<'a> StatementVisitor<'a> for ImportStatements<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::Import(import) => self.found.push((import.range, &import.names)),
            Stmt::ImportFrom(import) => self.found.push((import.range, &import.names)),
            _ => walk_stmt(self, statement),
        }
    }
}

/// The part removing a whole import statement, widened to swallow a `;`
/// separator on either side and anything after it that is only a comment.
fn whole_statement_part(source: &Source, range: TextRange) -> Part {
    let (start_line, mut start_column) = source.start(range);
    let (end_line, mut end_column) = source.end(range);

    let following = source.first_non_blank_from(end_line, end_column);
    if following.is_some_and(|column| source.byte_at(end_line, column) == Some(b';')) {
        end_column = following.unwrap_or(end_column) + 1;
        if let Some(next) = source.first_non_blank_from(end_line, end_column) {
            end_column = next;
        }
    } else if let Some(preceding) = source.last_non_blank_before(start_line, start_column) {
        if source.byte_at(start_line, preceding) == Some(b';') {
            start_column = preceding;
        }
    }

    let line_end = source.content_end(end_line);
    let line = source.line(end_line);
    let remainder = line
        .get(end_column as usize..line_end as usize)
        .unwrap_or_default();
    let remainder = remainder.trim_ascii();
    if remainder.is_empty() || remainder.starts_with(b"#") {
        end_column = line_end;
    }

    Part::new(start_line, end_line, start_column, end_column)
}

/// Returns code parts to remove for unused-import findings.
pub fn get_unused_import_parts(content: &[u8], unused_import_items: &[&CodeItem]) -> Vec<Part> {
    let fallback = || -> Vec<Part> {
        unused_import_items
            .iter()
            .flat_map(|item| item.code_parts.iter().copied())
            .collect()
    };
    let Ok(content) = std::str::from_utf8(content) else {
        return fallback();
    };
    let Ok(module) = parse_abstract_syntax_tree(content) else {
        return fallback();
    };

    let removable: HashSet<AliasKey> = unused_import_items
        .iter()
        .filter_map(|item| Some((item.name.clone(), item.name_line?, item.name_column?)))
        .collect();
    let source = Source::new(content);
    let mut statements = ImportStatements::default();
    statements.visit_body(&module);

    let mut parts: Vec<Part> = Vec::new();
    let mut matched: HashSet<AliasKey> = HashSet::new();

    for (range, names) in statements.found {
        // A wildcard alias is never removable.
        let aliases: Vec<&Alias> = names
            .iter()
            .filter(|alias| alias.name.as_str() != "*")
            .collect();
        if aliases.is_empty() {
            continue;
        }
        let keys: Vec<AliasKey> = aliases
            .iter()
            .map(|alias| alias_key(&source, alias))
            .collect();

        if keys.iter().all(|key| removable.contains(key)) {
            parts.push(whole_statement_part(&source, range));
            matched.extend(keys);
            continue;
        }

        // Inside parentheses a trailing comma is valid, so upstream's rule
        // (every alias but the last takes the comma after it) is kept there.
        // Without them, a run of removed names after the last kept one must
        // take the commas before them, or `a, ` is left behind.
        let before_first_alias = TextRange::new(range.start(), aliases[0].start());
        let parenthesized = content[before_first_alias].contains('(');
        let takes_comma_after = |index: usize| {
            if parenthesized {
                index + 1 < aliases.len()
            } else {
                keys[index + 1..].iter().any(|key| !removable.contains(key))
            }
        };

        for (index, alias) in aliases.iter().enumerate() {
            let key = &keys[index];
            if !removable.contains(key) {
                continue;
            }
            let alias_start = source.start(alias.range());
            let alias_end = source.end(alias.range());

            if takes_comma_after(index) {
                // Take the comma after it.
                let next = aliases[index + 1];
                if let Some(comma) = source.comma_between(alias_end, source.start(next.range())) {
                    let end_column = source.after_comma_and_blanks(comma);
                    parts.push(Part::new(alias_start.0, comma.0, alias_start.1, end_column));
                }
            } else {
                // Take the comma before it (never the only alias: that case
                // removed the whole statement).
                let previous_end = source.end(aliases[index - 1].range());
                if let Some(comma) = source.comma_between(previous_end, alias_start) {
                    if previous_end.0 != alias_start.0 {
                        parts.push(Part::new(
                            comma.0,
                            comma.0,
                            comma.1,
                            source.content_end(comma.0),
                        ));
                        parts.push(Part::new(alias_start.0, alias_end.0, 0, alias_end.1));
                    } else {
                        parts.push(Part::new(comma.0, alias_end.0, comma.1, alias_end.1));
                    }
                }
            }
            // Matched even when no comma was found, as upstream does: falling
            // back to the bare alias span would only produce broken code.
            matched.insert(key.clone());
        }
    }

    let fallback_parts = unused_import_items.iter().filter(|item| {
        let key = item
            .name_line
            .zip(item.name_column)
            .map(|(line, column)| (item.name.clone(), line, column));
        !key.is_some_and(|key| matched.contains(&key))
    });
    for part in fallback_parts.flat_map(|item| item.code_parts.iter().copied()) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::remove_file_parts_from_content::remove_file_parts_from_content;
    use crate::constants::UnusedCodeType;
    use crate::visitor::dead_code_visitor::DeadCodeVisitor;

    /// Runs the real pipeline on `source` and returns the fixed text.
    fn fix(source: &str) -> String {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("m.py");
        std::fs::write(&path, source).unwrap();
        let args = crate::data_types::Args::default();
        let tach_index = crate::actions::parse_tach_config::TachIndex::default();
        let mut visitor = DeadCodeVisitor::new(&args, &tach_index);
        visitor.visit_files(&[path.to_string_lossy().into_owned()]);
        let items = visitor.get_unused_code_items();
        let imports: Vec<&CodeItem> = items
            .iter()
            .filter(|item| matches!(item.type_, UnusedCodeType::Import))
            .collect();
        let parts = get_unused_import_parts(source.as_bytes(), &imports);
        let parts =
            crate::actions::merge_overlapping_file_parts::merge_overlapping_file_parts(&parts);
        let lines: Vec<Vec<u8>> = source
            .split_inclusive('\n')
            .map(|line| line.as_bytes().to_vec())
            .collect();
        String::from_utf8(remove_file_parts_from_content(&lines, &parts).concat()).unwrap()
    }

    #[test]
    fn removes_names_from_a_parenthesized_import() {
        let fixed = fix("from os import (\n    path,\n    sep,\n    getcwd,\n)\nprint(sep)\n");
        assert_eq!(fixed, "from os import (\n    sep\n)\nprint(sep)\n");
    }

    #[test]
    fn removes_semicolon_separated_statements_and_their_comment() {
        let fixed = fix("import sys, json; import re  # comment\nprint(json)\n");
        assert_eq!(fixed, "import json\nprint(json)\n");
    }

    #[test]
    fn removes_a_name_from_a_backslash_continued_import() {
        let fixed = fix("from collections import \\\n    OrderedDict, deque\nprint(deque)\n");
        assert_eq!(
            fixed,
            "from collections import \\\n    deque\nprint(deque)\n"
        );
    }

    #[test]
    fn removes_the_last_name_with_the_comma_before_it() {
        let fixed = fix("from os import sep, path\nprint(sep)\n");
        assert_eq!(fixed, "from os import sep\nprint(sep)\n");
    }

    #[test]
    fn removes_several_trailing_names_with_their_commas() {
        let fixed = fix("from os import sep, path, getcwd, walk\nprint(sep)\n");
        assert_eq!(fixed, "from os import sep\nprint(sep)\n");
    }

    #[test]
    fn keeps_a_wildcard_import() {
        let fixed = fix("from os import *\nimport sys\n");
        assert_eq!(fixed, "from os import *\n");
    }
}
