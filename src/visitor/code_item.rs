//! Port of `deadcode/visitor/code_item.py`. Python's `CodeItem` overrides
//! `__eq__`/`__hash__` so it can be used interchangeably with a plain string
//! as a dict key (name-based identity) — in Rust, callers that need that
//! just key their maps by `String` directly (see `nested_scopes.rs`), so no
//! custom `Eq`/`Hash` trick is needed here.

use std::path::PathBuf;

use crate::constants::UnusedCodeType;
use crate::data_types::Part;

#[derive(Debug, Clone)]
pub struct CodeItem {
    pub name: String,
    pub type_: UnusedCodeType,
    pub filename: PathBuf,
    pub code_parts: Vec<Part>,
    pub scope: Option<String>,
    pub inherits_from: Option<Vec<String>>,
    pub name_line: Option<u32>,
    pub name_column: Option<u32>,
    pub message: String,
    pub number_of_uses: u32,
}

impl CodeItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        type_: UnusedCodeType,
        filename: PathBuf,
        code_parts: Vec<Part>,
        scope: Option<String>,
        inherits_from: Option<Vec<String>>,
        name_line: Option<u32>,
        name_column: Option<u32>,
        message: String,
    ) -> Self {
        CodeItem {
            name,
            type_,
            filename,
            code_parts,
            scope,
            inherits_from,
            name_line,
            name_column,
            message,
            number_of_uses: 0,
        }
    }

    pub fn error_code(&self) -> &'static str {
        self.type_.error_code()
    }

    /// `.as_posix()` (not the OS-native separator) so output is forward-slash
    /// on every host OS, matching how the path was given on the command line.
    pub fn filename_with_position(&self) -> String {
        let mut out = path_as_posix(&self.filename);
        if let Some(line) = self.name_line {
            out.push(':');
            out.push_str(&line.to_string());
            if let Some(col) = self.name_column {
                out.push(':');
                out.push_str(&col.to_string());
                out.push(':');
            }
        }
        out
    }
}

/// `Path::as_posix()`-equivalent: forward slashes regardless of host OS.
pub fn path_as_posix(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_with_position_no_line() {
        let item = CodeItem::new(
            "Foo".into(),
            UnusedCodeType::Variable,
            PathBuf::from("foo.py"),
            vec![],
            None,
            None,
            None,
            None,
            String::new(),
        );
        assert_eq!(item.filename_with_position(), "foo.py");
    }

    #[test]
    fn filename_with_position_line_and_column() {
        let item = CodeItem::new(
            "Foo".into(),
            UnusedCodeType::Variable,
            PathBuf::from("foo.py"),
            vec![],
            None,
            None,
            Some(3),
            Some(4),
            String::new(),
        );
        assert_eq!(item.filename_with_position(), "foo.py:3:4:");
    }
}
