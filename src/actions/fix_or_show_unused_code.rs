//! Port of `deadcode/actions/fix_or_show_unused_code.py`.

use std::collections::BTreeMap;

use similar::TextDiff;

use crate::actions::merge_overlapping_file_parts::merge_overlapping_file_parts;
use crate::actions::remove_file_parts_from_content::remove_file_parts_from_content;
use crate::data_types::{Args, Part};
use crate::utils::add_colors_to_diff::add_colors_to_diff;
use crate::utils::fnmatch;
use crate::visitor::code_item::{path_as_posix, CodeItem};

/// Splits raw bytes into lines the same way Python's `f.readlines()` does:
/// each line keeps its trailing `\n` (except possibly the last).
fn readlines(content: &[u8]) -> Vec<Vec<u8>> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in content.iter().enumerate() {
        if b == b'\n' {
            lines.push(content[start..=i].to_vec());
            start = i + 1;
        }
    }
    if start < content.len() {
        lines.push(content[start..].to_vec());
    }
    lines
}

pub fn fix_or_show_unused_code(unused_items: &[CodeItem], args: &Args) -> String {
    // BTreeMap (not HashMap) to make grouping order deterministic
    // (alphabetical by filename), matching the practical effect Python gets
    // from `dict` preserving first-seen-key insertion order over
    // already-sorted `unused_items` (see `get_unused_code_items`'s final
    // sort by filename).
    let mut filename_to_items: BTreeMap<String, Vec<&CodeItem>> = BTreeMap::new();
    for item in unused_items {
        filename_to_items
            .entry(path_as_posix(&item.filename))
            .or_default()
            .push(item);
    }

    let mut result: Vec<Vec<u8>> = Vec::new();

    for (filename, file_items) in &filename_to_items {
        let file_parts: Vec<Part> = file_items
            .iter()
            .flat_map(|item| item.code_parts.iter().copied())
            .collect();
        let unused_file_parts = merge_overlapping_file_parts(&file_parts);

        let Ok(original_content) = std::fs::read(filename) else {
            continue;
        };
        let file_content_lines = readlines(&original_content);

        let updated_content_lines =
            remove_file_parts_from_content(&file_content_lines, &unused_file_parts);
        let updated_content: Vec<u8> = updated_content_lines.concat();

        // Matches Python `bytes.strip()`'s whitespace set exactly (adds 0x0b
        // vertical-tab, which Rust's `u8::is_ascii_whitespace()` excludes).
        let is_blank = updated_content
            .iter()
            .all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c));
        if !is_blank {
            if args.only.is_empty() || fnmatch::match_any(filename, &args.only, true) {
                if args.dry {
                    let old_text = String::from_utf8_lossy(&original_content).into_owned();
                    let new_text = String::from_utf8_lossy(&updated_content).into_owned();
                    let diff = TextDiff::from_lines(&old_text, &new_text);
                    let diff_text = diff
                        .unified_diff()
                        .context_radius(3)
                        .header(filename, filename)
                        .to_string();
                    let diff_bytes = diff_text.into_bytes();
                    if args.no_color {
                        result.push(diff_bytes);
                    } else {
                        result.push(add_colors_to_diff(&diff_bytes));
                    }
                } else if args.fix && std::fs::write(filename, &updated_content).is_err() {
                    continue;
                }
            }
        } else {
            let _ = std::fs::remove_file(filename);
        }
    }

    if result.is_empty() {
        String::new()
    } else {
        String::from_utf8_lossy(&result.join(&b'\n')).into_owned()
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::constants::UnusedCodeType;
    use std::path::PathBuf;

    fn item(filename: &str, name: &str, type_: UnusedCodeType, part: Part) -> CodeItem {
        CodeItem::new(
            name.to_string(),
            type_,
            PathBuf::from(filename),
            vec![part],
            None,
            None,
            Some(part.line_start),
            Some(part.col_start),
            String::new(),
        )
    }

    #[test]
    fn dry_run_produces_expected_unified_diff() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("foo.py");
        std::fs::write(
            &file_path,
            "class UnusedClass:\n    pass\n\nprint(\"Dont change this file\")\n",
        )
        .unwrap();

        let filename_str = path_as_posix(&file_path);
        let items = vec![item(
            &filename_str,
            "UnusedClass",
            UnusedCodeType::Class,
            Part::new(1, 3, 0, 0),
        )];

        let mut args = Args::default();
        args.dry = true;
        args.no_color = true;

        let output = fix_or_show_unused_code(&items, &args);
        let expected = format!(
            "--- {f}\n+++ {f}\n@@ -1,4 +1 @@\n-class UnusedClass:\n-    pass\n-\n print(\"Dont change this file\")\n",
            f = filename_str
        );
        assert_eq!(output, expected);

        // --dry must never modify the file on disk.
        let on_disk = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            on_disk,
            "class UnusedClass:\n    pass\n\nprint(\"Dont change this file\")\n"
        );
    }

    #[test]
    fn fix_writes_updated_content_and_removes_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("foo.py");
        std::fs::write(&file_path, "class MyTest:\n    pass\n").unwrap();
        let filename_str = path_as_posix(&file_path);

        let items = vec![item(
            &filename_str,
            "MyTest",
            UnusedCodeType::Class,
            Part::new(1, 2, 0, 8),
        )];
        let mut args = Args::default();
        args.fix = true;

        fix_or_show_unused_code(&items, &args);
        assert!(
            !file_path.exists(),
            "file reduced to whitespace-only should be removed"
        );
    }

    #[test]
    fn only_filter_restricts_which_files_get_a_diff() {
        let dir = tempfile::tempdir().unwrap();
        let foo = dir.path().join("foo.py");
        let bar = dir.path().join("bar.py");
        std::fs::write(&foo, "class Foo:\n    pass\n\nx = 1\n").unwrap();
        std::fs::write(&bar, "class Bar:\n    pass\n\ny = 1\n").unwrap();
        let foo_str = path_as_posix(&foo);
        let bar_str = path_as_posix(&bar);

        let items = vec![
            item(
                &foo_str,
                "Foo",
                UnusedCodeType::Class,
                Part::new(1, 3, 0, 0),
            ),
            item(
                &bar_str,
                "Bar",
                UnusedCodeType::Class,
                Part::new(1, 3, 0, 0),
            ),
        ];
        let mut args = Args::default();
        args.dry = true;
        args.no_color = true;
        args.only = vec![foo_str.clone()];

        let output = fix_or_show_unused_code(&items, &args);
        assert!(output.contains(&foo_str));
        assert!(!output.contains(&bar_str));
    }
}
