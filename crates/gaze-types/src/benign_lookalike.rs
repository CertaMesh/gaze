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
//! A cue anywhere keeps every value protected: validator veto scans the whole
//! document (or, for a structured document, every key and string value of the
//! whole record) with [`CueEvidence`], and a postal, phone or address cue for the
//! candidate's family, or any letter outside Latin script, disables the veto.

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

/// The single source these lists share with the benchmark generator is
/// `scripts/bench/lookalike_cue_vocabulary.json`; a `gaze-assembly` test requires exact equality.
///
/// Cue stems per family, in every language Gaze ships a locale pack or a postal rule for (en,
/// de, fr, es, it, nl, pt). Words are case- and accent-folded first (`Téléphone` reads
/// `telephone`), and a word starting with a stem counts, so a longer word that merely starts
/// like a cue also counts: matching errs toward protection. Every `phone_labels` entry in the
/// bundled locale packs must be a phone cue (`gaze-assembly` drift test).
pub const POSTAL_STEMS: &[&str] = &["zip", "plz", "post", "codigo", "codice", "npa"];
pub const PHONE_STEMS: &[&str] = &[
    "tel", "phone", "fon", "fax", "mobil", "mobiel", "movil", "cell", "celular", "handy",
    "portable", "ruf", "anruf", "call",
];
/// Address and contact words: a postcode or a phone number in an address or contact record is
/// the record's own, so these block both families.
pub const ADDRESS_STEMS: &[&str] = &[
    "contact",
    "contatto",
    "contacto",
    "kontakt",
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
pub const POSTAL_WORDS: &[&str] = &["cap", "cp", "cep"];
pub const PHONE_WORDS: &[&str] = &["gsm"];
pub const ADDRESS_WORDS: &[&str] = &["rue", "rua"];
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

    /// Whether `text[span]` sits inside this benign structure. This is the positive structural
    /// evidence only; validator veto also requires [`CueEvidence`] for the whole document or
    /// record to allow it.
    pub fn matches(self, text: &str, span: Range<usize>) -> bool {
        if span.start >= span.end || text.get(span.clone()).is_none() {
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

/// Whether `c` is a Latin-script letter: Basic Latin, Latin-1 Supplement, Latin Extended-A/B,
/// Latin Extended Additional, Latin Extended-C/D/E and fullwidth Latin. Any other letter
/// (Cyrillic, Greek, CJK, Arabic, Hebrew, Devanagari, ...) is a script the cue stems cannot
/// read.
fn is_latin_letter(c: char) -> bool {
    matches!(
        u32::from(c),
        0x41..=0x24F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF | 0xAB30..=0xAB6F
            | 0xFF21..=0xFF5A
    )
}

/// What a whole document or record says about postcodes and phone numbers. A benign lookalike
/// veto needs positive structural evidence *and* the absence of every cue for its family
/// anywhere in the document or record: a bounded window can never prove that a labelled value
/// is benign, so the scan has no window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CueEvidence {
    postal: bool,
    phone: bool,
    non_latin: bool,
}

impl CueEvidence {
    /// Scans every word and letter of `text`. Words split at every non-letter and at each
    /// lower-to-upper case change, so `postal_code`, `zipCode` and `phoneNumber` all count.
    pub fn scan(text: &str) -> Self {
        let mut evidence = Self::default();
        let mut word = String::new();
        let mut previous_lower = false;
        for c in text.chars().chain(std::iter::once(' ')) {
            if (!c.is_alphabetic() || (c.is_uppercase() && previous_lower)) && !word.is_empty() {
                evidence.add_word(&std::mem::take(&mut word));
            }
            if c.is_alphabetic() {
                if !is_latin_letter(c) {
                    evidence.non_latin = true;
                }
                word.extend(fold(c));
            }
            previous_lower = c.is_lowercase();
        }
        evidence
    }

    fn add_word(&mut self, word: &str) {
        let hit = |stems: &[&str], words: &[&str]| {
            words.contains(&word) || stems.iter().any(|stem| word.starts_with(stem))
        };
        if hit(ADDRESS_STEMS, ADDRESS_WORDS) {
            self.postal = true;
            self.phone = true;
        }
        self.postal |= hit(POSTAL_STEMS, POSTAL_WORDS);
        self.phone |= hit(PHONE_STEMS, PHONE_WORDS);
    }

    /// Both scans' evidence: a text leaf inherits its record's cues.
    pub fn merge(self, other: Self) -> Self {
        Self {
            postal: self.postal || other.postal,
            phone: self.phone || other.phone,
            non_latin: self.non_latin || other.non_latin,
        }
    }

    /// A postal, address or contact cue appears.
    pub fn postal(&self) -> bool {
        self.postal
    }

    /// A phone, address or contact cue appears.
    pub fn phone(&self) -> bool {
        self.phone
    }

    /// A letter outside Latin script appears.
    pub fn non_latin(&self) -> bool {
        self.non_latin
    }

    /// Whether this evidence forbids a benign lookalike veto for `class`. Any cue of either
    /// family, or any non-Latin letter, forbids every veto: a record that labels a phone may
    /// also hold a postcode, so a cue for one family is uncertainty for the other. A class
    /// other than postcode or phone is never vetoed.
    pub fn blocks(&self, class: &PiiClass) -> bool {
        let vetoable = matches!(
            class,
            PiiClass::Custom(name) if name == "postal_code" || name == "phone"
        );
        !vetoable || self.postal || self.phone || self.non_latin
    }
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
        // The value must end the token.
        assert!(!fires(J, "SKU-73821-A1 x", "73821"));
        assert!(!fires(J, "SKU 73821", "73821"));
    }

    #[test]
    fn cue_evidence_reads_the_whole_document() {
        let postal = PiiClass::custom("postal_code").expect("class");
        let phone = PiiClass::custom("phone").expect("class");
        let scan = CueEvidence::scan;
        // No cue: neither family is blocked.
        let plain = scan("Lagerartikel SKU-DEMO-73821 und Charge BATCH-SAMPLE-92163; fertig.");
        assert!(!plain.blocks(&postal) && !plain.blocks(&phone));
        // A cue anywhere, however far away, blocks every family; a blank line changes nothing.
        let far = format!("ZIP code:\n\n{}\nORDER-90210", "note line\n".repeat(50));
        assert!(scan(&far).postal() && !scan(&far).phone());
        assert!(scan(&far).blocks(&postal) && scan(&far).blocks(&phone));
        let end = format!("ORDER-90210\n{}\nZIP", "x ".repeat(5000));
        assert!(scan(&end).blocks(&postal));
        let tel = scan("ORDER-212-555-0187\n\nTelefon des Kunden");
        assert!(tel.phone() && !tel.postal());
        assert!(tel.blocks(&phone) && tel.blocks(&postal));
        // Address and contact words count for both families.
        for word in [
            "shippingAddress",
            "Anschrift",
            "contact",
            "Endereço",
            "Straße",
            "rue",
        ] {
            let evidence = scan(&format!("{word}: ORDER-90210"));
            assert!(evidence.postal() && evidence.phone(), "{word}");
        }
        // Seven Latin-script languages, case- and accent-folded, camelCase and snake_case.
        for label in [
            "Téléphone",
            "TÉL.",
            "Teléfono",
            "Telefono",
            "Telefoon",
            "gsm",
            "phoneNumber",
            "mobile_no",
        ] {
            assert!(scan(label).phone(), "{label}");
        }
        for label in [
            "Código postal",
            "CAP",
            "Postcode",
            "postal_code",
            "zipCode",
            "PLZ",
            "cep",
        ] {
            assert!(scan(label).postal(), "{label}");
        }
        // A word merely containing a cue, or a short word only as a stem, does not count.
        let near_miss = scan("Hotel capital recap");
        assert!(!near_miss.blocks(&postal) && !near_miss.blocks(&phone));
    }

    #[test]
    fn any_non_latin_letter_blocks_every_veto() {
        let postal = PiiClass::custom("postal_code").expect("class");
        let phone = PiiClass::custom("phone").expect("class");
        for text in [
            "Телефон: ORDER-212-555-0187",
            "電話番号: ORDER-212-555-0187",
            "ORDER-90210 and one Greek letter λ",
            "مرحبا ORDER-90210",
        ] {
            let evidence = CueEvidence::scan(text);
            assert!(evidence.non_latin(), "{text}");
            assert!(
                evidence.blocks(&postal) && evidence.blocks(&phone),
                "{text}"
            );
        }
        // Accented Latin letters are Latin.
        assert!(!CueEvidence::scan("Müller Łódź Ærø Șerban").non_latin());
    }

    #[test]
    fn merged_record_evidence_blocks_a_clean_leaf() {
        let postal = PiiClass::custom("postal_code").expect("class");
        let record = CueEvidence::scan("entry meta type phone number for customer contact");
        let leaf = CueEvidence::scan("ORDER-90210");
        assert!(!leaf.blocks(&postal));
        assert!(leaf.merge(record).blocks(&postal));
        // A class other than postcode or phone is never vetoed.
        assert!(leaf.blocks(&PiiClass::custom("iban").expect("class")));
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
