//! How a manifest value is spelled in the text it was captured from.
//!
//! The manifest stores a value in its source spelling, the exact bytes the recognizer captured.
//! For a value captured inside a JSON string literal of a JSON document (a tool result, a JSON
//! text block), those bytes are JSON-escaped: `Robert \"Bob\" Smith`, `M\u00fcller`. Restore
//! into plain text writes the source bytes back. Restore into a decoded JSON value needs the
//! value a JSON parser reads, so the session records which spelling each token was minted with.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// The spelling a manifest value had where its token was first minted.
///
/// Recorded once, when the token is minted, and never changed: a later capture of the same bytes
/// in a different syntax reuses the token and keeps the first reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SourceSpelling {
    /// The captured bytes are the value.
    #[default]
    Plain,
    /// The value was captured inside a JSON string literal, and its escapes change it: the
    /// manifest holds the escaped spelling, and JSON-decoding it yields the value.
    JsonEscaped,
}

impl SourceSpelling {
    pub(crate) fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }
}

/// Decodes `raw` as the contents of a JSON string literal.
///
/// `None` when `raw` is not valid string content (a lone `"` or `\`, a raw control character, a
/// truncated escape).
pub(crate) fn decode_json_string_content(raw: &str) -> Option<String> {
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}

/// Whether a manifest value can carry [`SourceSpelling::JsonEscaped`]: it decodes, and decoding
/// changes it. Values without an escape read the same either way and stay plain.
pub(crate) fn is_json_escaped_spelling(raw: &str) -> bool {
    raw.contains('\\') && decode_json_string_content(raw).is_some_and(|decoded| decoded != raw)
}

/// The string-literal contents of a text that is a whole JSON document.
///
/// Empty for any other text, including prose that merely contains JSON: only a text that parses
/// as one JSON object or array gives its string literals an unambiguous JSON reading.
#[derive(Debug, Default)]
pub(crate) struct JsonStringLiterals {
    /// Byte ranges between the quotes of every string literal, keys included, in order.
    contents: Vec<Range<usize>>,
}

impl JsonStringLiterals {
    pub(crate) fn scan(text: &str) -> Self {
        if !matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'['))
            || serde_json::from_str::<serde::de::IgnoredAny>(text).is_err()
        {
            return Self::default();
        }
        // The text is valid JSON, so outside a literal a `"` always opens one, and inside a
        // literal a `\` always starts a two-byte-or-longer escape whose second byte is ASCII.
        let bytes = text.as_bytes();
        let mut contents = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'"' {
                let start = index + 1;
                index = start;
                while bytes[index] != b'"' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
                contents.push(start..index);
            }
            index += 1;
        }
        Self { contents }
    }

    /// The spelling of `raw`, captured at `span` of the scanned text.
    pub(crate) fn spelling(&self, span: &Range<usize>, raw: &str) -> SourceSpelling {
        let after = self
            .contents
            .partition_point(|literal| literal.end < span.end);
        let inside = self
            .contents
            .get(after)
            .is_some_and(|literal| literal.start <= span.start && span.end <= literal.end);
        if inside && is_json_escaped_spelling(raw) {
            SourceSpelling::JsonEscaped
        } else {
            SourceSpelling::Plain
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spelling_of(text: &str, raw: &str) -> SourceSpelling {
        let start = text.find(raw).expect("raw in text");
        JsonStringLiterals::scan(text).spelling(&(start..start + raw.len()), raw)
    }

    #[test]
    fn escaped_values_inside_a_json_document_literal_are_json_spelled() {
        let document = r#"{"a": "Robert \"Bob\" Smith", "b": ["M\u00fcller"]}"#;
        assert_eq!(
            spelling_of(document, r#"Robert \"Bob\" Smith"#),
            SourceSpelling::JsonEscaped
        );
        assert_eq!(
            spelling_of(document, r"M\u00fcller"),
            SourceSpelling::JsonEscaped
        );
        let keyed = r#"{"Robert \"Bob\" Smith": 1}"#;
        assert_eq!(
            spelling_of(keyed, r#"Robert \"Bob\" Smith"#),
            SourceSpelling::JsonEscaped
        );
    }

    #[test]
    fn values_that_decode_unchanged_or_sit_outside_a_literal_stay_plain() {
        // No escape: both readings agree.
        assert_eq!(
            spelling_of(r#"{"a": "Robert Smith"}"#, "Robert Smith"),
            SourceSpelling::Plain
        );
        // Prose is not a JSON document, whatever it contains.
        assert_eq!(
            spelling_of(
                r#"note {"a": "Robert \"Bob\" Smith"}"#,
                r#"Robert \"Bob\" Smith"#
            ),
            SourceSpelling::Plain
        );
        // A broken document has no reading.
        assert_eq!(
            spelling_of(r#"{"a": "Robert \"Bob\" Smith""#, r#"Robert \"Bob\" Smith"#),
            SourceSpelling::Plain
        );
        // A span across two literals is in neither.
        let document = r#"{"a": "x\"", "b": "\"y"}"#;
        assert_eq!(
            spelling_of(document, r#"\"", "b": "\""#),
            SourceSpelling::Plain
        );
        // A span ending in half an escape does not decode.
        assert_eq!(
            spelling_of(r#"{"a": "path\\more"}"#, r"path\"),
            SourceSpelling::Plain
        );
    }
}
