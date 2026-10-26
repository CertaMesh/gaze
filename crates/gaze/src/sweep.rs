//! Repeat-value sweep.
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

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use aho_corasick::{AhoCorasick, AhoCorasickKind, MatchKind};
use gaze_types::{
    extend_over_name_joiners, is_inside_word, Candidate, ConflictTier, EvidenceKind, PiiClass,
};
use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// Recognizer id, source label and audit stage of a swept copy.
pub(crate) const SWEEP_ID: &str = "manifest_sweep";

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
        } else if candidate.validator_fail_reason.is_some() {
            // Kept although its checksum failed (`ValidatorOnFail::Record`): never swept,
            // whatever evidence it carries.
            Self::Learned
        } else if candidate.checksum_validated() {
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
    pub(crate) repeat_seed: Option<RepeatSeed>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct RepeatSeed {
    pub(crate) raw: String,
    pub(crate) kind: gaze_types::LabelledValueRepeatSeedKind,
}

impl SweepSource {
    fn field_owned_scalar(&self) -> bool {
        self.repeat_seed
            .as_ref()
            .is_some_and(|seed| seed.kind == gaze_types::LabelledValueRepeatSeedKind::ExactScalar)
    }
}

// Same byte ceiling as the field-owned scanner. Ownership remains unbounded;
// only the derived repeat matcher is limited.
pub(crate) const MAX_LABELLED_REPEAT_BYTES: usize = 40;
const MAX_ESCAPED_REPEAT_DEPTH: usize = 32;

/// How a swept copy relates to the value it was found from. Audit rows carry
/// this instead of the source token: the audit contract never holds tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SweepLink {
    /// Byte-identical after normalization: the copy reuses the source token.
    Exact,
    /// A different spelling (case, whitespace): the copy gets a sibling token.
    Variant,
    /// A part, or a run of parts, of a multi-word name.
    Part,
    /// A conservative prefix of a field-owned value, not its complete manifest raw.
    UncertainPrefix,
}

impl SweepLink {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "manifest_sweep:exact",
            Self::Variant => "manifest_sweep:variant",
            Self::Part => "manifest_sweep:part",
            Self::UncertainPrefix => "manifest_sweep:uncertain_prefix",
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
        if self.link == SweepLink::UncertainPrefix {
            candidate.labelled_value_scan_reason =
                Some(gaze_types::LabelledValueScanReason::UncertainRepeatPrefix);
        }
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
    derived_exact: Option<DerivedExactMatcher>,
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

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug)]
struct SweepWork {
    source_bytes: usize,
    pattern_bytes: usize,
    max_pattern_bytes: usize,
    matcher_bytes: usize,
    url_scan_bytes: usize,
    url_queries: usize,
}

#[cfg(test)]
thread_local! {
    static SWEEP_WORK: std::cell::Cell<SweepWork> = const { std::cell::Cell::new(SweepWork {
        source_bytes: 0, pattern_bytes: 0, max_pattern_bytes: 0, matcher_bytes: 0,
        url_scan_bytes: 0, url_queries: 0,
    }) };
}

