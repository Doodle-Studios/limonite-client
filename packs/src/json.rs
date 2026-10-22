//! JSON as Bedrock writes it: standard JSON, plus `//` and `/* */` comments
//! and an optional UTF-8 byte order mark.
//!
//! `serde_json` follows the JSON standard, which has neither, so they're
//! removed first. `biomes_client.json` in the vanilla pack starts with a
//! `/* ... */` comment, for example.

use serde::de::DeserializeOwned;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Parses Bedrock JSON into a `T`.
///
/// # Errors
/// If the text without its comments still isn't valid JSON for a `T`.
pub fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(&strip_comments(bytes))
}

/// Removes comments and a leading byte order mark, leaving strings alone.
///
/// A comment becomes a single space, so it still separates the tokens around
/// it, except that a block comment keeps its line breaks: an error later in
/// the file then reports the line number the author sees.
///
/// It works on bytes, not characters. That's safe for UTF-8: every byte of a
/// multi-byte character is 0x80 or above, so none can look like `/`, `*`,
/// `"`, `\` or a line break.
#[must_use]
pub fn strip_comments(bytes: &[u8]) -> Vec<u8> {
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1)) {
            (b'"', _) => {
                // Copy the whole string, so a `//` inside one is kept. A
                // backslash escapes the next byte, which may be a quote.
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                // Past the closing quote; an unterminated string runs to the
                // end, and the parser reports it.
                i = (i + 1).min(bytes.len());
                out.extend_from_slice(&bytes[start..i]);
            }
            (b'/', Some(b'/')) => {
                // To the end of the line; the line break itself is kept.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                out.push(b' ');
            }
            (b'/', Some(b'*')) => {
                i += 2;
                out.push(b' ');
                // An unterminated comment runs to the end of the file.
                while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                    if bytes[i] == b'\n' {
                        out.push(b'\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn strips_line_and_block_comments() {
        let text = br#"/* licence
            header */
            {
                "a": 1, // the first
                "b": /* inline */ [2, 3]
            }"#;
        let value: Value = parse(text).unwrap();
        assert_eq!(value, json!({ "a": 1, "b": [2, 3] }));
    }

    #[test]
    fn comment_markers_inside_strings_survive() {
        let text = br#"{
            "url": "https://example.com/a//b",
            "glob": "textures/*/blocks /* not a comment */",
            "quote": "say \"// hi\" \\",
            "after": 1 // a real comment
        }"#;
        let value: Value = parse(text).unwrap();
        assert_eq!(value["url"], "https://example.com/a//b");
        assert_eq!(value["glob"], "textures/*/blocks /* not a comment */");
        assert_eq!(value["quote"], "say \"// hi\" \\");
        assert_eq!(value["after"], 1);
    }

    #[test]
    fn block_comments_keep_line_numbers() {
        let text = b"/*\n\n*/ {\n \"a\": oops }";
        let err = parse::<Value>(text).unwrap_err();
        assert_eq!(err.line(), 4);
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        let value: Value = parse(b"\xEF\xBB\xBF{\"a\": \"\xC3\xA9\"}").unwrap();
        assert_eq!(value, json!({ "a": "é" }));
    }

    #[test]
    fn unterminated_input_is_an_error_not_a_panic() {
        for text in [&b"{\"a\": \"open"[..], b"{ /* open", b"{\"a\\", b"/", b"\"\\"] {
            assert!(parse::<Value>(text).is_err(), "{:?}", String::from_utf8_lossy(text));
        }
    }
}
