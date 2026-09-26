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
//!   matches only as written or in title case, and never when it is on the
//!   closed common-word list. Matching lower-case single words would tokenize
//!   ordinary words that happen to be names, so a lone lower-case `maria` is a
//!   stated leak.
//! - Collision-family tokens match byte-exact only: the family class exists
//!   because the value's class was ambiguous, and folding would widen that.
//! - Every edge must pass [`gaze_types::is_inside_word`], and a hit inside a
//!   URL-shaped run is skipped so a link is never cut apart.

use std::collections::HashSet;
use std::ops::Range;

use aho_corasick::{AhoCorasick, MatchKind};
use gaze_types::{is_inside_word, Candidate, ConflictTier, PiiClass};
use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// Recognizer id, source label and audit stage of a swept copy.
pub(crate) const SWEEP_ID: &str = "manifest_sweep";

/// Whole values shorter than this many non-space characters are left to
/// their own recognizers.
const MIN_VALUE_CHARS: usize = 4;
/// Name parts need at least this many letters.
const MIN_PART_LETTERS: usize = 3;
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
        } else if crate::resolver::is_learned(candidate) {
            Self::Learned
        } else {
            Self::Pattern
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
    /// A title-case part of a multi-word name.
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
            // A copy that reaches the resolver sits in a gap or overlaps a
            // weaker same-class span (an NER fragment such as `<Name_1>a`);
            // it must win the same-class ladder so the union is covered.
            i32::MAX,
            None,
            self.family.clone(),
            SWEEP_ID,
            ConflictTier::ManifestSweep,
            Vec::new(),
        );
        candidate.source_recognizer_ids = vec![SWEEP_ID.to_string()];
        candidate
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
        if span.is_empty()
            || is_inside_word(text, span.start)
            || is_inside_word(text, span.end)
            || inside_url(text, &span)
        {
            return;
        }
        let source = &self.sources[source];
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

fn source_patterns(index: usize, source: &SweepSource, patterns: &mut Vec<Pattern>) {
    let normalized = normalize(&source.raw).text;
    let value = normalized.trim();
    let non_space = value.chars().filter(|ch| !ch.is_whitespace()).count();
    let mut push = |kind, text: String, link| {
        patterns.push(Pattern {
            kind,
            text,
            source: index,
            link,
        })
    };
    if source.class.as_family_name().is_some() {
        if non_space >= MIN_VALUE_CHARS {
            push(PatternKind::Exact, value.to_string(), SweepLink::Exact);
        }
        return;
    }
    if is_word(value) {
        if non_space >= MIN_VALUE_CHARS {
            for spelling in word_spellings(value) {
                push(PatternKind::Exact, spelling, SweepLink::Exact);
            }
        }
        return;
    }
    if non_space >= MIN_VALUE_CHARS {
        push(PatternKind::Folded, fold(value).text, SweepLink::Exact);
    }
    if source.class == PiiClass::Name {
        for part in value.split_whitespace().filter(|part| is_word(part)) {
            if part.chars().filter(|ch| ch.is_alphabetic()).count() >= MIN_PART_LETTERS {
                for spelling in word_spellings(part) {
                    push(PatternKind::Exact, spelling, SweepLink::Part);
                }
            }
        }
    }
}

/// A single word: letters, joined by at most internal hyphens or apostrophes.
fn is_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .split(['-', '\'', '’'])
            .all(|piece| !piece.is_empty() && piece.chars().all(char::is_alphabetic))
}

/// The spellings a single word is swept in: title case, plus the word as
/// written when it starts upper-case and is not all capitals (`McDonald`).
/// Empty for a word on the common-word list.
fn word_spellings(word: &str) -> Vec<String> {
    if COMMON_WORDS.contains(&word.to_lowercase().as_str()) {
        return Vec::new();
    }
    let title = title_case(word);
    let mut spellings = vec![title.clone()];
    let starts_upper = word.chars().next().is_some_and(char::is_uppercase);
    let all_upper = word
        .chars()
        .filter(|ch| ch.is_alphabetic())
        .all(char::is_uppercase);
    if starts_upper && !all_upper && word != title {
        spellings.push(word.to_string());
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
    fn parts_match_in_title_case_only() {
        let sources = vec![name("MARIA SCHNEIDER")];
        assert_eq!(found(sources.clone(), "Thanks, Maria"), ["Maria"]);
        assert!(found(sources.clone(), "thanks maria").is_empty());
        assert!(found(sources, "thanks MARIA").is_empty());
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
}
