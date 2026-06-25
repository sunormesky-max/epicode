//! Integration tests for `epicode::util`.

use epicode::util::{strip_html, truncate_str};

#[test]
fn strip_html_removes_tags() {
    assert_eq!(strip_html("<p>hello</p>").trim(), "hello");
}

#[test]
fn strip_html_drops_script_content() {
    let clean = strip_html("<script>alert('xss')</script>");
    assert!(!clean.contains("alert"));
    assert!(!clean.contains("script"));
}

#[test]
fn strip_html_decodes_entities() {
    assert_eq!(strip_html("&lt;hello&gt;").trim(), "<hello>");
}

#[test]
fn strip_html_preserves_text_symbols() {
    assert_eq!(strip_html("3 < 4 && 5 > 6").trim(), "3 < 4 && 5 > 6");
}

#[test]
fn truncate_str_keeps_short_strings_unchanged() {
    assert_eq!(truncate_str("hello", 10), "hello");
}

#[test]
fn truncate_str_cuts_at_boundary() {
    assert_eq!(truncate_str("hello world", 5), "hello");
}

#[test]
fn truncate_str_does_not_split_multibyte() {
    // "你好" is 6 bytes in UTF-8 (3 bytes per char).
    // Cutting at 4 bytes should fall back to 3 bytes (first char only).
    let s = "你好";
    let truncated = truncate_str(s, 4);
    assert_eq!(truncated, "你");
}

#[test]
fn truncate_str_handles_empty_string() {
    assert_eq!(truncate_str("", 10), "");
}
