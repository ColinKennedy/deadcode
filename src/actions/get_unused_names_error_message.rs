//! Port of `deadcode/actions/get_unused_names_error_message.py`.

use crate::data_types::Args;
use crate::utils::fnmatch;
use crate::visitor::code_item::{path_as_posix, CodeItem};

/// `None` means "nothing to report" (distinct from `Some(String::new())`,
/// which `--quiet` produces even when violations WERE found — the exit-code
/// logic downstream must check `is_some()`, not truthiness, to get this
/// right; see the `--quiet` regression covered in the ported test suite).
pub fn get_unused_names_error_message(unused_names: &[CodeItem], args: &Args) -> Option<String> {
    if unused_names.is_empty() {
        return None;
    }
    if args.quiet {
        return Some(String::new());
    }
    if args.count {
        return Some(unused_names.len().to_string());
    }

    let mut messages: Vec<String> = Vec::new();
    for item in unused_names {
        if args.only.is_empty()
            || fnmatch::match_any(&path_as_posix(&item.filename), &args.only, true)
        {
            let mut message = format!(
                "{} \x1b[91m{}\x1b[0m ",
                item.filename_with_position(),
                item.error_code()
            );
            if !item.message.is_empty() {
                message.push_str(&item.message);
            } else {
                message.push_str(&format!(
                    "{} `\x1b[1m{}\x1b[0m` is never used",
                    item.type_.display_name(),
                    item.name
                ));
            }
            if args.no_color {
                message = message
                    .replace("\x1b[91m", "")
                    .replace("\x1b[1m", "")
                    .replace("\x1b[0m", "");
            }
            messages.push(message);
        }
    }

    if args.fix {
        let mut message = format!(
            "\nRemoved \x1b[1m{}\x1b[0m unused code item{}!",
            messages.len(),
            if messages.len() > 1 { "s" } else { "" }
        );
        if args.no_color {
            message = message.replace("\x1b[1m", "").replace("\x1b[0m", "");
        }
        messages.push(message);
    }

    Some(messages.join("\n"))
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::constants::UnusedCodeType;
    use std::path::PathBuf;

    fn variable_item(filename: &str, name: &str, line: u32, col: u32) -> CodeItem {
        CodeItem::new(
            name.to_string(),
            UnusedCodeType::Variable,
            PathBuf::from(filename),
            vec![],
            None,
            None,
            Some(line),
            Some(col),
            String::new(),
        )
    }

    #[test]
    fn no_items_returns_none() {
        assert_eq!(get_unused_names_error_message(&[], &Args::default()), None);
    }

    #[test]
    fn quiet_returns_empty_string_even_with_findings() {
        let items = vec![variable_item("foo.py", "x", 1, 0)];
        let mut args = Args::default();
        args.quiet = true;
        assert_eq!(
            get_unused_names_error_message(&items, &args),
            Some(String::new())
        );
    }

    #[test]
    fn count_returns_count_as_string() {
        let items = vec![
            variable_item("foo.py", "x", 1, 0),
            variable_item("foo.py", "y", 2, 0),
        ];
        let mut args = Args::default();
        args.count = true;
        assert_eq!(
            get_unused_names_error_message(&items, &args),
            Some("2".to_string())
        );
    }

    #[test]
    fn default_message_format_with_color() {
        let items = vec![variable_item("foo.py", "x", 1, 0)];
        let msg = get_unused_names_error_message(&items, &Args::default()).unwrap();
        assert_eq!(
            msg,
            "foo.py:1:0: \x1b[91mDC01\x1b[0m Variable `\x1b[1mx\x1b[0m` is never used"
        );
    }

    #[test]
    fn no_color_strips_ansi_codes() {
        let items = vec![variable_item("foo.py", "x", 1, 0)];
        let mut args = Args::default();
        args.no_color = true;
        let msg = get_unused_names_error_message(&items, &args).unwrap();
        assert_eq!(msg, "foo.py:1:0: DC01 Variable `x` is never used");
    }

    #[test]
    fn fix_appends_singular_removed_message() {
        let items = vec![variable_item("foo.py", "x", 1, 0)];
        let mut args = Args::default();
        args.fix = true;
        args.no_color = true;
        let msg = get_unused_names_error_message(&items, &args).unwrap();
        assert!(msg.ends_with("\nRemoved 1 unused code item!"));
    }

    #[test]
    fn fix_appends_plural_removed_message() {
        let items = vec![
            variable_item("foo.py", "x", 1, 0),
            variable_item("foo.py", "y", 2, 0),
        ];
        let mut args = Args::default();
        args.fix = true;
        args.no_color = true;
        let msg = get_unused_names_error_message(&items, &args).unwrap();
        assert!(msg.ends_with("\nRemoved 2 unused code items!"));
    }

    #[test]
    fn only_filter_restricts_reported_items() {
        let items = vec![
            variable_item("foo.py", "x", 1, 0),
            variable_item("bar.py", "y", 1, 0),
        ];
        let mut args = Args::default();
        args.only = vec!["foo.py".to_string()];
        args.no_color = true;
        let msg = get_unused_names_error_message(&items, &args).unwrap();
        assert!(msg.contains("foo.py"));
        assert!(!msg.contains("bar.py"));
    }

    #[test]
    fn dc11_empty_file_has_no_line_column_and_uses_its_own_message() {
        let item = CodeItem::new(
            "foo.py".to_string(),
            UnusedCodeType::UnusedFile,
            PathBuf::from("foo.py"),
            vec![],
            None,
            None,
            None,
            None,
            "Empty file".to_string(),
        );
        let mut args = Args::default();
        args.no_color = true;
        let msg = get_unused_names_error_message(&[item], &args).unwrap();
        assert_eq!(msg, "foo.py DC11 Empty file");
    }
}
