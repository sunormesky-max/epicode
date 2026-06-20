//! Shared utility helpers used across the backend.

/// Extract plain text from an HTML string.
///
/// All markup is removed or converted to a readable textual representation.
/// Script/style tag contents are dropped, HTML entities are decoded, and
/// remaining formatting is rendered as lightweight markdown-like text.
/// Falls back to the original string if parsing fails.
pub fn strip_html(s: &str) -> String {
    html2text::from_read(s.as_bytes(), 10_000).unwrap_or_else(|_| s.to_string())
}

/// Truncate a string to at most `max_bytes` UTF-8 bytes without splitting a
/// multi-byte character boundary.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_simple_tags() {
        assert_eq!(strip_html("<p>hello</p>").trim(), "hello");
    }

    #[test]
    fn removes_script_content() {
        let clean = strip_html("<script>alert('xss')</script>");
        assert!(!clean.contains("alert"));
        assert!(!clean.contains("script"));
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(strip_html("&lt;hello&gt;").trim(), "<hello>");
    }

    #[test]
    fn preserves_plain_text_symbols() {
        assert_eq!(strip_html("3 < 4 && 5 > 6").trim(), "3 < 4 && 5 > 6");
    }

    #[test]
    fn handles_nested_tags() {
        assert_eq!(
            strip_html("<div><b>bold</b> and <i>italic</i></div>").trim(),
            "**bold** and italic"
        );
    }

    #[test]
    fn drops_img_event_handlers() {
        let clean = strip_html("<img src=x onerror=alert(1)> click");
        assert!(!clean.contains("alert"));
        assert!(!clean.contains("onerror"));
    }
}
