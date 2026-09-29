//! Benign lookalike structures: text around a weak, cue-less shape match that
//! positively identifies a non-PII value (an order number, an amount, a room).
//!
//! A recognizer opts in per structure (`[recognizers.context] benign_lookalikes`).
//! Validator veto then drops that recognizer's candidate before conflict
//! resolution with a typed [`ValidatorFailReason`], exactly like a failed
//! validator. Only the opted-in recognizer's own candidate is dropped; any other
//! candidate over the same bytes (a cued phone, an IBAN, a card) still protects
//! them. Every check reads only ASCII context next to the candidate span.

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
    /// A room, seat or gate label sits directly before the candidate: `Room 4833`.
    LabelNumber,
}

/// Label words that open a document-reference identifier. Cue words of any PII class
/// (`tel`, `plz`, `zip`, ...) must never appear here.
const REFERENCE_LABELS: &[&str] = &[
    "art", "artikel", "auftrag", "batch", "beleg", "bestellung", "case", "charge", "gutschein",
    "inv", "invoice", "item", "lot", "ord", "order", "po", "quote", "rechnung", "ref", "rma",
    "shipment", "sku", "ticket", "track", "tracking", "voucher", "vorgang",
];
const PLACE_LABELS: &[&str] = &[
    "etage", "floor", "gate", "gleis", "platform", "raum", "room", "seat", "sitz", "zimmer",
];
/// Cue words of the classes that opt in (phone, postal code). One of these on the same line
/// shortly before the candidate means the writer labelled it as PII, so no structure fires.
const CUE_WORDS: &[&str] = &[
    "call", "cell", "fax", "handy", "mobil", "mobile", "phone", "plz", "postal", "postcode",
    "postleitzahl", "ruf", "tel", "telefon", "telephone", "zip",
];
/// How far before the candidate a cue word still counts, in bytes.
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
            "label_number" => Some(Self::LabelNumber),
            _ => None,
        }
    }

    /// The audit reason a veto by this structure records.
    pub fn reason(self) -> ValidatorFailReason {
        match self {
            Self::JoinedIdentifier => ValidatorFailReason::BenignJoinedIdentifier,
            Self::CurrencyAmount => ValidatorFailReason::BenignCurrencyAmount,
            Self::DigitRunFragment => ValidatorFailReason::BenignDigitRunFragment,
            Self::LabelNumber => ValidatorFailReason::BenignLabelNumber,
        }
    }

    /// Whether `text[span]` sits inside this benign structure.
    pub fn matches(self, text: &str, span: Range<usize>) -> bool {
        if span.start >= span.end || text.get(span.clone()).is_none() || cued(text, span.start) {
            return false;
        }
        match self {
            Self::JoinedIdentifier => joined_identifier(text, span),
            Self::CurrencyAmount => currency_amount(text, span),
            Self::DigitRunFragment => digit_run_fragment(text, span),
            Self::LabelNumber => label_number(text, span),
        }
    }
}

/// Whether a cue word stands on the candidate's line within [`CUE_WINDOW`] bytes before it.
fn cued(text: &str, start: usize) -> bool {
    let mut from = start.saturating_sub(CUE_WINDOW);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let window = &text[from..start];
    let window = window.rsplit(['\n', '\r']).next().unwrap_or(window);
    window
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|word| CUE_WORDS.contains(&word.to_ascii_lowercase().as_str()))
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
    text.strip_suffix([' ', '\u{00A0}', '\u{202F}']).unwrap_or(text)
}

fn strip_gap_start(text: &str) -> &str {
    text.strip_prefix([' ', '\u{00A0}', '\u{202F}']).unwrap_or(text)
}

fn currency_amount(text: &str, span: Range<usize>) -> bool {
    let before = strip_gap_end(&text[..span.start]);
    let code_before = CURRENCY_CODES.iter().any(|code| {
        before.strip_suffix(code).is_some_and(|rest| !rest.bytes().next_back().is_some_and(is_word))
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
        && !decimals.get(3).copied().is_some_and(|byte| byte.is_ascii_digit())
    {
        after = &after[3..];
    }
    let after = strip_gap_start(after);
    after.starts_with(CURRENCY_SIGNS)
        || CURRENCY_CODES.iter().any(|code| {
            after.strip_prefix(code).is_some_and(|rest| !rest.bytes().next().is_some_and(is_word))
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

fn label_number(text: &str, span: Range<usize>) -> bool {
    let before = &text[..span.start];
    let stripped = strip_gap_end(before);
    if stripped.len() == before.len() {
        return false;
    }
    let word_start = stripped
        .bytes()
        .rposition(|byte| !byte.is_ascii_alphabetic())
        .map_or(0, |index| index + 1);
    let word = &stripped[word_start..];
    let bounded = !stripped[..word_start].bytes().next_back().is_some_and(is_word);
    bounded && PLACE_LABELS.contains(&word.to_ascii_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, value: &str) -> Range<usize> {
        let start = text.find(value).expect("value in text");
        start..start + value.len()
    }

    fn fires(kind: BenignLookalike, text: &str, value: &str) -> bool {
        kind.matches(text, at(text, value))
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
        assert!(!fires(J, "PLZ-80331 München", "80331"));
        assert!(!fires(J, "Tel-0301234567", "0301234567"));
        // The value must end the token.
        assert!(!fires(J, "SKU-73821-A1 x", "73821"));
        assert!(!fires(J, "SKU 73821", "73821"));
    }

    #[test]
    fn a_cue_word_before_the_candidate_disables_every_structure() {
        assert!(!fires(BenignLookalike::CurrencyAmount, "Tel: 2125550187 USD", "2125550187"));
        assert!(!fires(BenignLookalike::JoinedIdentifier, "zip SKU-DEMO-73821", "73821"));
        assert!(!fires(
            BenignLookalike::DigitRunFragment,
            "Telefon 0593-9506-3395-7573",
            "0593-9506-3395"
        ));
        assert!(!fires(BenignLookalike::LabelNumber, "PLZ Raum 1010", "1010"));
        // A cue on an earlier line does not count; neither does a cue-shaped word part.
        assert!(fires(BenignLookalike::LabelNumber, "Tel\nRoom 4833", "4833"));
        assert!(fires(BenignLookalike::CurrencyAmount, "Hotel EUR 22186,12", "22186"));
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
        assert!(fires(D, "itemCode=0593-9506-3395-7573 status", "0593-9506-3395"));
        // A direct-dial extension keeps a phone number well under 16 digits.
        assert!(!fires(D, "Tel. 089/12345-0 bitte", "089/12345"));
        assert!(!fires(D, "Tel. 030-1234567-89 bitte", "030-1234567"));
        // The whole run is not a fragment.
        assert!(!fires(D, "x 0593-9506-3395-7573 y", "0593-9506-3395-7573"));
        // Mixed separators are two values, not one run.
        assert!(!fires(D, "0593-9506.3395-7573.1111", "0593-9506"));
    }

    #[test]
    fn label_number_needs_a_place_label_right_before_the_value() {
        use BenignLookalike::LabelNumber as L;
        assert!(fires(L, "values: Room 4833 Room 4844", "4833"));
        assert!(fires(L, "im Raum\u{00A0}1010", "1010"));
        assert!(!fires(L, "Bedroom 4833 Wien", "4833"));
        assert!(!fires(L, "1010 Wien Room", "1010"));
        assert!(!fires(L, "Room4833", "4833"));
    }
}
