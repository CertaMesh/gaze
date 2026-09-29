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

use crate::{
    LocaleBasis, LocaleTag, PiiClass, ValidatorFailReason, ValidatorKind, ValidatorOnFail,
};

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
/// Whether `recognizer_id` names an audited bundled rule (an early, friendly check for the
/// rulepack loader). Only a [`BenignLookalikeGrant`] actually permits a veto.
pub fn is_audited(recognizer_id: &str) -> bool {
    AUDITED_FINGERPRINTS
        .iter()
        .any(|(id, _)| *id == recognizer_id)
}

/// Stems of cue words for the classes that opt in (phone, postal code) and for addresses, in
/// every language Gaze ships a locale pack or a postal rule for (en, de, fr, es, it, nl, pt).
/// Words are case-folded and accent-folded first (`Téléphone` reads `telephone`), and a word
/// starting with a stem counts, so a longer word that merely starts like a cue also disables
/// the veto: matching errs toward protection. Every `phone_labels` entry in the bundled locale
/// packs must trip this check (`gaze-recognizers` drift test).
const CUE_STEMS: &[&str] = &[
    // phone
    "tel",
    "phone",
    "fon",
    "fax",
    "mobil",
    "mobiel",
    "movil",
    "cell",
    "celular",
    "handy",
    "portable",
    "ruf",
    "anruf",
    "call",
    "contact",
    "contatto",
    "contacto",
    "kontakt",
    // postal code
    "zip",
    "plz",
    "post",
    "codigo",
    "codice",
    "npa",
    // address
    "addr",
    "adres",
    "adress",
    "anschrift",
    "billing",
    "shipping",
    "delivery",
    "liefer",
    "street",
    "strasse",
    "straat",
    "calle",
    "indirizzo",
    "direccion",
    "endereco",
    "city",
    "stadt",
    "ort",
    "wohn",
    "ville",
    "citta",
    "ciudad",
    "cidade",
    "woonplaats",
];
/// Short cue words that only count as whole words: as stems they would match common words
/// (`capital`, `cepa`).
const CUE_WORDS: &[&str] = &["cap", "cp", "cep", "gsm", "rue", "rua"];
/// A label heads the record it labels, and a blank line ends a record, so the cue window reaches
/// back to the previous blank line. It is capped at [`BLOCK_LINES_ABOVE`] lines and
/// [`BLOCK_BYTES_ABOVE`] bytes above the candidate so one heading does not switch off every veto
/// in a long document, while a label line followed by a short instruction line or two (a typical
/// agent prompt) stays in reach.
const BLOCK_LINES_ABOVE: usize = 6;
const BLOCK_BYTES_ABOVE: usize = 400;
/// After the candidate: the rest of its line and the next non-blank line, this many bytes each.
const AFTER_BYTES: usize = 120;
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

    /// Whether `text[span]` sits inside this benign structure. `context` is the structured
    /// context the text came from, if any; a cue in it, or in the text around the candidate,
    /// keeps the candidate protected.
    pub fn matches(self, text: &str, span: Range<usize>, context: Option<&VetoContext>) -> bool {
        if span.start >= span.end
            || text.get(span.clone()).is_none()
            || context.is_some_and(VetoContext::has_cue)
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

/// Lowercases and strips the accents of common Latin letters, so `Téléphone` reads
/// `telephone` and `Straße` reads `strasse`.
fn fold(c: char) -> Vec<char> {
    let lower: Vec<char> = c.to_lowercase().collect();
    lower
        .into_iter()
        .flat_map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => vec!['a'],
            'ç' => vec!['c'],
            'è' | 'é' | 'ê' | 'ë' => vec!['e'],
            'ì' | 'í' | 'î' | 'ï' => vec!['i'],
            'ñ' => vec!['n'],
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => vec!['o'],
            'ù' | 'ú' | 'û' | 'ü' => vec!['u'],
            'ß' => vec!['s', 's'],
            other => vec![other],
        })
        .collect()
}

/// Whether any word of `text` is a cue: it starts with a [`CUE_STEMS`] stem or equals a
/// [`CUE_WORDS`] word, after case and accent folding. Words split at every non-letter and at
/// each lower-to-upper case change, so `postal_code`, `zipCode` and `phoneNumber` all count.
pub fn has_cue(text: &str) -> bool {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous_lower = false;
    for c in text.chars() {
        if (!c.is_alphabetic() || (c.is_uppercase() && previous_lower)) && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if c.is_alphabetic() {
            word.extend(fold(c));
        }
        previous_lower = c.is_lowercase();
    }
    words.push(word);
    words.iter().any(|word| {
        CUE_WORDS.contains(&word.as_str()) || CUE_STEMS.iter().any(|stem| word.starts_with(stem))
    })
}

