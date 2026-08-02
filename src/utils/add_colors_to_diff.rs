//! Port of `deadcode/utils/add_colors_to_diff.py`.

pub fn add_colors_to_diff(diff: &[u8]) -> Vec<u8> {
    let lines: Vec<&[u8]> = diff.split(|&b| b == b'\n').collect();
    let mut colorful_lines: Vec<Vec<u8>> = Vec::with_capacity(lines.len());

    for line in lines {
        let mut colorful = line.to_vec();
        if line.starts_with(b"-") {
            colorful = wrap(b"\x1b[31m", &colorful, b"\x1b[0m");
        }
        if line.starts_with(b"+") {
            colorful = wrap(b"\x1b[32m", &colorful, b"\x1b[0m");
        }
        colorful_lines.push(colorful);
    }

    colorful_lines.join(&b'\n')
}

fn wrap(prefix: &[u8], content: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + content.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(content);
    out.extend_from_slice(suffix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_added_to_diff_lines() {
        let diff = b"--- foo.py\n+++ foo.py\n@@ -1,3 +1,2 @@\n-removed\n+added\n    pass";
        let result = add_colors_to_diff(diff);
        let expected = b"\x1b[31m--- foo.py\x1b[0m\n\x1b[32m+++ foo.py\x1b[0m\n@@ -1,3 +1,2 @@\n\x1b[31m-removed\x1b[0m\n\x1b[32m+added\x1b[0m\n    pass";
        assert_eq!(result, expected.to_vec());
    }
}
