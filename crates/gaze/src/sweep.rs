//! Repeat-value sweep (solo todo 3849).
//!
//! Once a rule-found value is tokenized, every other copy of it in the same
//! document and in later documents of the same session must be tokenized too.
//! Without the sweep a copy was protected only when a recognizer happened to
//! fire at that exact spot, so a name caught in an email header shipped raw in
//! the body and in the next turn.
//!
//! This module owns the matching rules; the pipeline turns hits into resolver
//! candidates after resolve and before the safety net, and the session keeps
//! the evidence each value was found with. Only deterministic evidence
//! propagates: copying NER-found values spreads the model's mistakes across
//! whole documents (91 % of the new false positives on TAB court cases).
//!
//! Matching rules, each one a deliberate precision/recall trade-off:
//! - Multi-word or non-alphabetic values match case-insensitively, and any
//!   whitespace run matches any whitespace run. Case folding can change a
//!   character's length (Turkish `İ` lowers to two scalars), so every folded
//!   byte keeps the raw range of its source character and a match must start
//!   and end on whole characters.
//! - A single alphabetic word (a whole value or a part of a multi-word name)
//!   matches only as written, in title case or in upper case, and never when
//!   it is on the closed common-word list. Matching lower-case single words
//!   would tokenize ordinary words that happen to be names, so a lone
//!   lower-case `maria` is a stated leak.
//! - Two or more adjacent parts of a multi-word name match like a multi-word
//!   value when one of them is distinctive: a cue-found `Herr Tobias Brenner`
//!   also sweeps `TOBIAS BRENNER`.
//! - A name copy grows over parts glued on by a hyphen or apostrophe, so
//!   `Vasquez` never leaves `-Ellery` raw.
//! - Collision-family tokens match byte-exact only: the family class exists
//!   because the value's class was ambiguous, and folding would widen that.
//! - Every edge must pass [`gaze_types::is_inside_word`], and a hit inside a
//!   URL-shaped run is skipped so a link is never cut apart.

use std::collections::HashSet;
use std::ops::Range;

use aho_corasick::{AhoCorasick, MatchKind};
use gaze_types::{
    extend_over_name_joiners, is_inside_word, Candidate, ConflictTier, EvidenceKind, PiiClass,
};
use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// Recognizer id, source label and audit stage of a swept copy.
pub(crate) const SWEEP_ID: &str = "manifest_sweep";

/// Precision floors, one per [`ValueShape`]. Whole values below their floor
/// are left to their own recognizers.
const MIN_VALUE_CHARS: usize = 4;
/// A digit run needs this many digits. Short numbers (a four-digit AT/CH
/// postcode, a bare five-digit postal code) are found only through the cue or
/// city next to them, and that anchor is their whole precision: bare `\d{4}`
/// is 19 % precise. Copying the digits alone would tokenize years and room
/// numbers (`2024 Neuchâtel` then `Im Jahr 2024`).
const MIN_DIGIT_RUN: usize = 6;
/// Name parts need at least this many letters.
const MIN_PART_LETTERS: usize = 3;
/// Runs of parts are built only for names of at most this many parts, which
/// bounds the patterns one value adds (a header name has at most four).
const MAX_RUN_PARTS: usize = 6;
/// The sweep fails closed instead of degrading when a session's value list
/// grows past either cap.
const MAX_PATTERNS: usize = 200_000;
const MAX_PATTERN_BYTES: usize = 16 * 1024 * 1024;

/// Closed list of words that are also common first names or surnames. A
/// single-word value or name part on this list is never swept.
#[rustfmt::skip]
const COMMON_WORDS: &[&str] = &[
    // Months and weekdays, English and German.
    "january", "february", "march", "april", "may", "june", "july", "august",
    "september", "october", "november", "december", "januar", "februar", "maerz",
    "märz", "mai", "juni", "juli", "oktober", "dezember", "monday", "tuesday",
    "wednesday", "thursday", "friday", "saturday", "sunday", "montag", "dienstag",
    "mittwoch", "donnerstag", "freitag", "samstag", "sonntag",
    // Names that are everyday English words.
    "will", "mark", "rose", "grace", "hope", "joy", "faith", "summer", "autumn",
    "dawn", "eve", "iris", "lily", "daisy", "ivy", "holly", "ruby", "amber",
    "pearl", "crystal", "sky", "rain", "river", "stone", "wood", "hill", "field",
    "bill", "frank", "art", "sue", "pat", "rob", "jack", "guy", "ray", "bob",
    "don", "earl", "king", "page", "chase", "rich", "sterling", "young", "long",
    "little", "white", "black", "brown", "green", "gray", "grey", "cook", "baker",
    "miller", "smith", "hunter", "fisher", "carter", "mason", "porter", "turner",
    // Names that are everyday German words.
    "sonne", "winter", "sommer", "herbst", "lange", "klein", "gross", "groß",
    "weiss", "weiß", "schwarz", "braun", "roth", "jung", "alt", "berg", "wald",
    "stein", "bach", "graf", "koch", "fuchs", "wolf", "vogel", "haas", "kaiser",
    "koenig", "könig", "herr", "frau",
    // German surnames that are everyday nouns. German capitalises every noun,
    // so title case does not tell `Der Richter hat entschieden` from a name.
    "richter", "bauer", "fischer", "müller", "mueller", "schneider", "weber",
    "meyer", "meier", "maier", "mayer", "wagner", "becker", "bäcker", "baecker",
    "schäfer", "schaefer", "jäger", "jaeger", "zimmermann", "hoffmann", "hofmann",
    "keller", "engel", "hahn", "busch", "brandt", "schmidt", "schmid", "schulz",
    "schulze", "krüger", "krueger", "neumann", "lehmann", "kaufmann", "schuster",
    "kramer", "krämer", "metzger", "maurer", "wirth", "vogt", "förster", "gärtner",
    "pfarrer", "schreiber", "meister", "berger", "hartmann", "bergmann",
    // English surnames that are everyday verbs or nouns, sentence-initial in
    // title case (`Grant access to the repo.`).
    "grant", "price", "rice", "bush", "banks", "wells", "cross", "lane", "west",
    "dean", "case", "bond", "burns", "marsh", "moss", "ford", "fox", "lamb",
    "park", "parks", "reed", "ward", "bell", "bird", "brook", "brooks", "hall",
    "hart", "house", "hunt", "knight", "lord", "love", "low", "marshall", "mills",
    "noble", "north", "south", "east", "pike", "pool", "pope", "power", "sharp",
    "short", "spring", "steel", "swift", "walker", "street", "gates", "means",
];