/// Whether a cue stands in the candidate's block: back to the previous blank line (at most
/// [`BLOCK_LINES_ABOVE`] lines and [`BLOCK_BYTES_ABOVE`] bytes above the candidate's line),
/// the candidate's own line, and the next non-blank line.
fn cued(text: &str, span: Range<usize>) -> bool {
    let is_break = |c: char| c == '\n' || c == '\r';
    let head = &text[..span.start];
    let line_start = head.rfind(is_break).map_or(0, |index| index + 1);
    // Lines above, nearest first, until a blank line or a cap.
    let mut above = Vec::new();
    let mut rest = &head[..line_start];
    let mut bytes = 0;
    while above.len() < BLOCK_LINES_ABOVE && !rest.is_empty() {
        let trimmed = rest
            .strip_suffix("\r\n")
            .or_else(|| rest.strip_suffix(['\n', '\r']))
            .unwrap_or(rest);
        let start = trimmed.rfind(is_break).map_or(0, |index| index + 1);
        let line = &trimmed[start..];
        if line.trim().is_empty() {
            break;
        }
        // A line that crosses the byte cap still counts up to the cap, nearest bytes first.
        let budget = BLOCK_BYTES_ABOVE - bytes;
        if line.len() >= budget {
            let mut from = line.len() - budget;
            while !line.is_char_boundary(from) {
                from += 1;
            }
            above.push(&line[from..]);
            break;
        }
        bytes += line.len();
        above.push(line);
        rest = &trimmed[..start];
    }
    let tail = &text[span.end..];
    let line_end = tail.find(is_break).unwrap_or(tail.len());
    let below = tail[line_end..]
        .split(is_break)
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    has_cue(&head[line_start..])
        || above.into_iter().any(has_cue)
        || has_cue(first_bytes(&tail[..line_end]))
        || has_cue(first_bytes(below))
}

/// The first [`AFTER_BYTES`] bytes of `text`, widened to a character boundary.
fn first_bytes(text: &str) -> &str {
    let mut to = AFTER_BYTES.min(text.len());
    while !text.is_char_boundary(to) {
        to += 1;
    }
    &text[..to]
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

/// Structured context around a value: the keys on its path (`shippingAddress`, `code`), its
/// sibling keys and short sibling string values (`{"type": "phone", "value": ...}`). A cue
/// anywhere in it keeps every candidate in the value protected.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VetoContext {
    labels: Vec<String>,
}

/// Sibling string values longer than this are content, not labels.
pub const SIBLING_LABEL_MAX_BYTES: usize = 64;

impl VetoContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one label: a key on the path, a sibling key, or a short sibling value.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.labels.push(label.into());
        self
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Whether any label carries a cue.
    pub fn has_cue(&self) -> bool {
        self.labels.iter().any(|label| has_cue(label))
    }
}

/// The exact rule a benign lookalike veto may apply to: id, class, pattern, capture groups,
/// validator and its failure mode, locales and locale basis, and the structures it declares.
/// A caller describes its rule with this and asks [`BenignLookalikeGrant::audited`] for a grant.
#[derive(Debug, Clone, Copy)]
pub struct GrantRequest<'a> {
    pub id: &'a str,
    pub class: &'a PiiClass,
    pub pattern: &'a str,
    pub capture_groups: Option<&'a [u32]>,
    pub validator: Option<ValidatorKind>,
    pub on_fail: ValidatorOnFail,
    pub locales: &'a [LocaleTag],
    pub locale_basis: LocaleBasis,
    pub structures: &'a [BenignLookalike],
}

/// Fingerprints of the audited bundled rules, one per id. A grant is minted only for a request
/// whose fingerprint is listed here, so a rule that merely borrows an audited id (from a
/// rulepack handed to `Rulepack::parse_bundled`, or a custom `Recognizer`) never gets one.
/// `gaze-recognizers` pins these against its embedded `core` pack; update both together, and
/// only after a leak-direction review.
const AUDITED_FINGERPRINTS: &[(&str, &str)] = &[
    (
        "phone.national.de",
        "c74c5882ee5043938a26a341779dbcb919cf4aae3dca46b99fa97649614f6eba",
    ),
    (
        "phone.national.us",
        "6286ab60bc67af15dca3bb32ea242492cad079b2bba4dca3f7fcb0a405232fe1",
    ),
    (
        "postal.de",
        "8360f7b62bc7c502301cad396afb9d1cdb839c5dff405982caaacdd9fe1202c1",
    ),
    (
        "postal.us",
        "3d87bba275a2fde7bb1450b214de668b8f4e575b5dfbb008d6ba4adf1937a323",
    ),
];

