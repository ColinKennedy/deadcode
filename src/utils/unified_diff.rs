//! Port of Python's `difflib.unified_diff` (and the `SequenceMatcher` behind
//! it), so `--dry` renders byte-for-byte the diff upstream does.
//!
//! A general-purpose diff (Myers, as in the `similar` crate) aligns repeated
//! lines -- blank lines above all -- differently from `SequenceMatcher`'s
//! longest-matching-block recursion, which moves lines between hunks and
//! changes hunk boundaries even when the result is the same.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Equal,
    Replace,
    Delete,
    Insert,
}

/// `(tag, i1, i2, j1, j2)`: `a[i1..i2]` becomes `b[j1..j2]`.
type Opcode = (Tag, usize, usize, usize, usize);

struct SequenceMatcher<'a> {
    a: &'a [Vec<u8>],
    b: &'a [Vec<u8>],
    /// Line -> its indices in `b`, without "popular" lines (`autojunk`).
    b2j: HashMap<&'a [u8], Vec<usize>>,
}

impl<'a> SequenceMatcher<'a> {
    fn new(a: &'a [Vec<u8>], b: &'a [Vec<u8>]) -> Self {
        let mut b2j: HashMap<&[u8], Vec<usize>> = HashMap::new();
        for (index, line) in b.iter().enumerate() {
            b2j.entry(line.as_slice()).or_default().push(index);
        }
        // `autojunk`: in a long `b`, lines making up over 1% of it are not
        // used to seed matches.
        if b.len() >= 200 {
            let limit = b.len() / 100 + 1;
            b2j.retain(|_, indices| indices.len() <= limit);
        }
        SequenceMatcher { a, b, b2j }
    }

    /// The longest block of equal lines in `a[alo..ahi]` / `b[blo..bhi]`,
    /// earliest in `a` (then `b`) on ties.
    fn find_longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (mut best_i, mut best_j, mut best_size) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut new_j2len: HashMap<usize, usize> = HashMap::new();
            for &j in self.b2j.get(self.a[i].as_slice()).into_iter().flatten() {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j
                    .checked_sub(1)
                    .and_then(|previous| j2len.get(&previous))
                    .copied()
                    .unwrap_or(0)
                    + 1;
                new_j2len.insert(j, k);
                if k > best_size {
                    (best_i, best_j, best_size) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = new_j2len;
        }
        // Extend over equal lines `b2j` left out (popular ones).
        while best_i > alo && best_j > blo && self.a[best_i - 1] == self.b[best_j - 1] {
            best_i -= 1;
            best_j -= 1;
            best_size += 1;
        }
        while best_i + best_size < ahi
            && best_j + best_size < bhi
            && self.a[best_i + best_size] == self.b[best_j + best_size]
        {
            best_size += 1;
        }
        (best_i, best_j, best_size)
    }

    fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort_unstable();

        // Collapse adjacent blocks.
        let mut collapsed = Vec::new();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    collapsed.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            collapsed.push((i1, j1, k1));
        }
        collapsed.push((la, lb, 0));
        collapsed
    }

    fn opcodes(&self) -> Vec<Opcode> {
        let (mut i, mut j) = (0, 0);
        let mut opcodes = Vec::new();
        for (ai, bj, size) in self.matching_blocks() {
            let tag = match (i < ai, j < bj) {
                (true, true) => Some(Tag::Replace),
                (true, false) => Some(Tag::Delete),
                (false, true) => Some(Tag::Insert),
                (false, false) => None,
            };
            if let Some(tag) = tag {
                opcodes.push((tag, i, ai, j, bj));
            }
            (i, j) = (ai + size, bj + size);
            if size > 0 {
                opcodes.push((Tag::Equal, ai, i, bj, j));
            }
        }
        opcodes
    }

    /// Hunks: the opcodes split wherever more than `2 * context` lines are
    /// unchanged, trimmed to `context` lines of context.
    fn grouped_opcodes(&self, context: usize) -> Vec<Vec<Opcode>> {
        let mut codes = self.opcodes();
        if codes.is_empty() {
            codes.push((Tag::Equal, 0, 1, 0, 1));
        }
        if let Some(first) = codes.first_mut() {
            if first.0 == Tag::Equal {
                let (tag, i1, i2, j1, j2) = *first;
                *first = (
                    tag,
                    i1.max(i2.saturating_sub(context)),
                    i2,
                    j1.max(j2.saturating_sub(context)),
                    j2,
                );
            }
        }
        if let Some(last) = codes.last_mut() {
            if last.0 == Tag::Equal {
                let (tag, i1, i2, j1, j2) = *last;
                *last = (tag, i1, i2.min(i1 + context), j1, j2.min(j1 + context));
            }
        }

        let mut groups = Vec::new();
        let mut group = Vec::new();
        for (tag, mut i1, i2, mut j1, j2) in codes {
            if tag == Tag::Equal && i2 - i1 > 2 * context {
                group.push((tag, i1, i2.min(i1 + context), j1, j2.min(j1 + context)));
                groups.push(std::mem::take(&mut group));
                i1 = i1.max(i2.saturating_sub(context));
                j1 = j1.max(j2.saturating_sub(context));
            }
            group.push((tag, i1, i2, j1, j2));
        }
        if !(group.is_empty() || group.len() == 1 && group[0].0 == Tag::Equal) {
            groups.push(group);
        }
        groups
    }
}