impl SweepMatcher {
    /// Returns `None` when no source yields a pattern.
    pub(crate) fn build(
        sources: impl IntoIterator<Item = SweepSource>,
    ) -> Result<Option<Self>, ManifestSweepError> {
        let mut seen = HashSet::new();
        let sources = sources
            .into_iter()
            .inspect(|_source| {
                #[cfg(test)]
                SWEEP_WORK.with(|work| {
                    let mut count = work.get();
                    count.source_bytes += _source.raw.len()
                        + _source
                            .repeat_seed
                            .as_ref()
                            .map_or(0, |seed| seed.raw.len());
                    work.set(count);
                });
            })
            .filter(|source| seen.insert(source.clone()))
            .collect::<Vec<_>>();
        let mut patterns = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            source_patterns(index, source, &mut patterns);
            if patterns.len() > MAX_PATTERNS {
                return Err(ManifestSweepError::CapacityExceeded);
            }
        }
        let pattern_bytes = patterns.iter().map(|p| p.text.len()).sum::<usize>();
        if pattern_bytes > MAX_PATTERN_BYTES {
            return Err(ManifestSweepError::CapacityExceeded);
        }
        let derived_exact = DerivedExactMatcher::build(
            &sources,
            MAX_PATTERNS - patterns.len(),
            MAX_PATTERN_BYTES - pattern_bytes,
        )?;
        if patterns.is_empty() && derived_exact.is_none() {
            return Ok(None);
        }
        #[cfg(test)]
        SWEEP_WORK.with(|work| {
            let mut count = work.get();
            count.pattern_bytes += patterns.iter().map(|p| p.text.len()).sum::<usize>();
            count.max_pattern_bytes = count
                .max_pattern_bytes
                .max(patterns.iter().map(|p| p.text.len()).max().unwrap_or(0));
            work.set(count);
        });
        let build = |kind: fn(&PatternKind) -> bool| {
            let (texts, meta): (Vec<_>, Vec<_>) = patterns
                .iter()
                .filter(|p| kind(&p.kind))
                .map(|p| (p.text.as_str(), (p.source, p.link)))
                .unzip();
            if texts.is_empty() {
                return Ok(None);
            }
            let long_pattern = texts
                .iter()
                .any(|text| text.len() > MAX_LABELLED_REPEAT_BYTES);
            AhoCorasick::builder()
                .kind(long_pattern.then_some(AhoCorasickKind::ContiguousNFA))
                .match_kind(MatchKind::Standard)
                .build(texts)
                .map(|matcher| {
                    #[cfg(test)]
                    SWEEP_WORK.with(|work| {
                        let mut count = work.get();
                        count.matcher_bytes += matcher.memory_usage();
                        work.set(count);
                    });
                    Some((matcher, meta))
                })
                .map_err(|_| ManifestSweepError::MatcherBuild)
        };
        Ok(Some(Self {
            derived_exact,
            folded: build(|kind| matches!(kind, PatternKind::Folded))?,
            exact: build(|kind| matches!(kind, PatternKind::Exact))?,
            sources,
        }))
    }

    /// Every candidate copy in `text` (normalized input), word-bounded and
    /// outside URL-shaped runs. Hits may overlap; [`select`] picks the cover.
    pub(crate) fn find(&self, text: &str) -> Vec<SweepHit> {
        let mut hits = Vec::new();
        let json = JsonSweepIndex::new(text);
        if let Some((matcher, meta)) = &self.folded {
            let folded = fold(text);
            for found in matcher.find_overlapping_iter(&folded.text) {
                let (start, end) = (found.start(), found.end());
                if !folded.first[start] || !folded.last[end - 1] {
                    continue;
                }
                let span = folded.raw_start[start]..folded.raw_end[end - 1];
                let (source, link) = meta[found.pattern().as_usize()];
                self.push_hit(text, span, source, link, &json, &mut hits);
            }
        }
        if let Some((matcher, meta)) = &self.exact {
            for found in matcher.find_overlapping_iter(text) {
                let (source, link) = meta[found.pattern().as_usize()];
                self.push_hit(text, found.range(), source, link, &json, &mut hits);
            }
        }
        if let Some(derived) = &self.derived_exact {
            let mut view = EscapedView::identity(text);
            for _ in 0..=MAX_ESCAPED_REPEAT_DEPTH {
                for found in derived.matcher.find_overlapping_iter(&view.text) {
                    let span = view.source[found.start()].start..view.source[found.end() - 1].end;
                    if let Some(sources) = derived.sources.get(&view.text[found.range()]) {
                        for source in sources {
                            self.push_hit(
                                text,
                                span.clone(),
                                *source,
                                SweepLink::Exact,
                                &json,
                                &mut hits,
                            );
                        }
                    }
                }
                let Some(decoded) = view.decode() else {
                    break;
                };
                view = decoded;
            }
        }
        let mut seen = HashSet::new();
        hits.retain(|hit| {
            seen.insert((
                hit.span.start,
                hit.span.end,
                hit.family.clone(),
                hit.class.clone(),
                hit.link,
            ))
        });
        hits
    }

    fn push_hit(
        &self,
        text: &str,
        span: Range<usize>,
        source: usize,
        link: SweepLink,
        json: &JsonSweepIndex,
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
        if !json.allows(&span) || json.urls.contains(text, &span) {
            return;
        }
        let link = match link {
            SweepLink::Part => SweepLink::Part,
            SweepLink::UncertainPrefix => SweepLink::UncertainPrefix,
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

/// Deep source spelling may be long even though its decoded identifier is short.
/// Match a bounded decoded body and project each spelling to verified source bytes.
struct DerivedExactMatcher {
    matcher: AhoCorasick,
    sources: HashMap<String, Vec<usize>>,
}

impl DerivedExactMatcher {
    fn build(
        sources: &[SweepSource],
        remaining_patterns: usize,
        remaining_bytes: usize,
    ) -> Result<Option<Self>, ManifestSweepError> {
        let mut patterns = HashSet::new();
        let mut decoded_sources: HashMap<String, HashMap<(String, PiiClass), usize>> =
            HashMap::new();
        for (index, source) in sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.field_owned_scalar())
        {
            let raw = normalize(&source.raw).text;
            let mut view = EscapedView::identity(&raw);
            let mut depth = 0;
            while let Some(decoded) = view.decode() {
                if depth >= MAX_ESCAPED_REPEAT_DEPTH {
                    return Err(ManifestSweepError::CapacityExceeded);
                }
                depth += 1;
                view = decoded;
            }
            let start = view.text.len() - view.text.trim_start().len();
            let end = view.text.trim_end().len();
            for span in [0..view.text.len(), start..end] {
                if span.is_empty() {
                    continue;
                }
                let body = &view.text[span.clone()];
                if body.len() <= MAX_LABELLED_REPEAT_BYTES
                    && labelled_repeat_has_digit(body)
                    && source.class.clears_repeat_floor(body.trim())
                {
                    patterns.insert(body.to_owned());
                    decoded_sources
                        .entry(body.to_owned())
                        .or_default()
                        .entry((source.family.clone(), source.class.clone()))
                        .or_insert(index);
                }
            }
        }
        if patterns.is_empty() {
            return Ok(None);
        }
        if patterns.len() > remaining_patterns
            || patterns.iter().map(String::len).sum::<usize>() > remaining_bytes
        {
            return Err(ManifestSweepError::CapacityExceeded);
        }
        let mut patterns = patterns.into_iter().collect::<Vec<_>>();
        patterns.sort_unstable();
        let matcher = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(&patterns)
            .map_err(|_| ManifestSweepError::MatcherBuild)?;
        #[cfg(test)]
        SWEEP_WORK.with(|work| {
            let mut count = work.get();
            count.pattern_bytes += patterns.iter().map(String::len).sum::<usize>();
            count.max_pattern_bytes = count
                .max_pattern_bytes
                .max(patterns.iter().map(String::len).max().unwrap_or(0));
            count.matcher_bytes += matcher.memory_usage();
            work.set(count);
        });
        Ok(Some(Self {
            matcher,
            sources: decoded_sources
                .into_iter()
                .map(|(raw, owners)| {
                    let mut indexes = owners.into_values().collect::<Vec<_>>();
                    indexes.sort_unstable();
                    (raw, indexes)
                })
                .collect(),
        }))
    }
}

struct EscapedView {
    text: String,
    source: Vec<Range<usize>>,
}

impl EscapedView {
    fn identity(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            source: (0..text.len()).map(|at| at..at + 1).collect(),
        }
    }

    fn decode(&self) -> Option<Self> {
        let (text, projection) = gaze_types::json_regions::unescape_json_fragments(&self.text)?;
        let source = projection
            .into_iter()
            .map(|span| self.source[span.start].start..self.source[span.end - 1].end)
            .collect();
        Some(Self { text, source })
    }
}