/// Name particles and articles. They sit inside names but are everyday words
/// on their own, so a part or a run made only of them is never a name:
/// `von der` of `Anna von der Heide` must not sweep `Unterlagen von der Bank`
/// (review F1 and re-verify R1 on #691).
///
/// Compiled per naming convention, covering every setup-policy locale and the
/// common particle cultures. Each is a closed grammatical set, so the list
/// is by convention, not by corpus frequency:
/// - German nobiliary and locative particles: von, vom, zu, zum, zur, am, an,
///   auf, im, in, der, den, dem, des.
/// - Dutch and Flemish tussenvoegsels: van, de, der, den, het, 't, ten, ter,
///   te, op, in, uit, aan, bij, over, voor, onder.
/// - French: de, du, des, la, le, les (`d'` is a joiner, not a part).
/// - Spanish and Catalan: de, del, la, las, los, y, i.
/// - Portuguese: da, das, do, dos, de, e.
/// - Italian: di, da, dal, dalla, dalle, dei, degli, del, della, delle, dello,
///   lo.
/// - Scandinavian: af, av.
/// - Arabic and Hebrew patronymics and articles: al, el, bin, ibn, bint, abu,
///   abd, bat, bar. Not `ben`: `Ben` is a common English given name, and a
///   particle is never swept alone, so listing it would ship `Thanks, Ben`
///   raw; lower-case `ben` is not an everyday word to protect.
/// - Gaelic and Welsh patronymics: mac, mc, ni, ap, ab, ferch. Not `nic`,
///   for the same reason as `ben` (`Nic` is a given name).
#[rustfmt::skip]
const PARTICLES: &[&str] = &[
    "von", "vom", "zu", "zum", "zur", "am", "an", "auf", "im", "in", "der",
    "den", "dem", "des", "van", "de", "het", "'t", "ten", "ter", "te", "op",
    "uit", "aan", "bij", "over", "voor", "onder", "du", "la", "le", "les",
    "del", "las", "los", "y", "i", "da", "das", "do", "dos", "e", "di", "dal",
    "dalla", "dalle", "dei", "degli", "della", "delle", "dello", "lo", "af",
    "av", "al", "el", "bin", "ibn", "bint", "abu", "abd", "bat", "bar",
    "mac", "mc", "ni", "ap", "ab", "ferch",
];

/// Organization and role words in sender names (`SUPPORT TEAM`, `DHL Paket`).
/// A header value made of them is tokenized as a whole, but none of them is a
/// name part on its own (review F2 on #691).
#[rustfmt::skip]
const ORG_ROLE_WORDS: &[&str] = &[
    "support", "team", "service", "info", "paket", "kundenservice", "noreply",
    "newsletter",
];

/// Certainty of the evidence a manifest value was found with, ordered like
/// the resolver's evidence tiers. Persisted per manifest entry in the session
/// blob so a later turn can tell rule-found values from model-found ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManifestEvidence {
    /// A learned NER span. Recorded, never swept.
    Learned,
    /// A plain regex or dictionary term, or a swept copy of a rule value.
    Pattern,
    /// An anchored or cue-structured match.
    Anchored,
    /// A validator passed.
    Validated,
}

impl ManifestEvidence {
    pub(crate) fn of(candidate: &Candidate) -> Self {
        if candidate.recognizer_id == SWEEP_ID {
            Self::Pattern
        } else if candidate.canonical_form.is_some() {
            Self::Validated
        } else if candidate.source.starts_with("structural.") {
            Self::Anchored
        } else {
            match candidate.evidence {
                EvidenceKind::Learned => Self::Learned,
                EvidenceKind::Rule => Self::Pattern,
            }
        }
    }

    pub(crate) fn propagates(self) -> bool {
        self >= Self::Pattern
    }
}

/// A manifest value the sweep looks for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SweepSource {
    pub(crate) family: String,
    pub(crate) class: PiiClass,
    pub(crate) raw: String,
}

/// How a swept copy relates to the value it was found from. Audit rows carry
/// this instead of the source token: the audit contract never holds tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SweepLink {
    /// Byte-identical after normalization: the copy reuses the source token.
    Exact,
    /// A different spelling (case, whitespace): the copy gets a sibling token.
    Variant,
    /// A part, or a run of parts, of a multi-word name.
    Part,
}