/// The SHA3-256 fingerprint of a request, lowercase hex.
pub fn fingerprint(request: &GrantRequest<'_>) -> String {
    use sha3::{Digest, Sha3_256};
    let canonical = format!(
        "{}\u{0}{:?}\u{0}{}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}",
        request.id,
        request.class,
        request.pattern,
        request.capture_groups,
        request.validator,
        request.on_fail,
        request.locales,
        request.locale_basis,
        request.structures,
    );
    Sha3_256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Permission for one audited rule to be vetoed by benign lookalikes. Its fields are private
/// and [`Self::audited`] is its only constructor, so it cannot be forged: holding one proves
/// the rule is byte-for-byte an audited bundled rule. Validator veto still checks that the
/// recognizer presenting it has the same identity ([`Self::binds`]) and that each vetoed span is
/// a match of the audited pattern, so a grant borrowed from a real rule vetoes nothing else.
#[derive(Debug, Clone)]
pub struct BenignLookalikeGrant {
    id: String,
    class: PiiClass,
    pattern: String,
    capture_groups: Option<Vec<u32>>,
    validator: Option<ValidatorKind>,
    locales: Vec<LocaleTag>,
    locale_basis: LocaleBasis,
    structures: Vec<BenignLookalike>,
}

impl BenignLookalikeGrant {
    /// A grant for `request`, or `None` unless it is exactly an audited bundled rule. A rule
    /// that records validator failures is cued and never gets one.
    pub fn audited(request: &GrantRequest<'_>) -> Option<Self> {
        let expected = AUDITED_FINGERPRINTS
            .iter()
            .find(|(id, _)| *id == request.id)
            .map(|(_, fingerprint)| *fingerprint)?;
        (request.on_fail == ValidatorOnFail::Veto
            && !request.structures.is_empty()
            && fingerprint(request) == expected)
            .then(|| Self {
                id: request.id.to_string(),
                class: request.class.clone(),
                pattern: request.pattern.to_string(),
                capture_groups: request.capture_groups.map(<[u32]>::to_vec),
                validator: request.validator,
                locales: request.locales.to_vec(),
                locale_basis: request.locale_basis,
                structures: request.structures.to_vec(),
            })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The audited pattern; validator veto re-matches it to check a vetoed span.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The emitted capture groups of [`Self::pattern`]; `None` means the whole match.
    pub fn capture_groups(&self) -> Option<&[u32]> {
        self.capture_groups.as_deref()
    }

    pub fn structures(&self) -> &[BenignLookalike] {
        &self.structures
    }

    /// Whether `recognizer` presents the audited rule's identity: the same id, class,
    /// validator, failure mode, locales and locale basis. A recognizer that only borrows the
    /// grant but gates or validates differently is refused.
    pub fn binds(&self, recognizer: &(impl crate::Recognizer + ?Sized)) -> bool {
        recognizer.id() == self.id
            && recognizer.supported_class() == &self.class
            && recognizer.validator_kind() == self.validator
            && recognizer.validator_on_fail() == ValidatorOnFail::Veto
            && recognizer.locales() == self.locales.as_slice()
            && recognizer.locale_basis() == self.locale_basis
    }
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
        // A label anywhere in the block above counts (CRLF too), and on the next line below;
        // a blank line ends the block.
        assert!(!fires(JoinedIdentifier, "ZIP:\nORDER-90210", "90210"));
        assert!(!fires(
            JoinedIdentifier,
            "Telefon:\r\nORDER-0301234567",
            "0301234567"
        ));
        assert!(!fires(CurrencyAmount, "EUR 22186,12\n(phone)", "22186"));
        assert!(!fires(CurrencyAmount, "Tel\nnote\nEUR 22186,12", "22186"));
        assert!(!fires(
            JoinedIdentifier,
            "ZIP for delivery:\nUse the customer value below.\nORDER-90210",
            "90210"
        ));
        assert!(fires(CurrencyAmount, "Tel\n\nEUR 22186,12", "22186"));
        // The block reaches six lines up, and no further.
        let six = "ZIP\na\nb\nc\nd\ne\nEUR 22186,12";
        let seven = "ZIP\na\nb\nc\nd\ne\nf\nEUR 22186,12";
        assert!(!fires(CurrencyAmount, six, "22186"));
        assert!(fires(CurrencyAmount, seven, "22186"));
        // And 400 bytes up: a cue in the last 400 bytes of a long line above counts.
        let near = format!("{}ZIP {}\nEUR 22186,12", "x ".repeat(300), "y ".repeat(190));
        let far = format!("ZIP {}\nEUR 22186,12", "y ".repeat(205));
        assert!(!fires(CurrencyAmount, &near, "22186"));
        assert!(fires(CurrencyAmount, &far, "22186"));
        // Other languages, case- and accent-folded.
        for label in [
            "Téléphone:",
            "TÉL.",
            "Teléfono:",
            "Telefono:",
            "Código postal:",
            "CAP:",
            "Postcode:",
            "Telefoon:",
            "Endereço:",
            "Straße:",
            "Indirizzo:",
            "gsm",
        ] {
            assert!(
                !fires(JoinedIdentifier, &format!("{label} ORDER-90210"), "90210"),
                "{label}"
            );
        }
        // A word merely containing a cue does not count.
        assert!(fires(CurrencyAmount, "Hotel EUR 22186,12", "22186"));
    }

    #[test]
    fn a_cue_in_the_structured_context_disables_every_structure() {
        use BenignLookalike::JoinedIdentifier as J;
        let text = "ORDER-90210";
        let context = |labels: &[&str]| {
            labels.iter().fold(VetoContext::new(), |context, label| {
                context.with_label(*label)
            })
        };
        for labels in [
            &["postal_code"][..],
            &["zipCode"],
            &["PLZ"],
            &["customer", "telefon"],
            &["contact", "value"],
            &["shippingAddress", "code"],
            &["billing_address"],
            &["delivery-address"],
            &["Anschrift"],
            &["item", "value", "type", "phone"],
            &["item", "value", "label", "ZIP"],
        ] {
            assert!(
                !J.matches(text, at(text, "90210"), Some(&context(labels))),
                "{labels:?}"
            );
        }
        for labels in [
            &["order"][..],
            &["items", "reference"],
            &["itemCode", "sku"],
        ] {
            assert!(
                J.matches(text, at(text, "90210"), Some(&context(labels))),
                "{labels:?}"
            );
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

    #[test]
    fn only_audited_bundled_recognizers_are_eligible() {
        assert!(is_audited("postal.us"));
        assert!(is_audited("phone.national.de"));
        assert!(!is_audited("postal.at_ch"));
        assert!(!is_audited("custom.order_zip"));
    }
}

#[cfg(test)]
mod grant_tests {
    use super::*;

    fn request<'a>(
        pattern: &'a str,
        locales: &'a [LocaleTag],
        on_fail: ValidatorOnFail,
        class: &'a PiiClass,
        structures: &'a [BenignLookalike],
    ) -> GrantRequest<'a> {
        GrantRequest {
            id: "postal.us",
            class,
            pattern,
            capture_groups: None,
            validator: None,
            on_fail,
            locales,
            locale_basis: LocaleBasis::Document,
            structures,
        }
    }

    #[test]
    fn only_the_exact_audited_rule_is_granted() {
        let class = PiiClass::custom("postal_code").expect("class");
        let structures = [
            BenignLookalike::JoinedIdentifier,
            BenignLookalike::CurrencyAmount,
        ];
        let en_us = [LocaleTag::EnUs];
        let exact = request(
            r"\b\d{5}(-\d{4})?\b",
            &en_us,
            ValidatorOnFail::Veto,
            &class,
            &structures,
        );
        assert!(BenignLookalikeGrant::audited(&exact).is_some());
        // Any change to the tuple loses the grant.
        let pattern = request(
            r"ORDER-(\d{5})\s+Beverly",
            &en_us,
            ValidatorOnFail::Veto,
            &class,
            &structures,
        );
        assert!(BenignLookalikeGrant::audited(&pattern).is_none());
        let de = [LocaleTag::DeDe];
        let locale = request(
            r"\b\d{5}(-\d{4})?\b",
            &de,
            ValidatorOnFail::Veto,
            &class,
            &structures,
        );
        assert!(BenignLookalikeGrant::audited(&locale).is_none());
        let record = request(
            r"\b\d{5}(-\d{4})?\b",
            &en_us,
            ValidatorOnFail::Record,
            &class,
            &structures,
        );
        assert!(BenignLookalikeGrant::audited(&record).is_none());
        let more = [
            BenignLookalike::JoinedIdentifier,
            BenignLookalike::CurrencyAmount,
            BenignLookalike::DigitRunFragment,
        ];
        let widened = request(
            r"\b\d{5}(-\d{4})?\b",
            &en_us,
            ValidatorOnFail::Veto,
            &class,
            &more,
        );
        assert!(BenignLookalikeGrant::audited(&widened).is_none());
    }
}