/// Valid JSON permits a repeat only inside a string value. Index decoded
/// string layers too, so serialized tool schemas retain their keys and syntax.
struct JsonSweepIndex {
    urls: UrlRuns,
    markup: gaze_types::markup_regions::MarkupRegions,
    lexical_atoms: Vec<Range<usize>>,
    forbidden: Vec<Range<usize>>,
    escape_interiors: Vec<Range<usize>>,
}

impl JsonSweepIndex {
    fn new(text: &str) -> Self {
        let mut forbidden = Vec::new();
        let mut escape_interiors = Vec::new();
        let mut lexical_atoms = Vec::new();
        let mut view = EscapedView::identity(text);
        for _ in 0..=MAX_ESCAPED_REPEAT_DEPTH {
            collect_lexical_sweep_blocks(&view.text, &view.source, &mut lexical_atoms);
            Self::collect(
                &view.text,
                &view.source,
                &mut forbidden,
                &mut escape_interiors,
            );
            let Some(decoded) = view.decode() else {
                break;
            };
            view = decoded;
        }
        Self {
            urls: UrlRuns::new(text),
            markup: gaze_types::markup_regions::MarkupRegions::new(text),
            lexical_atoms: merge_json_ranges(lexical_atoms),
            forbidden: merge_json_ranges(forbidden),
            escape_interiors: merge_json_ranges(escape_interiors),
        }
    }

    fn collect(
        text: &str,
        source: &[Range<usize>],
        forbidden: &mut Vec<Range<usize>>,
        escape_interiors: &mut Vec<Range<usize>>,
    ) {
        let project = |start: usize, end: usize| source[start].start..source[end - 1].end;
        let regions = gaze_types::json_regions::json_regions(text);
        let mut strings = gaze_types::json_regions::json_strings(text, &regions);
        let trimmed = text.trim();
        let start = text.len() - text.trim_start().len();
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(serde_json::Value::String(_)) => {
                strings.push((start + 1..start + trimmed.len() - 1, false))
            }
            Ok(
                serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_),
            ) => forbidden.push(project(start, start + trimmed.len())),
            _ => {}
        }
        for region in &regions {
            let mut start = region.start;
            let first = strings.partition_point(|(span, _)| span.end <= region.start);
            for (span, key) in strings[first..]
                .iter()
                .take_while(|(span, _)| span.start < region.end)
            {
                if !key {
                    if start < span.start {
                        forbidden.push(project(start, span.start));
                    }
                    start = span.end;
                }
            }
            if start < region.end {
                forbidden.push(project(start, region.end));
            }
        }
        for (span, key) in &strings {
            if *key {
                continue;
            }
            let mut at = span.start;
            while at < span.end {
                if text.as_bytes()[at] == b'\\' {
                    let end = json_escape_end(text, at);
                    let mapped = project(at, end);
                    escape_interiors.push(mapped.start + 1..mapped.end);
                    at = end;
                } else {
                    at += text[at..].chars().next().expect("character").len_utf8();
                }
            }
        }
    }

    fn allows(&self, span: &Range<usize>) -> bool {
        if self.markup.blocks_repeat(span) {
            return false;
        }
        let at = self
            .lexical_atoms
            .partition_point(|atom| atom.end <= span.start);
        if self
            .lexical_atoms
            .get(at)
            .is_some_and(|atom| atom.start <= span.start && span.end <= atom.end)
        {
            return false;
        }
        let splits_escape = [span.start, span.end].iter().any(|at| {
            let index = self
                .escape_interiors
                .partition_point(|escape| escape.end <= *at);
            self.escape_interiors
                .get(index)
                .is_some_and(|escape| escape.contains(at))
        });
        if splits_escape {
            return false;
        }
        let at = self
            .forbidden
            .partition_point(|blocked| blocked.end <= span.start);
        self.forbidden
            .get(at)
            .is_none_or(|blocked| blocked.start >= span.end)
    }
}

// Lexical protection is independent of whether the surrounding container parses.
// Quotes and comments delimit content; braces remain active through truncation.
fn collect_lexical_sweep_blocks(
    text: &str,
    source: &[Range<usize>],
    forbidden: &mut Vec<Range<usize>>,
) {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut depth = 0usize;
    let mut block = |start: usize, end: usize| {
        forbidden.push(source[start].start..source[end - 1].end);
    };
    while at < bytes.len() {
        match bytes[at] {
            b'"' | b'\'' => {
                let start = at;
                let quote = bytes[at];
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == b'\\' {
                        at = (at + 2).min(bytes.len());
                    } else if bytes[at] == quote {
                        at += 1;
                        break;
                    } else {
                        at += 1;
                    }
                }
                let end = at;
                while let Some(ch) = text[at..].chars().next().filter(|ch| ch.is_whitespace()) {
                    at += ch.len_utf8();
                }
                if bytes.get(at) == Some(&b':') {
                    block(start, end);
                } else if depth == 0 {
                    // A stray prose quote cannot hide a later JSON-like container.
                    at = start + 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at += 2;
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                    at += 1;
                }
                at = (at + 2).min(bytes.len());
            }
            b'{' | b'[' => {
                depth += 1;
                at += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                at += 1;
            }
            byte if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'+' | b'_') => {
                let start = at;
                at += 1;
                while at < bytes.len()
                    && (bytes[at].is_ascii_alphanumeric() || b"_.+-".contains(&bytes[at]))
                {
                    at += 1;
                }
                let word = &text[start..at];
                let number = depth > 0
                    && word.bytes().any(|b| b.is_ascii_digit())
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b".eE+-".contains(&b));
                if number || matches!(word, "null" | "true" | "false") {
                    block(start, at);
                }
            }
            _ => {
                at += 1;
            }
        }
    }
}