fn format_range(start: usize, stop: usize) -> String {
    let length = stop - start;
    match length {
        1 => format!("{}", start + 1),
        0 => format!("{start},0"),
        _ => format!("{},{length}", start + 1),
    }
}

/// `difflib.unified_diff(a, b, filename, filename, n=3)` over lines that keep
/// their own terminators, as bytes. Empty when `a` and `b` are equal.
pub fn unified_diff(a: &[Vec<u8>], b: &[Vec<u8>], filename: &str) -> Vec<u8> {
    let matcher = SequenceMatcher::new(a, b);
    let mut out = Vec::new();
    for (index, group) in matcher.grouped_opcodes(3).iter().enumerate() {
        if index == 0 {
            out.extend_from_slice(format!("--- {filename}\n+++ {filename}\n").as_bytes());
        }
        let (first, last) = (group[0], group[group.len() - 1]);
        out.extend_from_slice(
            format!(
                "@@ -{} +{} @@\n",
                format_range(first.1, last.2),
                format_range(first.3, last.4)
            )
            .as_bytes(),
        );
        for &(tag, i1, i2, j1, j2) in group {
            let mut emit = |prefix: u8, lines: &[Vec<u8>]| {
                for line in lines {
                    out.push(prefix);
                    out.extend_from_slice(line);
                }
            };
            if tag == Tag::Equal {
                emit(b' ', &a[i1..i2]);
                continue;
            }
            if matches!(tag, Tag::Replace | Tag::Delete) {
                emit(b'-', &a[i1..i2]);
            }
            if matches!(tag, Tag::Replace | Tag::Insert) {
                emit(b'+', &b[j1..j2]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Vec<u8>> {
        text.split_inclusive('\n')
            .map(|line| line.as_bytes().to_vec())
            .collect()
    }

    fn diff(a: &str, b: &str) -> String {
        String::from_utf8(unified_diff(&lines(a), &lines(b), "f.py")).unwrap()
    }

    #[test]
    fn matches_difflib_for_a_simple_removal() {
        // python -c "import difflib,sys; sys.stdout.writelines(difflib.unified_diff(
        //     ['a\n','b\n','c\n'], ['a\n','c\n'], 'f.py', 'f.py'))"
        assert_eq!(
            diff("a\nb\nc\n", "a\nc\n"),
            "--- f.py\n+++ f.py\n@@ -1,3 +1,2 @@\n a\n-b\n c\n"
        );
    }

    #[test]
    fn splits_hunks_separated_by_more_than_six_unchanged_lines() {
        let a = "x\n1\n2\n3\n4\n5\n6\n7\ny\n";
        let b = "1\n2\n3\n4\n5\n6\n7\n";
        assert_eq!(
            diff(a, b),
            "--- f.py\n+++ f.py\n@@ -1,4 +1,3 @@\n-x\n 1\n 2\n 3\n@@ -6,4 +5,3 @@\n 5\n 6\n 7\n-y\n"
        );
    }

    #[test]
    fn deleting_everything_uses_a_zero_length_range() {
        assert_eq!(diff("a\n", ""), "--- f.py\n+++ f.py\n@@ -1 +0,0 @@\n-a\n");
    }

    #[test]
    fn equal_inputs_have_no_diff() {
        assert_eq!(diff("a\n", "a\n"), "");
    }
}
