//! Converts the parser's byte-offset `TextRange`s into (line, column) pairs
//! matching CPython's `ast` module convention: 1-indexed `lineno`, 0-indexed
//! `col_offset`, both counted in UTF-8 bytes (not chars/codepoints) within the
//! line — this matches CPython's post-3.8 AST column semantics.

use ruff_text_size::TextSize;

pub struct LineIndex {
    /// Byte offset of the start of each line. line_starts[0] == 0.
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        LineIndex { line_starts }
    }

    /// Returns (1-indexed line, 0-indexed byte column) for a byte offset.
    pub fn line_col(&self, offset: TextSize) -> (usize, usize) {
        let offset: u32 = offset.into();
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert_at) => insert_at - 1,
        };
        let col = offset - self.line_starts[line_idx];
        (line_idx + 1, col as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_first_column() {
        let idx = LineIndex::new("abc\ndef\n");
        assert_eq!(idx.line_col(TextSize::from(0)), (1, 0));
    }

    #[test]
    fn second_line() {
        let idx = LineIndex::new("abc\ndef\n");
        assert_eq!(idx.line_col(TextSize::from(4)), (2, 0));
        assert_eq!(idx.line_col(TextSize::from(6)), (2, 2));
    }

    #[test]
    fn offset_at_exact_newline_boundary() {
        let idx = LineIndex::new("ab\ncd\nef");
        assert_eq!(idx.line_col(TextSize::from(3)), (2, 0));
        assert_eq!(idx.line_col(TextSize::from(6)), (3, 0));
    }
}
