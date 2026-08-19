//! YAML frontmatter stripping.
//!
//! A leading `---` delimited block is metadata, not content; rendering it
//! verbatim (as glow does not, and as an unguarded parser would) turns the
//! first key into a setext heading.

/// Split leading frontmatter from the document body.
///
/// Returns `(frontmatter, body)`. When there is no frontmatter the first
/// element is `None` and the body is the whole input. The opening `---` must
/// be the very first line; the block ends at the next line that is exactly
/// `---` or `...`.
#[must_use]
pub fn split(source: &str) -> (Option<&str>, &str) {
    // Tolerate a UTF-8 BOM, which some editors write.
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);

    let after_open = match strip_delimiter_line(text) {
        Some(rest) => rest,
        None => return (None, text),
    };

    let mut offset = 0;
    let inner = after_open;
    for line in inner.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" || trimmed == "..." {
            let front = &inner[..offset];
            let body = &inner[offset + line.len()..];
            return (Some(front), body);
        }
        offset += line.len();
    }
    // Unterminated block: treat the whole document as content rather than
    // silently swallowing it.
    (None, text)
}

/// Strip a leading `---` line, returning what follows.
fn strip_delimiter_line(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---")?;
    if let Some(r) = rest.strip_prefix("\r\n") {
        return Some(r);
    }
    if let Some(r) = rest.strip_prefix('\n') {
        return Some(r);
    }
    // A bare `---` with no newline is a thematic break, not frontmatter.
    rest.is_empty().then_some("")
}

/// Convenience: the body with any frontmatter removed.
#[must_use]
pub fn strip(source: &str) -> &str {
    split(source).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_a_simple_block() {
        let (front, body) = split("---\ntitle: X\n---\n# Heading\n");
        assert_eq!(front, Some("title: X\n"));
        assert_eq!(body, "# Heading\n");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let (front, body) = split("---\r\ntitle: X\r\n---\r\n# H\r\n");
        assert_eq!(front, Some("title: X\r\n"));
        assert_eq!(body, "# H\r\n");
    }

    #[test]
    fn accepts_the_yaml_dot_terminator() {
        let (_, body) = split("---\na: 1\n...\nbody\n");
        assert_eq!(body, "body\n");
    }

    #[test]
    fn documents_without_frontmatter_are_untouched() {
        let src = "# Heading\n\nText.\n";
        assert_eq!(split(src), (None, src));
    }

    #[test]
    fn a_leading_thematic_break_is_not_frontmatter() {
        // `---` followed by content that never closes is a rule, not metadata.
        let src = "---\n\nJust a rule above.\n";
        assert_eq!(split(src), (None, src));
    }

    #[test]
    fn unterminated_frontmatter_keeps_the_document_intact() {
        let src = "---\ntitle: X\nnever closed\n";
        assert_eq!(split(src), (None, src));
    }

    #[test]
    fn a_rule_later_in_the_document_is_not_a_terminator() {
        let src = "# H\n\n---\n\ntext\n";
        assert_eq!(strip(src), src);
    }

    #[test]
    fn empty_frontmatter_block() {
        let (front, body) = split("---\n---\nbody\n");
        assert_eq!(front, Some(""));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn a_byte_order_mark_is_tolerated() {
        let (front, body) = split("\u{feff}---\na: 1\n---\nbody\n");
        assert_eq!(front, Some("a: 1\n"));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn empty_input() {
        assert_eq!(split(""), (None, ""));
    }
}
