//! Benign lookalike structures: text around a weak, cue-less shape match that
//! positively identifies a non-PII value (an order number, an amount, a SKU).
//!
//! A recognizer opts in per structure (`[recognizers.context] benign_lookalikes`).
//! Validator veto then drops that recognizer's candidate before conflict
//! resolution with a typed [`ValidatorFailReason`], exactly like a failed
//! validator. Only the opted-in recognizer's own candidate is dropped; any other
//! candidate over the same bytes (a cued phone, an IBAN, a card) still protects
//! them.
//!
//! A PII cue always wins, and uncertainty keeps the value protected: a cue word
//! on the candidate's line before or after it, or in the structured field name
//! the value came from, disables every structure. Cue words match by stem, so a
//! longer word that merely starts like a cue also disables the veto.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::ValidatorFailReason;

/// A closed set of benign structures a weak recognizer may be vetoed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum BenignLookalike {
    /// The candidate is the last hyphen segment of a token whose first segment is a
    /// document-reference label word: `SKU-DEMO-73821`, `INVOICE-TEST-03687455`.
    JoinedIdentifier,
    /// A currency code or sign sits directly before the candidate, or after it (optionally
    /// past two decimals): `EUR 22186,12`, `22186,12 EUR`, `$90210`.
    CurrencyAmount,
    /// The candidate is a strict part of one same-separator digit-group run holding at least
    /// 16 digits, longer than any E.164 number: the SKU `0593-9506-3395-7573`.
    DigitRunFragment,
}

/// Label words that open a document-reference identifier. Cue words of any PII class
/// (`tel`, `plz`, `zip`, ...) must never appear here.
const REFERENCE_LABELS: &[&str] = &[
    "art",
    "artikel",
    "auftrag",
    "batch",
    "beleg",
    "bestellung",
    "case",
    "charge",
    "gutschein",
    "inv",
    "invoice",
    "item",
    "lot",
    "ord",
    "order",
    "po",
    "quote",
    "rechnung",
    "ref",
    "rma",
    "shipment",
    "sku",
    "ticket",
    "track",
    "tracking",
    "voucher",
    "vorgang",
];
/// Stems of cue words for the classes that opt in (phone, postal code). A word starting with
/// one of these on the candidate's line, or in its field name, means the writer may have
/// labelled the value as PII, so no structure fires. Matching by stem errs toward protection.
const CUE_STEMS: &[&str] = &[
    "anruf", "call", "cell", "contact", "fax", "handy", "kontakt", "mobil", "phone", "plz", "post",
    "ruf", "tel", "zip",
];
/// How far before or after the candidate a cue word still counts, in bytes.
const CUE_WINDOW: usize = 40;
const CURRENCY_CODES: &[&str] = &["CHF", "EUR", "GBP", "USD"];
const CURRENCY_SIGNS: &[char] = &['€', '$', '£'];
/// No E.164 number has more digits than this, so a longer run cannot be one phone number.
const MAX_PHONE_DIGITS: usize = 15;

impl BenignLookalike {
    /// Parses a rulepack spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "joined_identifier" => Some(Self::JoinedIdentifier),
            "currency_amount" => Some(Self::CurrencyAmount),
            "digit_run_fragment" => Some(Self::DigitRunFragment),
            _ => None,
        }
    }

    /// The audit reason a veto by this structure records.
    pub fn reason(self) -> ValidatorFailReason {
        match self {
            Self::JoinedIdentifier => ValidatorFailReason::BenignJoinedIdentifier,
            Self::CurrencyAmount => ValidatorFailReason::BenignCurrencyAmount,
            Self::DigitRunFragment => ValidatorFailReason::BenignDigitRunFragment,
        }
    }

    /// Whether `text[span]` sits inside this benign structure. `field_name` is the structured
    /// field the text came from, if any.
    pub fn matches(self, text: &str, span: Range<usize>, field_name: Option<&str>) -> bool {
        if span.start >= span.end
            || text.get(span.clone()).is_none()
            || field_name.is_some_and(has_cue)
            || cued(text, span.clone())
        {
            return false;
        }
        match self {
            Self::JoinedIdentifier => joined_identifier(text, span),
            Self::CurrencyAmount => currency_amount(text, span),
            Self::DigitRunFragment => digit_run_fragment(text, span),
        }
    }
}

