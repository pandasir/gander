//! Source-preserving command wrapping in terminal display cells.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn escaped(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            output.extend(ch.escape_default());
        } else {
            output.push(ch);
        }
    }
    output
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut cells = 0;
    for grapheme in text.graphemes(true) {
        let next = grapheme.width();
        if cells + next > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            cells = 0;
        }
        line.push_str(grapheme);
        cells += next;
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_is_preserved_including_spaces_and_graphemes() {
        let text = escaped("/long/path  a   b \u{4e2d}\u{6587} e\u{301}\t\n\x1b");
        for width in [2, 10, 56, 80, 120] {
            let lines = wrap(&text, width);
            assert_eq!(lines.concat(), text);
            assert!(lines.iter().all(|line| line.width() <= width));
            assert!(lines.iter().all(|line| !line.starts_with('\u{301}')));
        }
        assert!(text.contains("\\u{1b}") || text.contains("\\x1b"));
    }
}
