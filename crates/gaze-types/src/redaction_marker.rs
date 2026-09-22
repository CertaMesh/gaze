//! The one-way `[REDACTED:<class>]` marker the safety net writes in place of flagged bytes.
//!
//! Before this existed the safety-net redact path replaced a flagged span with the empty string.
//! That is a silent one-way loss: the reader of the clean document cannot tell a redaction from a
//! typo, and neither can the model consuming it. The marker keeps the same decision — those bytes
//! do not leave the boundary — while making the decision visible.
//!
//! It is deliberately **not** a token. It carries no session prefix and no ordinal, restore never
//! substitutes it, and nothing downstream may treat it as owned output that can be turned back
//! into the original bytes. What makes that safe is the [`is_redaction_marker`] predicate and the
//! grammar below: every consumer that must recognise protected output — the daemon, the document
//! bundle, the proxy residual check, the token bridge — asks this one function rather than
//! carrying its own copy of the shape.

use std::ops::Range;

use crate::PiiClass;

/// Opening delimiter of a redaction marker.
pub const REDACTION_MARKER_PREFIX: &str = "[REDACTED:";

/// Closing delimiter of a redaction marker.
pub const REDACTION_MARKER_SUFFIX: &str = "]";

/// Renders the one-way marker that stands for `class`.
///
/// The class path is the canonical audit label ([`PiiClass::to_canonical_str`]) with every `_`
/// mapped to `-`. That substitution is load-bearing rather than cosmetic: every bare arm of the
/// token-shape grammar requires a trailing `_<digits>` inside word boundaries, so a custom class
/// legitimately named `address_2` would make `[REDACTED:custom:address_2]` contain the token shape
/// `custom:address_2`. Stripping the underscore makes those arms unmatchable by construction
/// instead of merely untested. The exact class is still carried by the audit row.
pub fn redaction_marker(class: &PiiClass) -> String {
    format!(
        "{REDACTION_MARKER_PREFIX}{}{REDACTION_MARKER_SUFFIX}",
        class.to_canonical_str().replace('_', "-")
    )
}

/// Whether `text` is exactly one redaction marker and nothing else.
///
/// The single shared predicate. A consumer deciding whether a span of output is already protected
/// asks this; it does not re-spell the shape locally, because a second spelling is a second thing
/// to keep in step with the emitter.
pub fn is_redaction_marker(text: &str) -> bool {
    let Some(body) = text
        .strip_prefix(REDACTION_MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(REDACTION_MARKER_SUFFIX))
    else {
        return false;
    };
    !body.is_empty()
        && body
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b':' | b'-'))
}

/// Byte spans of every redaction marker in `text`, ascending and disjoint.
///
/// Used by the consumers that have to reason about a whole document rather than one span: the
/// suspect guard (a net finding that overlaps a marker is already protected), the strict restore
/// scan, and the benchmark scorer, which must not count marker bytes as raw, leaked or
/// false-positive — they are gaze's own output, not the document's.
pub fn redaction_marker_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = text[cursor..].find(REDACTION_MARKER_PREFIX) {
        let start = cursor + offset;
        let body_start = start + REDACTION_MARKER_PREFIX.len();
        match text[body_start..].find(REDACTION_MARKER_SUFFIX) {
            Some(length) => {
                let end = body_start + length + REDACTION_MARKER_SUFFIX.len();
                if is_redaction_marker(&text[start..end]) {
                    spans.push(start..end);
                    cursor = end;
                } else {
                    cursor = body_start;
                }
            }
            None => break,
        }
    }
    spans
}

/// Total marker bytes in `text`. The scorer subtracts this from every byte count it reports.
pub fn redaction_marker_byte_len(text: &str) -> usize {
    redaction_marker_spans(text)
        .iter()
        .map(|span| span.end - span.start)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adversarial_classes() -> Vec<PiiClass> {
        let mut classes = PiiClass::builtin_variants().to_vec();
        // Custom classes whose own names end in `_<digits>`: the exact shape that would make a
        // naive marker parse as a bare token.
        for name in ["address_2", "phone", "class_alpha_1", "iban_99"] {
            classes.push(PiiClass::custom(name).expect("valid custom class"));
        }
        classes
    }

    #[test]
    fn marker_renders_the_canonical_class_path_without_underscores() {
        assert_eq!(redaction_marker(&PiiClass::Name), "[REDACTED:name]");
        assert_eq!(
            redaction_marker(&PiiClass::custom("phone").expect("valid")),
            "[REDACTED:custom:phone]"
        );
        assert_eq!(
            redaction_marker(&PiiClass::custom("address_2").expect("valid")),
            "[REDACTED:custom:address-2]"
        );
    }

    #[test]
    fn every_rendered_marker_is_recognised_by_the_shared_predicate() {
        for class in adversarial_classes() {
            let marker = redaction_marker(&class);
            assert!(
                is_redaction_marker(&marker),
                "emitter and predicate disagree on {marker}"
            );
        }
    }

    #[test]
    fn the_predicate_rejects_shapes_that_are_not_whole_markers() {
        for text in [
            "",
            "[REDACTED:]",
            "[REDACTED:name",
            "REDACTED:name]",
            "[REDACTED:Name]",
            "[REDACTED:na me]",
            "x[REDACTED:name]",
            "[REDACTED:name]x",
            "[REDACTED:name][REDACTED:name]",
            "[REDACTED:address_2]",
        ] {
            assert!(!is_redaction_marker(text), "should not be a marker: {text}");
        }
    }

    #[test]
    fn spans_find_every_marker_and_skip_surrounding_prose() {
        let text = "Dear [REDACTED:name], your [REDACTED:custom:order-id] shipped.";
        let spans = redaction_marker_spans(text);
        assert_eq!(spans.len(), 2);
        assert_eq!(&text[spans[0].clone()], "[REDACTED:name]");
        assert_eq!(&text[spans[1].clone()], "[REDACTED:custom:order-id]");
        assert_eq!(
            redaction_marker_byte_len(text),
            "[REDACTED:name]".len() + "[REDACTED:custom:order-id]".len()
        );
    }

    #[test]
    fn spans_ignore_bracketed_prose_that_only_looks_like_a_marker() {
        let text = "[REDACTED:Name] and [REDACTED: name] are prose, [REDACTED:name] is not.";
        let spans = redaction_marker_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].clone()], "[REDACTED:name]");
    }

    #[test]
    fn an_unterminated_prefix_does_not_loop_or_panic() {
        assert!(redaction_marker_spans("[REDACTED:name").is_empty());
        assert_eq!(redaction_marker_spans("[REDACTED:[REDACTED:name]").len(), 1);
    }
}