/// Whether any word of `text` starts with a cue stem. Words split at every non-letter and at
/// each lower-to-upper case change, so `postal_code`, `zipCode` and `phoneNumber` all count.
fn has_cue(text: &str) -> bool {
    let mut word = String::new();
    let mut previous_lower = false;
    let mut words = Vec::new();
    for c in text.chars() {
        if !c.is_alphabetic() || (c.is_uppercase() && previous_lower) {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
        if c.is_alphabetic() {
            word.extend(c.to_lowercase());
        }
        previous_lower = c.is_lowercase();
    }
    words.push(word);
    words
        .iter()
        .any(|word| CUE_STEMS.iter().any(|stem| word.starts_with(stem)))
}

/// Whether a cue word stands on the candidate's line within [`CUE_WINDOW`] bytes before or
/// after it, or inside the candidate's own token.
fn cued(text: &str, span: Range<usize>) -> bool {
    let mut from = span.start.saturating_sub(CUE_WINDOW);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let mut to = (span.end + CUE_WINDOW).min(text.len());
    while !text.is_char_boundary(to) {
        to -= 1;
    }
    let before = &text[from..span.start];
    let before = before.rsplit(['\n', '\r']).next().unwrap_or(before);
    let after = &text[span.end..to];
    let after = after.split(['\n', '\r']).next().unwrap_or(after);
    has_cue(before) || has_cue(after)
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

fn joined_identifier(text: &str, span: Range<usize>) -> bool {
    let bytes = text.as_bytes();
    if span.start == 0 || bytes[span.start - 1] != b'-' {
        return false;
    }
    // The candidate must end the token: no word byte, and no further `-segment`.
    match bytes.get(span.end) {
        Some(&next) if is_word(next) => return false,
        Some(b'-') if bytes.get(span.end + 1).copied().is_some_and(is_word) => return false,
        _ => {}
    }
    let mut start = span.start - 1;
    while start > 0 && (is_word(bytes[start - 1]) || bytes[start - 1] == b'-') {
        start -= 1;
    }
    let first = text[start..span.start - 1].split('-').next().unwrap_or("");
    !first.is_empty()
        && first.bytes().all(|byte| byte.is_ascii_alphabetic())
        && REFERENCE_LABELS.contains(&first.to_ascii_lowercase().as_str())
}

/// Strips one space, NBSP or narrow NBSP from the end of `text`.
fn strip_gap_end(text: &str) -> &str {
    text.strip_suffix([' ', '\u{00A0}', '\u{202F}'])
        .unwrap_or(text)
}

fn strip_gap_start(text: &str) -> &str {
    text.strip_prefix([' ', '\u{00A0}', '\u{202F}'])
        .unwrap_or(text)
}

fn currency_amount(text: &str, span: Range<usize>) -> bool {
    let before = strip_gap_end(&text[..span.start]);
    let code_before = CURRENCY_CODES.iter().any(|code| {
        before
            .strip_suffix(code)
            .is_some_and(|rest| !rest.bytes().next_back().is_some_and(is_word))
    });
    if code_before || before.ends_with(CURRENCY_SIGNS) {
        return true;
    }
    let mut after = &text[span.end..];
    let decimals = after.as_bytes();
    if decimals.len() >= 3
        && matches!(decimals[0], b'.' | b',')
        && decimals[1].is_ascii_digit()
        && decimals[2].is_ascii_digit()
        && !decimals
            .get(3)
            .copied()
            .is_some_and(|byte| byte.is_ascii_digit())
    {
        after = &after[3..];
    }
    let after = strip_gap_start(after);
    after.starts_with(CURRENCY_SIGNS)
        || CURRENCY_CODES.iter().any(|code| {
            after
                .strip_prefix(code)
                .is_some_and(|rest| !rest.bytes().next().is_some_and(is_word))
        })
}

fn digit_run_fragment(text: &str, span: Range<usize>) -> bool {
    let bytes = text.as_bytes();
    let separator = |byte: u8| matches!(byte, b'-' | b'.' | b'/');
    let (mut start, mut end) = (span.start, span.end);
    while start > 0
        && (bytes[start - 1].is_ascii_digit()
            || (start >= 2 && separator(bytes[start - 1]) && bytes[start - 2].is_ascii_digit()))
    {
        start -= 1;
    }
    while end < bytes.len()
        && (bytes[end].is_ascii_digit()
            || (separator(bytes[end]) && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
    {
        end += 1;
    }
    if (start, end) == (span.start, span.end) {
        return false;
    }
    let run = &bytes[start..end];
    let mut separators = run.iter().copied().filter(|&byte| separator(byte));
    let first = separators.next();
    first.is_some()
        && separators.all(|byte| Some(byte) == first)
        && run.iter().filter(|byte| byte.is_ascii_digit()).count() > MAX_PHONE_DIGITS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, value: &str) -> Range<usize> {
        let start = text.find(value).expect("value in text");
        start..start + value.len()
    }

    fn fires(kind: BenignLookalike, text: &str, value: &str) -> bool {
        kind.matches(text, at(text, value), None)
    }

    #[test]
    fn joined_identifier_needs_a_reference_label_first_and_the_value_last() {
        use BenignLookalike::JoinedIdentifier as J;
        assert!(fires(J, "Lagerartikel SKU-DEMO-73821 und", "73821"));
        assert!(fires(J, "invoice INVOICE-TEST-03687455, stock", "03687455"));
        assert!(fires(J, "order ORDER-9041-145684.", "145684"));
        // Security tokens and country-prefixed postcodes are not references.
        assert!(!fires(J, "token ASDFG-98765-ZXCVB here", "98765"));
        assert!(!fires(J, "token XYZ123-abcde-98765 here", "98765"));
        assert!(!fires(J, "D-80331 München", "80331"));
        // The value must end the token.
        assert!(!fires(J, "SKU-73821-A1 x", "73821"));
        assert!(!fires(J, "SKU 73821", "73821"));
    }

    #[test]
    fn a_cue_before_or_after_the_candidate_disables_every_structure() {
        use BenignLookalike::*;
        // Before, inside the token, and after the value, on the same line.
        assert!(!fires(CurrencyAmount, "Tel: 2125550187 USD", "2125550187"));
        assert!(!fires(JoinedIdentifier, "PLZ-80331 München", "80331"));
        assert!(!fires(JoinedIdentifier, "Tel-0301234567", "0301234567"));
        assert!(!fires(JoinedIdentifier, "ORDER-90210 (ZIP)", "90210"));
        assert!(!fires(
            JoinedIdentifier,
            "ORDER-212-555-0187 (phone)",
            "212-555-0187"
        ));
        assert!(!fires(CurrencyAmount, "EUR 90210 is my zip", "90210"));
        assert!(!fires(
            DigitRunFragment,
            "0593-9506-3395-7573 Telefon",
            "0593-9506-3395"
        ));
        // JSON keys: snake_case and camelCase words both count.
        assert!(!fires(
            JoinedIdentifier,
            r#"{"postal_code":"ORDER-90210"}"#,
            "90210"
        ));
        assert!(!fires(
            JoinedIdentifier,
            r#"{"zipCode": "ORDER-90210"}"#,
            "90210"
        ));
        assert!(!fires(
            JoinedIdentifier,
            r#"{"phoneNumber": "ORDER-2125550187"}"#,
            "2125550187"
        ));
        // Stems: a longer word starting like a cue also counts.
        assert!(!fires(
            JoinedIdentifier,
            "Telefonnummer ORDER-0301234567",
            "0301234567"
        ));
        assert!(!fires(
            JoinedIdentifier,
            "Postleitzahl ORDER-80331",
            "80331"
        ));
        // A cue on another line, or a word merely containing a cue, does not count.
        assert!(fires(CurrencyAmount, "Tel\nEUR 22186,12", "22186"));
        assert!(fires(CurrencyAmount, "EUR 22186,12\nTel", "22186"));
        assert!(fires(CurrencyAmount, "Hotel EUR 22186,12", "22186"));
    }

    #[test]
    fn a_cue_in_the_field_name_disables_every_structure() {
        use BenignLookalike::JoinedIdentifier as J;
        let text = "ORDER-90210";
        for field in [
            "postal_code",
            "zipCode",
            "PLZ",
            "phone",
            "customer.telefon",
            "contact",
        ] {
            assert!(!J.matches(text, at(text, "90210"), Some(field)), "{field}");
        }
        for field in ["order", "reference", "itemCode"] {
            assert!(J.matches(text, at(text, "90210"), Some(field)), "{field}");
        }
    }

    #[test]
    fn currency_amount_needs_a_code_or_sign_right_next_to_the_value() {
        use BenignLookalike::CurrencyAmount as C;
        assert!(fires(C, "total EUR 22186,12 today", "22186"));
        assert!(fires(C, "total 22186,12\u{00A0}EUR", "22186"));
        assert!(fires(C, "price $90210 now", "90210"));
        assert!(!fires(C, "EURO 80331 München", "80331"));
        assert!(!fires(C, "80331 München, EUR 50", "80331"));
        assert!(!fires(C, "80331 EURASIA", "80331"));
    }

    #[test]
    fn digit_run_fragment_needs_a_run_longer_than_any_phone_number() {
        use BenignLookalike::DigitRunFragment as D;
        assert!(fires(
            D,
            "itemCode=0593-9506-3395-7573 status",
            "0593-9506-3395"
        ));
        // A direct-dial extension keeps a phone number well under 16 digits.
        assert!(!fires(D, "Bitte 089/12345-0 bitte", "089/12345"));
        assert!(!fires(D, "Bitte 030-1234567-89 bitte", "030-1234567"));
        // The whole run is not a fragment.
        assert!(!fires(D, "x 0593-9506-3395-7573 y", "0593-9506-3395-7573"));
        // Mixed separators are two values, not one run.
        assert!(!fires(D, "0593-9506.3395-7573.1111", "0593-9506"));
    }
}