fn merge_json_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_unstable_by_key(|span| span.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for span in ranges {
        if let Some(last) = merged.last_mut().filter(|last| last.end > span.start) {
            last.end = last.end.max(span.end);
        } else {
            merged.push(span);
        }
    }
    merged
}

fn json_escape_end(text: &str, at: usize) -> usize {
    if text.as_bytes()[at + 1] != b'u' {
        return at + 2;
    }
    let end = at + 6;
    let first = u16::from_str_radix(&text[at + 2..end], 16).expect("validated JSON escape");
    if (0xd800..=0xdbff).contains(&first) {
        end + 6
    } else {
        end
    }
}

fn labelled_repeat_has_digit(value: &str) -> bool {
    serde_json::from_str::<String>(&format!("\"{value}\""))
        .unwrap_or_else(|_| value.to_owned())
        .chars()
        .any(|ch| ch.is_ascii_digit())
}

fn trim_labelled_repeat(mut value: &str) -> &str {
    loop {
        value = value.trim();
        let before = value.len();
        if let Some(len) = gaze_types::json_regions::json_whitespace_escape_len(value) {
            value = &value[len..];
        }
        for len in [2, 6] {
            if value
                .get(value.len().saturating_sub(len)..)
                .is_some_and(|tail| {
                    gaze_types::json_regions::json_whitespace_escape_len(tail) == Some(len)
                })
            {
                value = &value[..value.len() - len];
            }
        }
        if before == value.len() {
            return value;
        }
    }
}

/// Winner spans, answering "does one span contain this range" in O(log n).
///
/// Asking every winner for every sweep hit made a document that repeats one
/// value N times cost O(N^2). A span containing `range` starts
/// at or before `range.start`, so it exists exactly when the largest end
/// among those spans reaches `range.end`.
pub(crate) struct Coverage {
    starts: Vec<usize>,
    /// `max_end[i]` is the largest end among the first `i + 1` spans.
    max_end: Vec<usize>,
}

impl Coverage {
    pub(crate) fn new(spans: impl Iterator<Item = Range<usize>>) -> Self {
        let mut spans = spans.collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start);
        let starts = spans.iter().map(|span| span.start).collect();
        let max_end = spans
            .iter()
            .scan(0, |max, span| {
                *max = span.end.max(*max);
                Some(*max)
            })
            .collect();
        Self { starts, max_end }
    }

    pub(crate) fn contains(&self, range: &Range<usize>) -> bool {
        let before = self.starts.partition_point(|&start| start <= range.start);
        before > 0 && self.max_end[before - 1] >= range.end
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
    /// A collision-family value: byte-exact.
    Family,
    /// Digits and separators only: folded.
    DigitRun,
    /// One alphabetic word: its spellings.
    Word,
    /// Anything else: folded, plus title-case parts of a `Name`.
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
}

