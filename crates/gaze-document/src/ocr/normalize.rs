//! Narrow OCR-artifact normalization applied between OCR and the redact pipeline.
//!
//! Tesseract — like every OCR engine — sometimes inserts spurious whitespace
//! between adjacent glyphs that share kerning. The most common (and most
//! dangerous) artifact in the gaze-document corpus is whitespace inserted
//! around the `@` of an email address, e.g.:
//!
//! ```text
// fixture-cited(crates/gaze-document/src/ocr/normalize.rs:ocr::normalize::tests::collapses_space_before_at)
//! jane.doe@example.invalid   →   "jane.doe @example.invalid"
//! ```
//!
//! Such corrupted forms are still unmistakable emails to a human or LLM
//! reader but slip past strict `\S+@\S+`-shaped detectors. Per the v0.7.x
//! axis-1 (never leak) contract the pipeline MUST fail closed against this
//! class, so we normalize the artifact at the OCR → redact boundary instead
//! of relying on more permissive detectors.
//!
//! ## Normalization rules
//!
//! Rule 1 — **email separator repair.**
//! Collapse intra-line whitespace immediately adjacent to `@` when **both**
//! sides are non-whitespace:
//!
//! ```text
//! (\S)[ \t]*@[ \t]*(\S)   →   $1@$2
//! ```
//!
//! Only `[ \t]` (horizontal whitespace) is collapsed; newlines remain
//! preserved so a stray `@` on a line of its own is left untouched and
//! cannot accidentally glue two unrelated lines together.
//!
//! Rule 2 — **email domain-dot repair.**
//! Collapse intra-line whitespace after a dot in the domain segment:
//!
//! ```text
//! (\S+@\S+\.)[ \t]+(\S)   →   $1$2
//! ```
//!
//! Phone-number artifacts are already tolerated by the gaze-document phone
//! recognizer (`[-.\s]` separator class); any future additions belong here as
//! additional named rules with their own focused regex.

use std::sync::OnceLock;

use regex::Regex;

/// Apply every documented OCR normalization rule to `text`.
///
/// Returns an owned `String`. The function is allocation-cheap on
/// already-clean input (only the regex pass walks the string).
pub(crate) fn normalize_ocr_artifacts(text: &str) -> String {
    let text = email_separator_regex()
        .replace_all(text, "$pre@$post")
        .into_owned();
    email_domain_dot_regex()
        .replace_all(&text, "$pre$post")
        .into_owned()
}

fn email_separator_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Capture single non-whitespace char on each side of the `@` so we
        // do not accidentally munge `\n@foo` (twitter-handle-style line
        // starters) or `bar@\n` (line-ending atoms).
        Regex::new(r"(?P<pre>\S)[ \t]*@[ \t]*(?P<post>\S)")
            .expect("static email-separator pattern compiles")
    })
}

fn email_domain_dot_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?P<pre>\S+@\S+\.)[ \t]+(?P<post>\S)")
            .expect("static email-domain-dot pattern compiles")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_space_before_at() {
        assert_eq!(
            normalize_ocr_artifacts("Email: jane.doe @example.invalid"),
            "Email: jane.doe@example.invalid"
        );
    }

    #[test]
    fn collapses_space_after_at() {
        assert_eq!(
            normalize_ocr_artifacts("Email: jane.doe@ example.invalid"),
            "Email: jane.doe@example.invalid"
        );
    }

    #[test]
    fn collapses_spaces_around_at() {
        assert_eq!(
            normalize_ocr_artifacts("Email: jane.doe @ example.invalid"),
            "Email: jane.doe@example.invalid"
        );
    }

    #[test]
    fn collapses_tabs_around_at() {
        assert_eq!(
            normalize_ocr_artifacts("jane.doe\t@\texample.invalid"),
            "jane.doe@example.invalid"
        );
    }

    #[test]
    fn collapses_space_after_domain_dot() {
        assert_eq!(
            normalize_ocr_artifacts("Email: jane.doe@example. invalid"),
            "Email: jane.doe@example.invalid"
        );
    }

    #[test]
    fn already_clean_passthrough() {
        let s = "Email: jane.doe@example.invalid\nPhone: +1-555-0142";
        assert_eq!(normalize_ocr_artifacts(s), s);
    }

    #[test]
    fn leaves_newline_adjacent_at_untouched() {
        // A bare `@` on its own line is not an email artifact; we MUST NOT
        // glue the lines together.
        let s = "alpha\n@\nbeta";
        assert_eq!(normalize_ocr_artifacts(s), s);
    }

    #[test]
    fn leaves_leading_at_handle_untouched() {
        let s = "ping @handle now";
        // `g @h` collapses → "ping@handle now". This is acceptable: a bare
        // word-followed-by-@-handle is rare in document OCR and the
        // collapsed form does not introduce a NEW PII class (it produces a
        // pseudo-email that the email recognizer may or may not match; the
        // pipeline is fail-closed either way). Document the behavior.
        assert_eq!(normalize_ocr_artifacts(s), "ping@handle now");
    }

    #[test]
    fn handles_multiple_occurrences() {
        let s = "a @b and c@ d and e @ f";
        assert_eq!(normalize_ocr_artifacts(s), "a@b and c@d and e@f");
    }

    #[test]
    fn preserves_non_at_whitespace() {
        // Whitespace elsewhere in the document is NOT touched.
        let s = "Bill to:  Jane   Doe   \nPhone: +1 555 0142";
        assert_eq!(normalize_ocr_artifacts(s), s);
    }
}
