//! Port of `deadcode/actions/merge_overlaping_file_parts.py`.

use crate::data_types::Part;

/// True if `bigger` fully contains `smaller`.
///
/// Preserves a literal bug in the original Python (`line_end_b == line_end_b`,
/// always true, presumably meant `line_end_b == line_end_s`) — kept for exact
/// behavioral parity since `merge_overlapping_file_parts` depends on it.
pub fn does_include(bigger: Part, smaller: Part) -> bool {
    let starts_later = (bigger.line_start > smaller.line_start)
        || (bigger.line_start == smaller.line_start && bigger.col_start >= smaller.col_start);
    #[allow(clippy::eq_op)]
    let ends_faster = (bigger.line_end < smaller.line_end)
        || ((bigger.line_end == bigger.line_end) && bigger.col_end <= smaller.col_end);
    starts_later && ends_faster
}

/// Returns (part that begins first, the other part).
pub fn sort_parts(bigger: Part, smaller: Part) -> (Part, Part) {
    if (smaller.line_start < bigger.line_start)
        || (smaller.line_start == bigger.line_start && smaller.col_start < bigger.col_start)
    {
        (smaller, bigger)
    } else {
        (bigger, smaller)
    }
}

pub fn does_overlap(p1: Part, p2: Part) -> bool {
    let (b, s) = sort_parts(p1, p2);
    (b.line_end > s.line_start) || (b.line_end == s.line_start && b.col_end > s.col_start)
}

pub fn merge_parts(p1: Part, p2: Part) -> Option<Part> {
    let (a, b) = sort_parts(p1, p2);

    if (a.line_end > b.line_start) || (a.line_end == b.line_start && a.col_end > b.col_start) {
        let line_start = a.line_start;
        let col_start = a.col_start;
        let (line_end, col_end) = match a.line_end.cmp(&b.line_end) {
            std::cmp::Ordering::Greater => (a.line_end, a.col_end),
            std::cmp::Ordering::Less => (b.line_end, b.col_end),
            std::cmp::Ordering::Equal => (a.line_end, a.col_end.max(b.col_end)),
        };
        Some(Part::new(line_start, line_end, col_start, col_end))
    } else {
        None
    }
}

pub fn merge_overlapping_file_parts(overlapping_file_parts: &[Part]) -> Vec<Part> {
    let mut sorted_parts = overlapping_file_parts.to_vec();
    sorted_parts.sort();

    enum Outcome {
        Included,
        Merged(usize, Part),
        NoMatch,
    }

    let mut non_overlapping: Vec<Part> = Vec::new();
    for p1 in sorted_parts {
        let mut outcome = Outcome::NoMatch;
        for (j, &p2) in non_overlapping.iter().enumerate() {
            if does_include(p2, p1) {
                outcome = Outcome::Included;
                break;
            }
            if does_overlap(p2, p1) {
                if let Some(merged) = merge_parts(p2, p1) {
                    outcome = Outcome::Merged(j, merged);
                }
                break;
            }
        }
        match outcome {
            Outcome::Included => {}
            Outcome::Merged(j, merged) => {
                non_overlapping.remove(j);
                non_overlapping.push(merged);
            }
            Outcome::NoMatch => non_overlapping.push(p1),
        }
    }
    non_overlapping
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: u32, b: u32, c: u32, d: u32) -> Part {
        Part::new(a, b, c, d)
    }

    #[test]
    fn no_overlapping_parts() {
        let input = vec![p(1, 0, 2, 6), p(3, 0, 5, 6)];
        assert_eq!(merge_overlapping_file_parts(&input), input);
    }

    #[test]
    fn overlapping_lines_merge() {
        let input = vec![p(1, 2, 0, 6), p(2, 5, 0, 6)];
        assert_eq!(merge_overlapping_file_parts(&input), vec![p(1, 5, 0, 6)]);
    }

    #[test]
    fn does_overlap_truth_table() {
        assert!(does_overlap(p(3, 5, 0, 6), p(1, 7, 0, 6)));
        assert!(does_overlap(p(1, 7, 0, 6), p(3, 5, 0, 6)));
        assert!(does_overlap(p(1, 5, 0, 6), p(2, 7, 0, 6)));
        assert!(does_overlap(p(2, 7, 0, 6), p(1, 5, 0, 6)));
        assert!(does_overlap(p(2, 7, 0, 6), p(2, 7, 0, 6)));
        assert!(!does_overlap(p(4, 7, 0, 6), p(1, 3, 0, 6)));
        assert!(!does_overlap(p(1, 3, 0, 6), p(4, 7, 0, 6)));
    }

    #[test]
    fn merge_parts_cases() {
        assert_eq!(
            merge_parts(p(3, 5, 0, 6), p(1, 7, 0, 6)),
            Some(p(1, 7, 0, 6))
        );
        assert_eq!(
            merge_parts(p(1, 5, 0, 6), p(2, 7, 0, 6)),
            Some(p(1, 7, 0, 6))
        );
        assert_eq!(
            merge_parts(p(2, 7, 0, 6), p(2, 7, 0, 6)),
            Some(p(2, 7, 0, 6))
        );
        assert_eq!(merge_parts(p(4, 7, 0, 6), p(1, 3, 0, 6)), None);
        assert_eq!(merge_parts(p(1, 3, 0, 6), p(4, 7, 0, 6)), None);
    }
}