impl SweepLink {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "manifest_sweep:exact",
            Self::Variant => "manifest_sweep:variant",
            Self::Part => "manifest_sweep:part",
        }
    }
}

/// One copy found in normalized-text coordinates.
#[derive(Clone, Debug)]
pub(crate) struct SweepHit {
    pub(crate) span: Range<usize>,
    pub(crate) family: String,
    pub(crate) class: PiiClass,
    pub(crate) link: SweepLink,
}

impl SweepHit {
    /// The resolver candidate for this copy. It carries the source's class
    /// and token family so a byte-identical copy maps to the same token key.
    pub(crate) fn candidate(&self) -> Candidate {
        let mut candidate = Candidate::new(
            self.span.clone(),
            self.class.clone(),
            SWEEP_ID,
            1.0,
            // A copy in a gap needs no tie-break. Strict same-class containment
            // now selects the enclosing span; this priority retains the sweep
            // choice for other overlaps with a weaker fragment.
            i32::MAX,
            None,
            self.family.clone(),
            SWEEP_ID,
            ConflictTier::ManifestSweep,
            Vec::new(),
        );
        candidate.source_recognizer_ids = vec![SWEEP_ID.to_string()];
        // A copy of a rule-found value is itself rule evidence.
        candidate.with_evidence(EvidenceKind::Rule)
    }
}

#[derive(Clone, Copy)]
enum PatternKind {
    Folded,
    Exact,
}

struct Pattern {
    kind: PatternKind,
    text: String,
    source: usize,
    link: SweepLink,
}

/// Multi-pattern matcher over one value list. Built once per session state
/// (cached like the restore regex) and once per document for that
/// document's own rule-found values.
pub(crate) struct SweepMatcher {
    sources: Vec<SweepSource>,
    folded: Option<(AhoCorasick, Vec<(usize, SweepLink)>)>,
    exact: Option<(AhoCorasick, Vec<(usize, SweepLink)>)>,
}

impl std::fmt::Debug for SweepMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SweepMatcher")
            .field("sources", &self.sources.len())
            .finish_non_exhaustive()
    }
}

/// The sweep refused to run; the request fails closed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestSweepError {
    /// The value list passed the pattern-count or pattern-byte cap.
    #[error("manifest sweep value list exceeds its size cap")]
    CapacityExceeded,
    /// The multi-pattern matcher could not be built.
    #[error("manifest sweep matcher could not be built")]
    MatcherBuild,
}

impl SweepMatcher {
    /// Returns `None` when no source yields a pattern.
    pub(crate) fn build(
        sources: impl IntoIterator<Item = SweepSource>,
    ) -> Result<Option<Self>, ManifestSweepError> {
        let mut seen = HashSet::new();
        let sources = sources
            .into_iter()
            .filter(|source| seen.insert(source.clone()))
            .collect::<Vec<_>>();
        let mut patterns = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            source_patterns(index, source, &mut patterns);
            if patterns.len() > MAX_PATTERNS {
                return Err(ManifestSweepError::CapacityExceeded);
            }
        }
        if patterns.iter().map(|p| p.text.len()).sum::<usize>() > MAX_PATTERN_BYTES {
            return Err(ManifestSweepError::CapacityExceeded);
        }
        if patterns.is_empty() {
            return Ok(None);
        }
        let build = |kind: fn(&PatternKind) -> bool| {
            let (texts, meta): (Vec<_>, Vec<_>) = patterns
                .iter()
                .filter(|p| kind(&p.kind))
                .map(|p| (p.text.as_str(), (p.source, p.link)))
                .unzip();
            if texts.is_empty() {
                return Ok(None);
            }
            AhoCorasick::builder()
                .match_kind(MatchKind::Standard)
                .build(texts)
                .map(|matcher| Some((matcher, meta)))
                .map_err(|_| ManifestSweepError::MatcherBuild)
        };
        Ok(Some(Self {
            folded: build(|kind| matches!(kind, PatternKind::Folded))?,
            exact: build(|kind| matches!(kind, PatternKind::Exact))?,
            sources,
        }))
    }

    /// Every candidate copy in `text` (normalized input), word-bounded and
    /// outside URL-shaped runs. Hits may overlap; [`select`] picks the cover.
    pub(crate) fn find(&self, text: &str) -> Vec<SweepHit> {
        let mut hits = Vec::new();
        if let Some((matcher, meta)) = &self.folded {
            let folded = fold(text);
            for found in matcher.find_overlapping_iter(&folded.text) {
                let (start, end) = (found.start(), found.end());
                if !folded.first[start] || !folded.last[end - 1] {
                    continue;
                }
                let span = folded.raw_start[start]..folded.raw_end[end - 1];
                let (source, link) = meta[found.pattern().as_usize()];
                self.push_hit(text, span, source, link, &mut hits);
            }
        }
        if let Some((matcher, meta)) = &self.exact {
            for found in matcher.find_overlapping_iter(text) {
                let (source, link) = meta[found.pattern().as_usize()];
                self.push_hit(text, found.range(), source, link, &mut hits);
            }
        }
        hits
    }

    fn push_hit(
        &self,
        text: &str,
        span: Range<usize>,
        source: usize,
        link: SweepLink,
        hits: &mut Vec<SweepHit>,
    ) {
        if span.is_empty() || is_inside_word(text, span.start) || is_inside_word(text, span.end) {
            return;
        }
        let source = &self.sources[source];
        // A name copy glued to more name (`Vasquez-Ellery`, `O'Brien`) covers
        // the whole compound; stopping at the source's edge ships the rest raw.
        let span = if source.class == PiiClass::Name {
            extend_over_name_joiners(text, span)
        } else {
            span
        };
        if inside_url(text, &span) {
            return;
        }
        let link = match link {
            SweepLink::Part => SweepLink::Part,
            _ if normalize(&source.raw).text == text[span.clone()] => SweepLink::Exact,
            _ => SweepLink::Variant,
        };
        hits.push(SweepHit {
            span,
            family: source.family.clone(),
            class: source.class.clone(),
            link,
        });
    }
}

