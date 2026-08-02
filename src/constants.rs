//! Port of `deadcode/constants.py`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnusedCodeType {
    Attribute,
    Class,
    Function,
    Import,
    Method,
    Property,
    Variable,
    UnreachableCode,
    Name,
    UnusedFile,
    CommentedOutCode,
    IgnoreExpression,
}

impl UnusedCodeType {
    /// Matches Python's `type_.replace('_', ' ').capitalize()` used to build
    /// the default "`{Type}` `{name}` is never used" message.
    pub fn display_name(self) -> &'static str {
        match self {
            UnusedCodeType::Attribute => "Attribute",
            UnusedCodeType::Class => "Class",
            UnusedCodeType::Function => "Function",
            UnusedCodeType::Import => "Import",
            UnusedCodeType::Method => "Method",
            UnusedCodeType::Property => "Property",
            UnusedCodeType::Variable => "Variable",
            UnusedCodeType::UnreachableCode => "Unreachable code",
            UnusedCodeType::Name => "Name",
            UnusedCodeType::UnusedFile => "Unused file",
            UnusedCodeType::CommentedOutCode => "Commented out code",
            UnusedCodeType::IgnoreExpression => "Ignore expression",
        }
    }

    pub fn error_code(self) -> &'static str {
        match self {
            UnusedCodeType::Variable => "DC01",
            UnusedCodeType::Function => "DC02",
            UnusedCodeType::Class => "DC03",
            UnusedCodeType::Method => "DC04",
            UnusedCodeType::Attribute => "DC05",
            UnusedCodeType::Name => "DC06",
            UnusedCodeType::Import => "DC07",
            UnusedCodeType::Property => "DC08",
            UnusedCodeType::UnreachableCode => "DC09",
            // Note: DC10 is deliberately absent (matches upstream numbering gap).
            UnusedCodeType::UnusedFile => "DC11",
            UnusedCodeType::CommentedOutCode => "DC12",
            UnusedCodeType::IgnoreExpression => "DC13",
        }
    }
}