fn source_patterns(index: usize, source: &SweepSource, patterns: &mut Vec<Pattern>) {
    let normalized = normalize(&source.raw).text;
    let value = if source.field_owned_scalar() {
        trim_labelled_repeat(&normalized)
    } else {
        normalized.trim()
    };
    if source.field_owned_scalar() && !labelled_repeat_has_digit(value) {
        return;
    }
    let shape = ValueShape::of(&source.class, value);
    let mut push = |kind, text: String, link| {
        if !source.field_owned_scalar() || text.len() <= MAX_LABELLED_REPEAT_BYTES {
            patterns.push(Pattern {
                kind,
                text,
                source: index,
                link,
            });
        }
    };
    if source.class.clears_repeat_floor(value) {
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
    if let Some(seed) = &source.repeat_seed {
        let raw = normalize(&seed.raw).text;
        let body = trim_labelled_repeat(&raw);
        if source.class.clears_repeat_floor(body) {
            let uncertain = matches!(
                seed.kind,
                gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix
            );
            let link = if uncertain {
                SweepLink::UncertainPrefix
            } else {
                SweepLink::Exact
            };
            if uncertain {
                push(PatternKind::Folded, fold(body).text, link);
            }
            // Exact raw scalar bytes retain leading/trailing whitespace. The
            // floor is still evaluated on the body, never on those fringes.
            if raw != value {
                push(PatternKind::Exact, raw, link);
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
struct UrlRuns {
    urls: Vec<Range<usize>>,
}

impl UrlRuns {
    fn new(text: &str) -> Self {
        #[cfg(test)]
        SWEEP_WORK.with(|work| {
            let mut count = work.get();
            count.url_scan_bytes += text.len();
            work.set(count);
        });
        let mut urls = Vec::new();
        let mut start = 0;
        for (at, ch) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            if ch.is_whitespace() {
                let run = &text[start..at];
                if run.contains("://")
                    || run
                        .get(..4)
                        .is_some_and(|head| head.eq_ignore_ascii_case("www."))
                {
                    urls.push(start..at);
                }
                start = at + ch.len_utf8();
            }
        }
        Self { urls }
    }

    fn contains(&self, _text: &str, span: &Range<usize>) -> bool {
        #[cfg(test)]
        SWEEP_WORK.with(|work| {
            let mut count = work.get();
            count.url_queries += 1;
            work.set(count);
        });
        let at = self.urls.partition_point(|run| run.end <= span.start);
        self.urls.get(at).is_some_and(|run| run.start < span.end)
    }
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
    include!("sweep/carrier_tests.rs");

    #[cfg(feature = "bundled-recognizers")]
    fn labelled_pipeline() -> crate::Pipeline {
        use crate::rule::DefaultRule;
        use crate::{Action, Pipeline};
        let core = crate::Rulepack::load(crate::RulepackSource::Embedded(
            gaze_recognizers::embedded("core").unwrap(),
        ))
        .unwrap();
        let mut builder = Pipeline::builder().rule(DefaultRule::new(Action::Tokenize));
        for rule in core.recognizers.into_iter().filter(|rule| {
            matches!(
                rule.id.as_str(),
                "customer_id.labelled" | "employee_id.labelled" | "record_id.labelled"
            )
        }) {
            let crate::RawMatch::Regex {
                pattern: Some(pattern),
                capture_groups,
                ..
            } = rule.matcher
            else {
                panic!("regex rule")
            };
            let detector = gaze_recognizers::RegexDetector::with_rulepack_fields(
                &pattern,
                rule.class,
                &rule.id,
                vec![crate::LocaleTag::Global],
                0.9,
                0,
                "counter",
                capture_groups,
                vec![],
                None,
                None,
            )
            .unwrap()
            .with_complete_labelled_value(true)
            .with_labelled_value_mode(gaze_types::LabelledValueMode::FieldOwned);
            builder = builder.recognizer(detector);
        }
        builder.build().unwrap()
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn pipeline_labelled_sweep_work_and_size_are_bounded() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        for body in [
            "customerId: \"SYN731284 ",
            "customerId: ''SYN''x''\n",
            "customerId: \"\"\"SYN'''x\n",
        ] {
            for depth in 0..=2 {
                for fields in [8, 32, 128, 512, 2048] {
                    let mut input = body.repeat(fields);
                    for _ in 0..depth {
                        input = serde_json::to_string(&input).unwrap();
                    }
                    let session = Session::new(Scope::Ephemeral).unwrap();
                    SWEEP_WORK.with(|work| work.set(SweepWork::default()));
                    let CleanDocument::Text(clean) = pipeline
                        .redact(&session, RawDocument::Text(input.clone()))
                        .unwrap()
                    else {
                        panic!("text")
                    };
                    assert!(
                        !clean.contains("SYN731284"),
                        "owned value must stay protected"
                    );
                    assert_eq!(
                        pipeline.restore_strict_text(&session, &clean).unwrap(),
                        input
                    );
                    pipeline
                        .redact(&session, RawDocument::Text("public control".to_owned()))
                        .unwrap();
                    let work = SWEEP_WORK.with(|work| work.get());
                    assert!(
                        work.max_pattern_bytes <= 40,
                        "{fields} fields, depth {depth}: {work:?}"
                    );
                    assert!(work.source_bytes <= input.len() * 8, "{work:?}");
                    assert!(work.pattern_bytes <= input.len() * 8, "{work:?}");
                    assert!(work.matcher_bytes <= input.len() * 128 + 65536, "{work:?}");
                }
            }
        }
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn pipeline_deep_escaped_repeat_patterns_remain_bounded() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut raw = "SYN731\"284\n".to_owned();
        let value = format!("{}731284", "İ".repeat(16));
        assert!(value.len() <= MAX_LABELLED_REPEAT_BYTES);
        assert!(fold(&value).text.len() > MAX_LABELLED_REPEAT_BYTES);
        let input = format!("Lookup {value} again.\n{{\"customerId\":\"{value}\"}}");
        let session = Session::new(Scope::Ephemeral).unwrap();
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(input.clone()))
            .unwrap()
        else {
            panic!("text")
        };
        assert!(
            !clean.contains(&value),
            "short exact spelling must survive fold expansion"
        );
        assert_eq!(
            pipeline.restore_strict_text(&session, &clean).unwrap(),
            input
        );
        let mut checked = 1;
        for _depth in 1..=9 {
            let encoded = serde_json::to_string(&raw).unwrap();
            raw = encoded[1..encoded.len() - 1].to_owned();
            let input = format!("Lookup {raw} again.\n{{\"customerId\":\"{raw}\"}}");
            let session = Session::new(Scope::Ephemeral).unwrap();
            SWEEP_WORK.with(|work| work.set(SweepWork::default()));
            let CleanDocument::Text(clean) = pipeline
                .redact(&session, RawDocument::Text(input.clone()))
                .unwrap()
            else {
                panic!("text")
            };
            let entries = session.snapshot_entries();
            let entry = entries
                .iter()
                .find(|entry| entry.raw == raw)
                .expect("complete source spelling");
            assert_eq!(clean, input.replace(&raw, &entry.token));
            assert_eq!(
                pipeline.restore_strict_text(&session, &clean).unwrap(),
                input
            );
            let work = SWEEP_WORK.with(|work| work.get());
            assert!(
                work.max_pattern_bytes <= MAX_LABELLED_REPEAT_BYTES,
                "{work:?}"
            );
            assert!(work.matcher_bytes <= input.len() * 128 + 65536, "{work:?}");
            checked += 1;
        }
        assert_eq!(checked, 10);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn deeply_escaped_padding_keeps_trimmed_repeats_across_turns() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for padding in ["\t", "\n", "\u{00a0}", " "] {
            for count in [1, 64] {
                let mut raw = format!("SYN731284{}", padding.repeat(count));
                for _depth in 1..=5 {
                    let encoded = serde_json::to_string(&raw).unwrap();
                    raw = encoded[1..encoded.len() - 1].to_owned();
                    let input = format!("Lookup SYN731284 again.\n{{\"customerId\":\"{raw}\"}}");
                    let session =
                        Session::new(Scope::Conversation("synthetic-decoded-padding".into()))
                            .unwrap();
                    SWEEP_WORK.with(|work| work.set(SweepWork::default()));
                    let CleanDocument::Text(clean) = pipeline
                        .redact(&session, RawDocument::Text(input.clone()))
                        .unwrap()
                    else {
                        panic!("text")
                    };
                    assert!(!clean.contains("SYN731284"), "{input:?} -> {clean}");
                    assert_eq!(
                        pipeline.restore_strict_text(&session, &clean).unwrap(),
                        input
                    );
                    let imported = Session::import(session.export().unwrap()).unwrap();
                    let CleanDocument::Text(next) = pipeline
                        .redact(&imported, RawDocument::Text("Follow-up SYN731284".into()))
                        .unwrap()
                    else {
                        panic!("text")
                    };
                    assert!(!next.contains("SYN731284"), "{next}");
                    let work = SWEEP_WORK.with(|work| work.get());
                    assert!(
                        work.max_pattern_bytes <= MAX_LABELLED_REPEAT_BYTES,
                        "{work:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 40);
    }

    #[test]
    fn cascading_unicode_escapes_cannot_create_unbounded_repeat_work() {
        let raw = format!("\\{}SYN731284", "u005c".repeat(256));
        let source = SweepSource {
            family: "synthetic.counter".into(),
            class: PiiClass::custom("customer_id").unwrap(),
            raw: raw.clone(),
            repeat_seed: Some(RepeatSeed {
                raw,
                kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
            }),
        };
        assert!(matches!(
            SweepMatcher::build([source]),
            Err(ManifestSweepError::CapacityExceeded)
        ));
    }

    #[test]
    fn field_owned_families_keep_byte_exact_matching() {
        let source = SweepSource {
            family: "synthetic.counter".into(),
            class: PiiClass::family("synthetic_identifier"),
            raw: "AB1234".into(),
            repeat_seed: Some(RepeatSeed {
                raw: "AB1234".into(),
                kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
            }),
        };
        let matcher = SweepMatcher::build([source]).unwrap().unwrap();
        let hits = matcher.find("AB1234 ab1234");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].span, 0..6);
    }

    #[test]
    fn legacy_uncertain_prefixes_keep_their_floor_and_long_values() {
        for body in ["ABCD", "Q9176243 AB2345 CD6789 EF1234 GH5678 IJ9012 KL3456"] {
            let source = SweepSource {
                family: "synthetic.counter".into(),
                class: PiiClass::custom("synthetic_identifier").unwrap(),
                raw: format!("{body} PRIVATE NOTE"),
                repeat_seed: Some(RepeatSeed {
                    raw: body.into(),
                    kind: gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix,
                }),
            };
            let matcher = SweepMatcher::build([source])
                .unwrap()
                .expect("legacy prefix");
            let text = format!("Lookup {body} again.");
            assert!(
                matcher
                    .find(&text)
                    .iter()
                    .any(|hit| hit.span == (7..7 + body.len())
                        && hit.link == SweepLink::UncertainPrefix),
                "{text}"
            );
        }
    }

    #[cfg(feature = "bundled-recognizers")]
    fn json_value_types(value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), json_value_types(value)))
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.iter().map(json_value_types).collect()),
            Value::String(_) => Value::String("string".into()),
            Value::Number(_) => Value::String("number".into()),
            other => other.clone(),
        }
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn mixed_carriers_preserve_json_keys_types_and_string_boundaries() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for seed in [
            "customerId: 731284",
            "Customer ID = 731284",
            "customer_id: 731284",
            r#"{"customerId":"731284"}"#,
        ] {
            for prefix in [
                "",
                "payload=",
                "TV 55\" screen ",
                "path C:\\\" ",
                "msg \"oops ",
            ] {
                for body in [
                    r#"{"731284":true,"n":731284,"a":[731284,null,false],"copy":"731284"}"#,
                    r#"{"Customer ID:":"C-731284"}"#,
                    r#"{"properties":{"customerId":{"type":"string","maxLength":36}},"required":["customerId"]}"#,
                    r#"{"customerId":{"type":"integer","minimum":1,"maximum":999999}}"#,
                    r#"{"customerId":{"format":"int64","pattern":"^C-[0-9]{6}$","description":"6 digit id","examples":["C-123456"],"enum":["C-123456"],"version":2}}"#,
                ] {
                    let original: serde_json::Value = serde_json::from_str(body).unwrap();
                    for encoding in 0..3 {
                        let encoded = serde_json::to_string(body).unwrap();
                        let carrier = match encoding {
                            0 => body.to_owned(),
                            1 => encoded[1..encoded.len() - 1].to_owned(),
                            _ => encoded,
                        };
                        for separate_turn in [false, true] {
                            let session = Session::new(Scope::Ephemeral).unwrap();
                            let input = if separate_turn {
                                pipeline
                                    .redact(&session, RawDocument::Text(seed.into()))
                                    .unwrap();
                                format!("CARRIER {prefix}{carrier}")
                            } else {
                                format!("{seed}\nCARRIER {prefix}{carrier}")
                            };
                            let CleanDocument::Text(clean) = pipeline
                                .redact(&session, RawDocument::Text(input.clone()))
                                .unwrap()
                            else {
                                panic!("text")
                            };
                            assert_eq!(
                                pipeline.restore_strict_text(&session, &clean).unwrap(),
                                input
                            );
                            let output = clean
                                .split_once("CARRIER ")
                                .unwrap()
                                .1
                                .strip_prefix(prefix)
                                .unwrap_or_else(|| panic!("prefix changed: {clean}"));
                            let decoded =
                                match encoding {
                                    0 => output.to_owned(),
                                    1 => serde_json::from_str::<String>(&format!("\"{output}\""))
                                        .unwrap_or_else(|error| {
                                            panic!("{input:?} -> {clean:?}: {error}")
                                        }),
                                    _ => serde_json::from_str::<String>(output).unwrap_or_else(
                                        |error| panic!("{input:?} -> {clean:?}: {error}"),
                                    ),
                                };
                            let parsed: serde_json::Value = serde_json::from_str(&decoded)
                                .unwrap_or_else(|error| panic!("{input:?} -> {clean:?}: {error}"));
                            assert_eq!(
                                json_value_types(&original),
                                json_value_types(&parsed),
                                "{input:?} -> {clean:?}"
                            );
                            if !original["properties"].is_null()
                                || original["customerId"].is_object()
                            {
                                assert_eq!(original, parsed, "schema changed: {clean}");
                            }
                            for key in ["copy", "Customer ID:"] {
                                if original[key].is_string() {
                                    assert_ne!(parsed[key], original[key], "value leaked: {clean}");
                                }
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 600);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn decoded_record_ids_repeat_between_raw_spellings_and_turns() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for (seed, copy) in [
            (r"SYN\u0037\u0033\u0031\u0032\u0038\u0034", "SYN731284"),
            ("SYN731284", r"SYN\u0037\u0033\u0031\u0032\u0038\u0034"),
        ] {
            for separate_turn in [false, true] {
                for quoted in [false, true] {
                    let session = Session::new(Scope::Ephemeral).unwrap();
                    let seed = format!(r#"{{"customerId":"{seed}"}}"#);
                    let copy = if quoted {
                        format!(r#"{{"m":"{copy}"}}"#)
                    } else {
                        format!("Ref {copy} noted.")
                    };
                    let input = if separate_turn {
                        pipeline.redact(&session, RawDocument::Text(seed)).unwrap();
                        copy
                    } else {
                        format!("{copy}\n{seed}")
                    };
                    let CleanDocument::Text(clean) = pipeline
                        .redact(&session, RawDocument::Text(input.clone()))
                        .unwrap()
                    else {
                        panic!("text")
                    };
                    assert!(
                        !clean.contains("SYN731284") && !clean.contains(r"SYN\u0037"),
                        "{input} -> {clean}"
                    );
                    assert_eq!(
                        pipeline.restore_strict_text(&session, &clean).unwrap(),
                        input
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 8);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn labelled_repeat_preserves_json_keys_literals_and_schema() {
        assert!(SweepMatcher::build([SweepSource {
            family: "test".into(),
            class: PiiClass::custom("customer_id").unwrap(),
            raw: "opaquevalue".into(),
            repeat_seed: Some(RepeatSeed {
                raw: "opaquevalue".into(),
                kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
            }),
        }])
        .unwrap()
        .is_none());
        // A whole deterministic value crossing JSON syntax still needs the strict gate.
        let matcher = SweepMatcher::build([SweepSource {
            family: "test".into(),
            class: PiiClass::custom("test").unwrap(),
            raw: r#"731284","n":731284"#.into(),
            repeat_seed: None,
        }])
        .unwrap()
        .unwrap();
        assert!(matcher.find(r#"{"copy":"731284","n":731284}"#).is_empty());
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for value in [
            "null",
            "true",
            "false",
            "status",
            "string",
            "guest",
            "unknown",
            "SYN731284",
            "731284",
        ] {
            let body = serde_json::json!({"customerId": value, (value): "public", "copy": value, "v":null, "active":true, "n":731284, "schema":{"type":"string"}}).to_string();
            let mut input = body.clone();
            for depth in 0..=4 {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                let mut decoded = clean;
                for _ in 0..depth {
                    decoded = serde_json::from_str::<String>(&decoded).unwrap();
                }
                let parsed: serde_json::Value = serde_json::from_str(&decoded)
                    .unwrap_or_else(|error| panic!("{decoded}: {error}"));
                assert_eq!(parsed[value], "public", "object key changed: {decoded}");
                assert_eq!(parsed["v"], serde_json::Value::Null);
                assert_eq!(parsed["active"], true);
                assert_eq!(parsed["n"], 731284);
                assert_eq!(parsed["schema"]["type"], "string");
                if !value.chars().any(|ch| ch.is_ascii_digit()) {
                    assert_eq!(parsed["copy"], value);
                }
                let CleanDocument::Text(number) = pipeline
                    .redact(&session, RawDocument::Text("731284".into()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(
                    number, "731284",
                    "a repeat cannot replace a standalone JSON number"
                );
                let CleanDocument::Text(next) = pipeline
                    .redact(
                        &session,
                        RawDocument::Text(format!("Follow-up about {value}")),
                    )
                    .unwrap()
                else {
                    panic!("text")
                };
                if !value.chars().any(|ch| ch.is_ascii_digit()) {
                    assert!(next.contains(value), "{next}");
                }
                checked += 1;
                input = serde_json::to_string(&input).unwrap();
            }
        }
        assert_eq!(checked, 45);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn labelled_repeats_cannot_split_json_escapes() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for (value, escaped) in [
            ("u0031", r#"\u0031"#),
            ("t731284", r#"\t731284"#),
            ("n731284", r#"\n731284"#),
        ] {
            let mut input = format!(r#"{{"customerId":"{value}","copy":"{escaped}"}}"#);
            for depth in 0..=4 {
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                let mut decoded = clean;
                for _ in 0..depth {
                    decoded = serde_json::from_str::<String>(&decoded).expect("serialized JSON");
                }
                let parsed: serde_json::Value = serde_json::from_str(&decoded)
                    .unwrap_or_else(|error| panic!("{decoded}: {error}"));
                assert_eq!(
                    parsed["copy"],
                    serde_json::from_str::<String>(&format!("\"{escaped}\"")).unwrap()
                );
                let literal = format!("\"{escaped}\"");
                let CleanDocument::Text(literal_clean) = pipeline
                    .redact(&session, RawDocument::Text(literal.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert_eq!(literal_clean, literal, "standalone JSON escape split");
                checked += 1;
                input = serde_json::to_string(&input).unwrap();
            }
        }
        assert_eq!(checked, 15);
    }

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn padded_labelled_values_repeat_in_document_and_session() {
        use crate::{CleanDocument, RawDocument, Scope, Session};
        let pipeline = labelled_pipeline();
        let mut checked = 0;
        for padding in [" ", "\u{00a0}", "\\t", "\\u0009", " \\t ", "\\f", "\\u00a0"] {
            for count in [1, 16, 32, 64] {
                let input = format!(
                    r#"Ref SYN731284 noted.
{{"customerId":"SYN731284{}"}}"#,
                    padding.repeat(count)
                );
                let session = Session::new(Scope::Ephemeral).unwrap();
                let CleanDocument::Text(clean) = pipeline
                    .redact(&session, RawDocument::Text(input.clone()))
                    .unwrap()
                else {
                    panic!("text")
                };
                assert!(!clean.contains("SYN731284"), "{input:?} -> {clean}");
                assert_eq!(
                    pipeline.restore_strict_text(&session, &clean).unwrap(),
                    input
                );
                let CleanDocument::Text(next) = pipeline
                    .redact(
                        &session,
                        RawDocument::Text("Follow-up about SYN731284".into()),
                    )
                    .unwrap()
                else {
                    panic!("text")
                };
                assert!(!next.contains("SYN731284"), "{padding:?}, {count}: {next}");
                checked += 1;
            }
        }
        assert_eq!(checked, 28);
    }

    use super::*;

    /// A value kept although its checksum failed is never swept, whatever evidence it carries and
    /// even with a canonical form.
    #[test]
    fn a_recorded_checksum_failure_is_learned_evidence() {
        let mut candidate = Candidate::new(
            0..4,
            PiiClass::custom("iban").expect("class"),
            "iban.structural",
            0.7,
            80,
            Some("DE99".into()),
            "counter",
            "iban.structural",
            gaze_types::ConflictTier::None,
            Vec::new(),
        )
        .with_evidence(EvidenceKind::Rule);
        assert_eq!(
            ManifestEvidence::of(&candidate),
            ManifestEvidence::Validated
        );
        candidate.validator_fail_reason = Some(gaze_types::ValidatorFailReason::IbanMod97Failed);
        assert_eq!(ManifestEvidence::of(&candidate), ManifestEvidence::Learned);
        assert!(!ManifestEvidence::of(&candidate).propagates());
    }

    /// `Coverage` replaced a scan of every winner per sweep hit.
    /// Pin it against that scan on random, overlapping and empty spans.
    #[test]
    fn coverage_matches_the_full_scan() {
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }
        fn span(state: &mut u64) -> Range<usize> {
            let start = (next(state) % 50) as usize;
            start..start + (next(state) % 12) as usize
        }
        let mut state = 0x3895_u64;
        let (mut inside, mut outside) = (0, 0);
        for _ in 0..5_000 {
            let spans = (0..next(&mut state) % 10)
                .map(|_| span(&mut state))
                .collect::<Vec<_>>();
            let coverage = Coverage::new(spans.iter().cloned());
            for _ in 0..8 {
                let range = span(&mut state);
                let expected = spans
                    .iter()
                    .any(|span| span.start <= range.start && range.end <= span.end);
                assert_eq!(coverage.contains(&range), expected, "{spans:?} {range:?}");
                if expected {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
        assert!(inside > 1_000 && outside > 1_000, "{inside} / {outside}");
    }

    fn name(raw: &str) -> SweepSource {
        SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: raw.into(),
            repeat_seed: None,
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
            repeat_seed: None,
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
            repeat_seed: None,
        };
        assert!(SweepMatcher::build(vec![postcode]).unwrap().is_none());
        let long = SweepSource {
            family: "counter".into(),
            class: PiiClass::Custom("id".into()),
            raw: "123 456".into(),
            repeat_seed: None,
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
        assert!(!custom.clears_repeat_floor("10115"));
        assert!(custom.clears_repeat_floor("101 150"));
    }

    #[test]
    fn repeat_floor_independent_truth_table() {
        let ordinary = PiiClass::custom("synthetic_identifier").unwrap();
        let family = PiiClass::family("synthetic_identifier");
        for (body, ordinary_expected, family_expected) in [
            ("", false, false),
            ("   ", false, false),
            ("12345", false, true),
            ("123456", true, true),
            ("12-3./45", false, true),
            ("12-3./456", true, true),
            ("12 345", false, true),
            ("12\u{a0}3456", true, true),
            ("１２３４５", true, true),
            ("١٢٣٤٥", true, true),
            ("ABC", false, false),
            ("ABCD", true, true),
            ("A B C", false, false),
            ("A\u{202f}B C D", true, true),
        ] {
            assert_eq!(
                ordinary.clears_repeat_floor(body),
                ordinary_expected,
                "ordinary {body:?}"
            );
            assert_eq!(
                family.clears_repeat_floor(body),
                family_expected,
                "family {body:?}"
            );
        }
    }

    #[test]
    fn all_caps_single_word_sweeps_its_byte_exact_copy() {
        let source = SweepSource {
            family: "counter".into(),
            class: PiiClass::Name,
            raw: "KOWALSKI".into(),
            repeat_seed: None,
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
        // A cue-found value carries its honorific, so the bare
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
            repeat_seed: None,
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
            repeat_seed: None,
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
        // dropped from the production list fails this test. `'t` is left
        // out: it is not a word, so it can never be part of a run.
        #[rustfmt::skip]
        let words = [
            "von", "vom", "zu", "zum", "zur", "der", "den", "dem", "des", "van",
            "de", "het", "ten", "ter", "te", "op", "uit", "aan", "du", "la",
            "le", "les", "del", "las", "los", "y", "da", "das", "do", "dos", "e",
            "di", "dal", "dalla", "dei", "degli", "della", "delle", "dello", "af",
            "av", "al", "el", "bin", "ibn", "bint", "abu", "bar", "mac",
            "ap", "auf", "bij", "over", "voor", "onder", "dalle", "abd", "bat",
            "ferch", "ab", "am", "an", "i", "im", "in", "lo", "mc", "ni",
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
            repeat_seed: None,
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
            repeat_seed: None,
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