/// Leftmost-longest non-overlapping cover of `hits`.
pub(crate) fn select(mut hits: Vec<SweepHit>) -> Vec<SweepHit> {
    hits.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| b.span.end.cmp(&a.span.end))
    });
    let mut out: Vec<SweepHit> = Vec::with_capacity(hits.len());
    for hit in hits {
        if out
            .last()
            .is_none_or(|last| last.span.end <= hit.span.start)
        {
            out.push(hit);
        }
    }
    out
}

/// The shape of a value decides how it is swept and its precision floor.
/// One minimum for every shape let a four-digit postcode lose the anchor it
/// was found with; each variant now owns its floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueShape {
    /// A collision-family value: byte-exact, at least [`MIN_VALUE_CHARS`].
    Family,
    /// Digits and separators only: folded, at least [`MIN_DIGIT_RUN`] digits.
    DigitRun,
    /// One alphabetic word: its spellings, at least [`MIN_VALUE_CHARS`].
    Word,
    /// Anything else: folded, at least [`MIN_VALUE_CHARS`] non-space
    /// characters, plus title-case parts of a `Name`.
    MultiWord,
}

impl ValueShape {
    fn of(class: &PiiClass, value: &str) -> Self {
        if class.as_family_name().is_some() {
            Self::Family
        } else if value.chars().any(|ch| ch.is_ascii_digit())
            && value
                .chars()
                .all(|ch| ch.is_ascii_digit() || ch.is_whitespace() || "-./".contains(ch))
        {
            Self::DigitRun
        } else if is_word(value) {
            Self::Word
        } else {
            Self::MultiWord
        }
    }

    fn clears_floor(self, value: &str) -> bool {
        match self {
            Self::DigitRun => value.chars().filter(char::is_ascii_digit).count() >= MIN_DIGIT_RUN,
            Self::Family | Self::Word | Self::MultiWord => {
                value.chars().filter(|ch| !ch.is_whitespace()).count() >= MIN_VALUE_CHARS
            }
        }
    }
}

fn source_patterns(index: usize, source: &SweepSource, patterns: &mut Vec<Pattern>) {
    let normalized = normalize(&source.raw).text;
    let value = normalized.trim();
    let shape = ValueShape::of(&source.class, value);
    let mut push = |kind, text: String, link| {
        patterns.push(Pattern {
            kind,
            text,
            source: index,
            link,
        })
    };
    if shape.clears_floor(value) {
        match shape {
            ValueShape::Family => push(PatternKind::Exact, value.to_string(), SweepLink::Exact),
            ValueShape::Word => {
                for spelling in word_spellings(value) {
                    push(PatternKind::Exact, spelling, SweepLink::Exact);
                }
            }
            ValueShape::DigitRun | ValueShape::MultiWord => {
                push(PatternKind::Folded, fold(value).text, SweepLink::Exact)
            }
        }
    }
    if shape == ValueShape::MultiWord && source.class == PiiClass::Name {
        let parts = value.split_whitespace().collect::<Vec<_>>();
        // An org-shaped sender (`DHL Paket`, `GitHub Support`) is tokenized
        // whole, but its parts are product and department words, so none of
        // them sweeps alone (review F2 on #691).
        let single_parts = if is_org_shaped(&parts) {
            &[][..]
        } else {
            &parts[..]
        };
        for part in single_parts
            .iter()
            .filter(|part| is_word(part) && is_distinctive_part(part))
        {
            // A part is swept only in a spelling that starts upper-case:
            // a lower-case single word is too often an ordinary word.
            for spelling in word_spellings(part)
                .into_iter()
                .filter(|spelling| spelling.chars().next().is_some_and(char::is_uppercase))
            {
                push(PatternKind::Exact, spelling, SweepLink::Part);
            }
        }
        // Two or more adjacent parts are a name in any case: a cue-found
        // value carries its honorific (`Herr Tobias Brenner`), so the bare
        // `tobias brenner` is a run of its parts, not a spelling of the value.
        // A run needs one distinctive part, so the `grace hall` of
        // `Grace Hall Kowalski` (a venue) is not a name.
        if parts.len() <= MAX_RUN_PARTS {
            for len in 2..parts.len() {
                for run in parts.windows(len) {
                    if run.iter().all(|part| is_word(part))
                        && run.iter().any(|part| is_distinctive_part(part))
                    {
                        push(
                            PatternKind::Folded,
                            fold(&run.join(" ")).text,
                            SweepLink::Part,
                        );
                    }
                }
            }
        }
    }
}

