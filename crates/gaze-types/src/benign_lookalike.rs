//! Benign lookalikes: values whose benignness is proven by the value itself,
//! never by a missing label.
//!
//! Only one structure remains: a phone candidate that is a strict part of one
//! digit run longer than any E.164 number cannot be a phone number, whatever
//! its label. (Loopback IP addresses are excluded by the IP validators.) Earlier
//! structures that rested on the absence of a recognised label (an order-number
//! tail, a currency amount) were removed: a finite cue list cannot prove that a
//! label is absent (markup, typos, other languages and encodings all evade it).
//!
//! A recognizer opts in per structure (`[recognizers.context] benign_lookalikes`),
//! and only with a [`BenignLookalikeGrant`] for its exact audited rule. As extra
//! caution, validator veto still scans the whole document or record with
//! [`CueEvidence`] and keeps every candidate when any cue word or any non-Latin
//! letter appears.

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
    /// The candidate is a strict part of one same-separator digit-group run holding at least
    /// 16 digits, longer than any E.164 number: the SKU `0593-9506-3395-7573`.
    DigitRunFragment,
}

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
/// No E.164 number has more digits than this, so a longer run cannot be one phone number.
const MAX_PHONE_DIGITS: usize = 15;

impl BenignLookalike {
    /// Parses a rulepack spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "digit_run_fragment" => Some(Self::DigitRunFragment),
            _ => None,
        }
    }

    /// The audit reason a veto by this structure records.
    pub fn reason(self) -> ValidatorFailReason {
        match self {
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

/// Zero-width characters, the soft hyphen, the word joiner, the byte-order mark and combining
/// marks: read as nothing, so they cannot split a cue word. A combining mark after a base
/// letter is its decomposed accent (`e` + U+0301 reads `e`).
fn is_invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD | 0x200B..=0x200D | 0x2060 | 0xFEFF | 0x0300..=0x036F | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
    )
}

/// `text` with markup-like tags removed: a `<` followed within 64 characters by a `>` with no
/// other `<` in between. `Ph<b>one</b>` reads `Phone`.
fn strip_markup(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('<') {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        let close = tail
            .char_indices()
            .take(64)
            .find(|&(_, c)| c == '>' || c == '<')
            .filter(|&(_, c)| c == '>');
        match close {
            Some((end, _)) => rest = &tail[end + 1..],
            None => {
                out.push('<');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
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
    /// Scans every word and letter of `text`. Before splitting, markup-like tags (`<b>`,
    /// `</span>`) are removed, and zero-width characters, soft hyphens and combining marks are
    /// read as nothing, so `Ph<b>one</b>`, `Ph\u{200b}one` and a decomposed `Te\u{301}l` read
    /// `phone` and `tel`. Words then split at every other non-letter and at each lower-to-upper
    /// case change, so `postal_code`, `zipCode` and `phoneNumber` all count. Encoded labels
    /// (percent-encoding, base64) are not decoded.
    pub fn scan(text: &str) -> Self {
        let text = strip_markup(text);
        let mut evidence = Self::default();
        let mut word = String::new();
        let mut previous_lower = false;
        for c in text.chars().chain(std::iter::once(' ')) {
            if is_invisible(c) {
                continue;
            }
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
        "8d0d575fafa9b73df08bc17d699919a2fb1a5c16de48ac764d22b003766a6551",
    ),
    (
        "phone.national.us",
        "caee26c9cfd34a190cdafc41555b7a81a220457055c5fe77cbcf47ad92cfc8b1",
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
        // Markup, zero-width characters and decomposed accents cannot hide a cue.
        for label in [
            "Ph<b>one</b>:",
            "Ph\u{200b}one:",
            "Z<b>IP</b>:",
            "Te\u{301}l.:",
            "Tel\u{ad}efon",
        ] {
            let evidence = scan(label);
            assert!(
                evidence.blocks(&postal) && evidence.blocks(&phone),
                "{label:?}"
            );
        }
        // A lone `<` or `>` (a comparison) is kept as text.
        assert!(scan("a < b and ZIP").postal());
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
        assert!(is_audited("phone.national.us"));
        assert!(is_audited("phone.national.de"));
        assert!(!is_audited("postal.us"));
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
    fn only_the_audited_phone_rules_can_be_granted() {
        let postal = PiiClass::custom("postal_code").expect("class");
        let phone = PiiClass::custom("phone").expect("class");
        let structures = [BenignLookalike::DigitRunFragment];
        let en_us = [LocaleTag::EnUs];
        // The postal rules were audited once but carry no structure any more: no grant.
        let postal_us = request(
            r"\b\d{5}(-\d{4})?\b",
            &en_us,
            ValidatorOnFail::Veto,
            &postal,
            &structures,
        );
        assert!(BenignLookalikeGrant::audited(&postal_us).is_none());
        assert!(!is_audited("postal.us") && !is_audited("postal.de"));
        assert!(is_audited("phone.national.us") && is_audited("phone.national.de"));
        // A borrowed audited id with another pattern, or a recorded-failure rule, never is.
        let mut spoof = request(
            r"\d{10}",
            &en_us,
            ValidatorOnFail::Veto,
            &phone,
            &structures,
        );
        spoof.id = "phone.national.us";
        assert!(BenignLookalikeGrant::audited(&spoof).is_none());
        spoof.on_fail = ValidatorOnFail::Record;
        assert!(BenignLookalikeGrant::audited(&spoof).is_none());
        // The positive case, the exact bundled phone rules, is pinned end to end: building the
        // bundled pipeline fails unless both mint a grant.
    }
}
