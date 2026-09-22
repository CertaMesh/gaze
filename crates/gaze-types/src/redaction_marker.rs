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
/// The class path is the canonical audit label ([`PiiClass::to_canonical_str`]), lowercased, with
/// `:` kept as the namespace separator and every other non-alphanumeric byte mapped to `-`.
///
/// Mapping `_` is load-bearing rather than cosmetic: every bare arm of the token-shape grammar
/// requires a trailing `_<digits>` inside word boundaries, so a custom class legitimately named
/// `address_2` would make `[REDACTED:custom:address_2]` contain the token shape
/// `custom:address_2`. Stripping the underscore makes those arms unmatchable by construction
/// instead of merely untested.
///
/// Mapping *everything* else is what keeps the emitter and [`is_redaction_marker`] from drifting
/// apart. [`PiiClass::custom`] normalises, but [`PiiClass::Custom`] is a public variant an adopter
/// can build directly (a custom [`crate::LeakSuspect`] class) or deserialize, and
/// [`PiiClass::family`] does not normalise its name. A class carrying an uppercase letter, a
/// space or a `]` would otherwise render a marker the shared predicate rejects, and the one
/// production consumer of that predicate -- the token-bridge index, which skips markers so a
/// one-way redaction never becomes a searchable, translatable entity -- would index the redacted
/// bytes instead. Sanitising here makes `is_redaction_marker(redaction_marker(c))` true for every
/// `PiiClass` by construction. The exact class is still carried by the audit row.
pub fn redaction_marker(class: &PiiClass) -> String {
    let canonical = class.to_canonical_str();
    let mut body = String::with_capacity(canonical.len());
    for character in canonical.chars() {
        match character {
            ':' => body.push(':'),
            c if c.is_ascii_alphanumeric() => body.push(c.to_ascii_lowercase()),
            _ => body.push('-'),
        }
    }
    format!("{REDACTION_MARKER_PREFIX}{body}{REDACTION_MARKER_SUFFIX}")
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
        && body.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b':' | b'-')
        })
}

/// Byte spans of every well-formed redaction marker in `text`, ascending and disjoint.
///
/// For consumers that hold only text, with no manifest: an adopter inspecting clean output, or a
/// log reader counting redactions. It answers "what LOOKS like a marker", which is a weaker claim
/// than "what did gaze redact". Anything that decides protection must use the manifest instead --
/// the runtime's own suspect guard does -- because a document can contain the literal
/// `[REDACTED:name]` without gaze having written it, and text alone cannot tell the two apart.
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

/// Total bytes of well-formed markers in `text`, with the same text-only caveat as
/// [`redaction_marker_spans`].
///
/// Useful when comparing clean output against its input by length: a redaction makes the output
/// LONGER for the marker's width, where deleting made it shorter. The benchmark scorer does not
/// need it -- it scores in original-request coordinates, where marker bytes never appear.
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
        // Classes that never went through the normalising constructor. `PiiClass::Custom` is a
        // public variant an adopter can build directly or deserialize, and `PiiClass::family`
        // does not normalise. Each of these used to render a marker the shared predicate
        // rejected, which is how a redaction could still be indexed as a searchable entity.
        for name in [
            "Tenant Docs",
            "a]b",
            "a\nb",
            "Gr\u{fc}\u{df}e",
            "UPPER_1",
            "  ",
        ] {
            classes.push(PiiClass::Custom(name.to_string()));
            classes.push(PiiClass::family(name));
        }
        classes
    }

    /// The emitter and the predicate must never disagree, for ANY `PiiClass`.
    ///
    /// `every_rendered_marker_is_recognised_by_the_shared_predicate` states the invariant; this
    /// names the concrete shapes that broke it before the emitter sanitised the class, so a
    /// regression reads as "the marker for a space-carrying class is not recognised" rather than
    /// as an opaque loop failure.
    ///
    /// Mutation: drop the sanitiser (keep only `.replace('_', "-")`) and this goes RED, together
    /// with `a_redaction_marker_with_an_unnormalised_class_is_never_indexed` in `gaze-token-bridge`.
    #[test]
    fn a_class_that_never_went_through_the_normalising_constructor_still_renders_a_valid_marker() {
        for (class, expected) in [
            (
                PiiClass::Custom("Tenant Docs".to_string()),
                "[REDACTED:custom:tenant-docs]",
            ),
            (PiiClass::Custom("a]b".to_string()), "[REDACTED:custom:a-b]"),
            (
                PiiClass::family("Tenant Docs"),
                "[REDACTED:custom:family:tenant-docs]",
            ),
            (
                PiiClass::Custom("UPPER_1".to_string()),
                "[REDACTED:custom:upper-1]",
            ),
        ] {
            let marker = redaction_marker(&class);
            assert_eq!(marker, expected);
            assert!(is_redaction_marker(&marker), "not recognised: {marker}");
        }
    }

    /// A marker can never carry a `]` of its own, so text that does is prose, not gaze output.
    ///
    /// `redaction_marker_spans` is the text-only helper: it answers "what LOOKS like a marker",
    /// which is deliberately weaker than "what did gaze redact". It reads `[REDACTED:custom:a]b]`
    /// as the marker `[REDACTED:custom:a]` followed by the prose `b]`. That is harmless precisely
    /// because nothing decides protection from it -- the runtime's suspect guard asks the
    /// manifest -- and because the emitter can no longer produce that shape.
    #[test]
    fn a_bracket_inside_bracketed_prose_is_split_at_the_first_close() {
        let text = "[REDACTED:custom:a]b] tail";
        let spans = redaction_marker_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].clone()], "[REDACTED:custom:a]");
        assert_eq!(
            redaction_marker(&PiiClass::Custom("a]b".to_string())),
            "[REDACTED:custom:a-b]",
            "the emitter can never produce the split shape"
        );
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