/// A name part that is a name on its own: long enough and not an ordinary
/// word. The one rule for single parts and for the distinctive part of a run.
fn is_distinctive_part(part: &str) -> bool {
    part.chars().filter(|ch| ch.is_alphabetic()).count() >= MIN_PART_LETTERS
        && !is_ordinary_word(part)
}

/// A common word that is also a name, a particle, or an org or role word
/// (in any hyphen piece, so `IT-Support` counts).
fn is_ordinary_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    COMMON_WORDS.contains(&lower.as_str())
        || PARTICLES.contains(&lower.as_str())
        || lower
            .split(['-', '\'', '’'])
            .any(|piece| ORG_ROLE_WORDS.contains(&piece))
}

/// A word with an upper-case letter right after a lower-case one (`GitHub`).
fn is_camel_case(word: &str) -> bool {
    word.chars()
        .zip(word.chars().skip(1))
        .any(|(before, after)| before.is_lowercase() && after.is_uppercase())
}

/// A word of two or more letters, all upper case (`DHL`, `TOBIAS`).
fn is_all_caps(word: &str) -> bool {
    word.chars().filter(|ch| ch.is_alphabetic()).count() >= 2
        && word
            .chars()
            .filter(|ch| ch.is_alphabetic())
            .all(char::is_uppercase)
}

/// A name value shaped like an organization: a CamelCase part, or all-caps
/// parts mixed with others (`DHL Paket`). A wholly all-caps value
/// (`MARIA KOWALSKI`) is a shouted personal name and keeps its parts.
fn is_org_shaped(parts: &[&str]) -> bool {
    let caps = parts.iter().filter(|part| is_all_caps(part)).count();
    parts.iter().any(|part| is_camel_case(part)) || (caps > 0 && caps < parts.len())
}

/// A single word: letters, joined by at most internal hyphens or apostrophes.
fn is_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .split(['-', '\'', '’'])
            .all(|piece| !piece.is_empty() && piece.chars().all(char::is_alphabetic))
}

/// The spellings a single word is swept in: the word as written (a
/// byte-identical copy carries the source's own evidence, so `SCHNEIDER`
/// sweeps `SCHNEIDER`), its title case (`MARIA` also sweeps `Maria`) and its
/// upper case (`Maria` also sweeps `MARIA` in a shouted subject line). A
/// CamelCase word (`GitHub`) is a brand spelling, so it gets no upper case.
/// Empty for an ordinary word ([`is_ordinary_word`]).
fn word_spellings(word: &str) -> Vec<String> {
    if is_ordinary_word(word) {
        return Vec::new();
    }
    let mut spellings = vec![word.to_string()];
    let upper = (!is_camel_case(word)).then(|| word.to_uppercase());
    for spelling in std::iter::once(title_case(word)).chain(upper) {
        if !spellings.contains(&spelling) {
            spellings.push(spelling);
        }
    }
    spellings
}

fn title_case(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut upper_next = true;
    for ch in word.chars() {
        if upper_next {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
        upper_next = matches!(ch, '-' | '\'' | '’');
    }
    out
}

/// A hit inside a whitespace-delimited run that looks like a URL is skipped:
/// cutting a link apart breaks the agent's tool calls, and URLs have their
/// own recognizer.
fn inside_url(text: &str, span: &Range<usize>) -> bool {
    let start = text[..span.start]
        .rfind(char::is_whitespace)
        .map_or(0, |at| {
            at + text[at..].chars().next().map_or(1, char::len_utf8)
        });
    let end = text[span.end..]
        .find(char::is_whitespace)
        .map_or(text.len(), |at| span.end + at);
    let run = &text[start..end];
    run.contains("://") || run.to_ascii_lowercase().starts_with("www.")
}

/// Case- and whitespace-folded text with a map from every folded byte back to
/// the input character it came from.
struct Folded {
    text: String,
    raw_start: Vec<usize>,
    raw_end: Vec<usize>,
    /// The byte starts a character's fold expansion.
    first: Vec<bool>,
    /// The byte ends a character's fold expansion.
    last: Vec<bool>,
}

fn fold(input: &str) -> Folded {
    let mut folded = Folded {
        text: String::with_capacity(input.len()),
        raw_start: Vec::with_capacity(input.len()),
        raw_end: Vec::with_capacity(input.len()),
        first: Vec::with_capacity(input.len()),
        last: Vec::with_capacity(input.len()),
    };
    let mut in_space = false;
    for (start, ch) in input.char_indices() {
        let end = start + ch.len_utf8();
        if ch.is_whitespace() {
            if in_space {
                // One folded space stands for the whole run.
                *folded.raw_end.last_mut().expect("space pushed") = end;
            } else {
                folded.text.push(' ');
                folded.raw_start.push(start);
                folded.raw_end.push(end);
                folded.first.push(true);
                folded.last.push(true);
                in_space = true;
            }
            continue;
        }
        in_space = false;
        let before = folded.text.len();
        folded.text.extend(ch.to_lowercase());
        let len = folded.text.len() - before;
        for offset in 0..len {
            folded.raw_start.push(start);
            folded.raw_end.push(end);
            folded.first.push(offset == 0);
            folded.last.push(offset + 1 == len);
        }
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> SweepSource {
        SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: raw.into(),
        }
    }

    fn found(sources: Vec<SweepSource>, text: &str) -> Vec<&str> {
        let matcher = SweepMatcher::build(sources).unwrap().unwrap();
        let hits = select(matcher.find(text));
        hits.iter().map(|hit| &text[hit.span.clone()]).collect()
    }

    #[test]
    fn folding_matches_case_and_whitespace_variants() {
        let text = "a maria  schneider b MARIA\nSCHNEIDER c Maria Schneider";
        assert_eq!(
            found(vec![name("Maria Schneider")], text),
            ["maria  schneider", "MARIA\nSCHNEIDER", "Maria Schneider"]
        );
    }

    #[test]
    fn folding_maps_length_changing_case_back_to_input_bytes() {
        // `İ` lowers to two scalars; the hit must still cover exactly the
        // input bytes of the copy, not drift by one. (Default Unicode case
        // mapping lowers `I` to `i`, so a dotless `ı` in the source value
        // does not fold to an upper-case `I` copy: stated, not handled.)
        let text = "x İPEK YILDIZ y";
        let hits = found(vec![name("İpek Yildiz")], text);
        assert_eq!(hits, ["İPEK YILDIZ"]);
        assert_eq!(
            found(vec![name("İpek Yildiz")], "İpek Yildiz!"),
            ["İpek Yildiz"]
        );
    }

    #[test]
    fn edges_inside_a_word_are_refused() {
        assert!(found(vec![name("Maria Schneider")], "xmaria schneidery").is_empty());
        // The whole value stops inside `Schneiders`; only the title-case
        // part `Maria` stands on word edges.
        assert_eq!(
            found(vec![name("Maria Schneider")], "Maria Schneiders"),
            ["Maria"]
        );
    }

    #[test]
    fn copies_inside_urls_are_refused() {
        let text = "see https://example.invalid/Maria/Schneider and www.Maria.invalid";
        assert!(found(vec![name("Maria Schneider")], text).is_empty());
    }

    #[test]
    fn parts_match_in_title_case_or_as_written_only() {
        let sources = vec![name("MARIA KOWALSKI")];
        assert_eq!(found(sources.clone(), "Thanks, Maria"), ["Maria"]);
        // Byte-identical to the source's own part: same evidence.
        assert_eq!(found(sources.clone(), "thanks MARIA"), ["MARIA"]);
        assert!(found(sources.clone(), "thanks maria").is_empty());
        // A lower-case source part never sweeps lower-case single words.
        assert!(found(vec![name("maria kowalski")], "thanks maria").is_empty());
        assert!(found(sources, "thanks mARIA").is_empty());
    }

    #[test]
    fn common_words_are_never_parts() {
        assert!(found(vec![name("Rose May")], "The Rose garden opens in May.").is_empty());
    }

    #[test]
    fn short_parts_and_values_are_skipped() {
        assert!(SweepMatcher::build(vec![name("Al")]).unwrap().is_none());
        assert!(found(vec![name("Al Bo")], "Al met Bo").is_empty());
    }

    #[test]
    fn family_values_match_byte_exact() {
        let family = SweepSource {
            family: "counter".into(),
            class: PiiClass::family("id"),
            raw: "AB12-CD34".into(),
        };
        assert_eq!(found(vec![family.clone()], "x AB12-CD34 y"), ["AB12-CD34"]);
        assert!(found(vec![family], "x ab12-cd34 y").is_empty());
    }

    #[test]
    fn hyphenated_values_fold_as_one() {
        let sources = vec![name("Jonas Albrecht-Quaye")];
        assert_eq!(
            found(sources.clone(), "regards, jonas albrecht-quaye"),
            ["jonas albrecht-quaye"]
        );
        assert_eq!(found(sources, "Dear Albrecht-Quaye,"), ["Albrecht-Quaye"]);
    }

    #[test]
    fn links_tell_exact_copies_from_variants() {
        let matcher = SweepMatcher::build(vec![name("Maria Schneider")])
            .unwrap()
            .unwrap();
        let hits = select(matcher.find("Maria Schneider, maria schneider, Maria"));
        let links = hits.iter().map(|hit| hit.link).collect::<Vec<_>>();
        assert_eq!(
            links,
            [SweepLink::Exact, SweepLink::Variant, SweepLink::Part]
        );
    }
    #[test]
    fn short_digit_runs_are_not_swept() {
        // A four-digit postcode found through its city anchor must not turn
        // every year or room number into a postcode.
        let postcode = SweepSource {
            family: "counter".into(),
            class: PiiClass::Custom("postal_code".into()),
            raw: "2024".into(),
        };
        assert!(SweepMatcher::build(vec![postcode]).unwrap().is_none());
        let long = SweepSource {
            family: "counter".into(),
            class: PiiClass::Custom("id".into()),
            raw: "123 456".into(),
        };
        assert_eq!(found(vec![long], "ref 123\n456 ok"), ["123\n456"]);
    }

    #[test]
    fn value_shapes_own_their_floors() {
        let custom = PiiClass::Custom("x".into());
        assert_eq!(ValueShape::of(&custom, "10115"), ValueShape::DigitRun);
        assert_eq!(
            ValueShape::of(&custom, "030 123-45.6"),
            ValueShape::DigitRun
        );
        assert_eq!(
            ValueShape::of(&custom, "BC-2024-789"),
            ValueShape::MultiWord
        );
        assert_eq!(
            ValueShape::of(&PiiClass::Name, "Schneider"),
            ValueShape::Word
        );
        assert_eq!(
            ValueShape::of(&PiiClass::family("id"), "2024"),
            ValueShape::Family
        );
        assert!(!ValueShape::DigitRun.clears_floor("10115"));
        assert!(ValueShape::DigitRun.clears_floor("101 150"));
    }

    #[test]
    fn all_caps_single_word_sweeps_its_byte_exact_copy() {
        let source = SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: "KOWALSKI".into(),
        };
        assert_eq!(found(vec![source.clone()], "x KOWALSKI y"), ["KOWALSKI"]);
        assert_eq!(found(vec![source], "x Kowalski y"), ["Kowalski"]);
    }

    #[test]
    fn occupational_and_verb_surnames_are_never_parts() {
        let sources = vec![
            name("Thomas Richter"),
            name("Hugh Grant"),
            name("Anna Bauer"),
        ];
        assert!(found(sources.clone(), "Der Richter hat entschieden.").is_empty());
        assert!(found(sources.clone(), "Grant access to the repo.").is_empty());
        assert!(found(sources, "Der Bauer verkaufte Eier.").is_empty());
    }

    #[test]
    fn runs_of_name_parts_match_in_any_case() {
        // Solo todo 3897: a cue-found value carries its honorific, so the bare
        // name is a run of its parts, not a spelling of the whole value.
        let sources = vec![name("Herr Tobias Brenner")];
        let text = "later: tobias brenner and TOBIAS BRENNER, tObIaS\u{a0}bReNnEr";
        assert_eq!(
            found(sources.clone(), text),
            ["tobias brenner", "TOBIAS BRENNER", "tObIaS\u{a0}bReNnEr"]
        );
        assert_eq!(found(sources, "herr tobias kam"), ["herr tobias"]);
    }

    #[test]
    fn runs_need_a_distinctive_part() {
        // `grace` and `hall` are common words, so the adjacent run of
        // `Grace Hall Kowalski` is a venue, not a name.
        assert!(found(vec![name("Grace Hall Kowalski")], "book the grace hall").is_empty());
        // Runs are made of whole parts; a lone lower-case part stays raw.
        assert!(found(vec![name("Herr Tobias Brenner")], "thanks tobias").is_empty());
    }

    #[test]
    fn parts_match_in_all_caps() {
        let sources = vec![name("Maria Kowalski")];
        assert_eq!(found(sources.clone(), "RE: KOWALSKI"), ["KOWALSKI"]);
        assert_eq!(found(sources.clone(), "HI MARIA"), ["MARIA"]);
        assert!(found(sources, "thanks mARIA").is_empty());
        let single = SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: "Kowalski".into(),
        };
        assert_eq!(found(vec![single], "x KOWALSKI y"), ["KOWALSKI"]);
    }

    #[test]
    fn name_copies_cover_glued_hyphen_and_apostrophe_continuations() {
        let sources = vec![name("Jorunn Vasquez")];
        assert_eq!(
            found(sources.clone(), "ping jorunn vasquez-ellery now"),
            ["jorunn vasquez-ellery"]
        );
        assert_eq!(
            found(sources.clone(), "Dear Vasquez-Ellery,"),
            ["Vasquez-Ellery"]
        );
        assert_eq!(
            found(sources.clone(), "Mrs Ellery-Vasquez"),
            ["Ellery-Vasquez"]
        );
        assert_eq!(found(sources.clone(), "O'Vasquez said"), ["O'Vasquez"]);
        // A possessive `'s` is not more name.
        assert_eq!(found(sources.clone(), "Vasquez's desk"), ["Vasquez"]);
        assert_eq!(found(sources, "Vasquez’s desk"), ["Vasquez"]);
        // Other classes keep their exact edges.
        let id = SweepSource {
            family: "counter".into(),
            class: PiiClass::Custom("id".into()),
            raw: "AB12CD".into(),
        };
        assert_eq!(found(vec![id], "x AB12CD-extra y"), ["AB12CD"]);
    }

    /// Review F1 (#691): particles and articles are never name parts on
    /// their own, so `von der` is not swept from `Anna von der Heide`.
    #[test]
    fn particles_are_never_distinctive_or_parts() {
        assert!(found(
            vec![name("Anna von der Heide")],
            "Unterlagen von der Bank und von der Post holen, VON DER KASSE."
        )
        .is_empty());
        assert!(found(vec![name("Jan van der Berg")], "The van der Waals force.").is_empty());
        assert!(found(vec![name("Paul van den Broek")], "we gaan van den bosch").is_empty());
        let leyen = vec![name("Ursula von der Leyen")];
        assert!(found(leyen.clone(), "DER VERTRAG VON HEUTE").is_empty());
        assert!(found(leyen.clone(), "Der Vertrag von heute").is_empty());
        // The distinctive part still carries every run and its own spellings.
        assert_eq!(found(leyen.clone(), "an VON DER LEYEN"), ["VON DER LEYEN"]);
        assert_eq!(found(leyen, "frau von der leyen"), ["von der leyen"]);
    }

    #[test]
    fn ben_stays_a_given_name() {
        assert_eq!(found(vec![name("Ben Kowalski")], "Thanks, Ben"), ["Ben"]);
        assert_eq!(found(vec![name("Nic Kowalski")], "Thanks, Nic"), ["Nic"]);
    }

    /// Re-verify R1 (#691): the particle rule covers the class, not the
    /// listed instances. Every particle and every particle pair from a
    /// sender `Maria P Q Kowalski` must stay raw in ordinary text, in lower,
    /// title and upper case.
    #[test]
    fn every_particle_and_particle_pair_stays_raw() {
        // The reference set lives here, not in `PARTICLES`, so a particle
        // dropped from the production list fails this test.
        #[rustfmt::skip]
        let words = [
            "von", "vom", "zu", "zum", "zur", "der", "den", "dem", "des", "van",
            "de", "het", "ten", "ter", "te", "op", "uit", "aan", "du", "la",
            "le", "les", "del", "las", "los", "y", "da", "das", "do", "dos", "e",
            "di", "dal", "dalla", "dei", "degli", "della", "delle", "dello", "af",
            "av", "al", "el", "bin", "ibn", "bint", "abu", "bar", "mac",
            "ap",
        ];
        let mut swept = Vec::new();
        for first in &words {
            for second in &words {
                let matcher = SweepMatcher::build(vec![
                    name(&format!("Maria {first} {second} Kowalski")),
                    name(&format!("Maria {first} Kowalski")),
                ])
                .unwrap()
                .unwrap();
                let lower = format!("go {first} {second} home");
                for text in [
                    lower.clone(),
                    title_case_words(&lower),
                    lower.to_uppercase(),
                ] {
                    for hit in matcher.find(&text) {
                        swept.push(text[hit.span].to_string());
                    }
                }
            }
        }
        assert!(swept.is_empty(), "particles swept: {swept:?}");
    }

    fn title_case_words(text: &str) -> String {
        text.split(' ')
            .map(title_case)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn spanish_and_dutch_particle_runs_stay_raw() {
        assert!(found(
            vec![name("Carmen de los Rios")],
            "Vamos de los Angeles, DE LOS ANDES"
        )
        .is_empty());
        assert!(found(
            vec![name("Gerrit op het Veld")],
            "Zet het op het bord. OP HET PLEIN."
        )
        .is_empty());
        assert_eq!(
            found(vec![name("Gerrit op het Veld")], "met gerrit op het veld"),
            ["gerrit op het veld"]
        );
    }

    /// Review F2 (#691): an org-shaped sender (a CamelCase part, or all-caps
    /// mixed with other parts) and org or role words never sweep as single
    /// parts; the whole value still does.
    #[test]
    fn org_shaped_values_do_not_propagate_single_parts() {
        let dhl = vec![name("DHL Paket")];
        assert!(found(dhl.clone(), "Ihr Paket kommt. Das PAKET ist da").is_empty());
        assert_eq!(found(dhl, "Ihr DHL Paket kommt"), ["DHL Paket"]);
        assert!(found(
            vec![name("DHL Paket Service")],
            "SERVICE desk, Service desk"
        )
        .is_empty());
        // Only the shape rule stops these: `Express` and `Enterprise` are not
        // org words.
        assert!(found(vec![name("DHL Express")], "Express delivery, EXPRESS lane").is_empty());
        assert!(found(vec![name("GitHub Enterprise")], "Enterprise plan").is_empty());
        let team = vec![name("SUPPORT TEAM")];
        assert!(found(team.clone(), "Contact Support or the Team").is_empty());
        assert_eq!(found(team, "ask the support team"), ["support team"]);
        let hub = SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: "GitHub".into(),
        };
        assert!(found(vec![hub.clone()], "GITHUB outage").is_empty());
        assert_eq!(found(vec![hub], "GitHub outage"), ["GitHub"]);
        // A fully all-caps personal name keeps its parts (main already swept them).
        let caps = vec![name("JORUNN VASQUEZ-ELLERY")];
        assert_eq!(
            found(caps.clone(), "Dear Vasquez-Ellery,"),
            ["Vasquez-Ellery"]
        );
        assert_eq!(
            found(caps, "ping jorunn vasquez-ellery"),
            ["jorunn vasquez-ellery"]
        );
    }

    /// Review F3 (#691), accepted and disclosed: joiner growth also takes a
    /// hyphenated common word glued to a name. Fail-closed, few bytes.
    #[test]
    fn joiner_growth_takes_a_glued_hyphenated_word() {
        assert_eq!(
            found(vec![name("Maria Kowalski")], "see Kowalski-follow-up notes"),
            ["Kowalski-follow-up"]
        );
    }

    #[test]
    fn pattern_count_cap_fails_closed() {
        let sources = (0..=MAX_PATTERNS).map(|i| SweepSource {
            family: "counter".into(),
            class: PiiClass::Custom("id".into()),
            raw: format!("ID-{i:08}"),
        });
        assert_eq!(
            SweepMatcher::build(sources).err(),
            Some(ManifestSweepError::CapacityExceeded)
        );
    }

    #[test]
    fn byte_cap_fails_the_request_closed() {
        use crate::{Action, DefaultRule, Pipeline, RawDocument, Scope, Session};
        let session = Session::new(Scope::Ephemeral).unwrap();
        let huge = format!("Maria {}", "x".repeat(MAX_PATTERN_BYTES + 1));
        session.tokenize(&PiiClass::Name, &huge).unwrap();
        session.record_evidence(None, &PiiClass::Name, &huge, ManifestEvidence::Pattern);
        let pipeline = Pipeline::builder()
            .rule(DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        let result = pipeline.redact(&session, RawDocument::Text("hello Maria".into()));
        assert!(
            matches!(
                result,
                Err(crate::Error::ManifestSweep(
                    ManifestSweepError::CapacityExceeded
                ))
            ),
            "no text may ship when the sweep cannot run: {result:?}"
        );
    }
}
