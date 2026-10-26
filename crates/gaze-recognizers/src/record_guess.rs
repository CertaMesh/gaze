//! Prepared, batched exclusion of model-supplied guesses. Never detection evidence.
use std::collections::BTreeSet;

use aho_corasick::{automaton::Automaton, nfa::contiguous::NFA, Anchored};
use gaze_types::{PiiClass, RecordMatchKind};

use crate::dictionary::{
    fold_with_original_offsets, has_identifier_component, record_match_kind_with_folded,
    record_name_is_common_folded,
};

/// Matching semantics of the response recognizer that owns a term.
#[derive(Clone)]
pub enum RecordGuessMode {
    /// A non-record dictionary uses literal or ASCII-insensitive matching.
    Dictionary { case_sensitive: bool },
    /// Record matchers fold Unicode only for classes configured to do so.
    Record {
        class: PiiClass,
        allowed_kinds: BTreeSet<RecordMatchKind>,
    },
}

/// One private term and its response-matching contract.
pub struct RecordGuessTerm<'a> {
    pub value: &'a str,
    pub mode: RecordGuessMode,
}

/// Normalize and fold an argument once, before matching any private terms.
/// Debug deliberately does not expose the text.
pub struct RecordGuessText {
    normalized: String,
    folded: String,
    starts: std::collections::HashMap<usize, usize>,
    ends: std::collections::HashMap<usize, usize>,
    literal_folded: String,
    literal_offsets: std::collections::HashMap<usize, usize>,
}

#[cfg(test)]
thread_local! { pub(crate) static WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) }; }

impl RecordGuessText {
    pub fn new(value: &str) -> Self {
        Self::from_normalized(crate::normalize::normalize(value).text)
    }

    fn from_normalized(normalized: String) -> Self {
        let (literal_folded, literal_starts, literal_ends) =
            fold_with_original_offsets(&normalized, true, None);
        let mut literal_offsets = std::collections::HashMap::from([(0, 0)]);
        for (&folded, &raw) in literal_starts.iter().chain(&literal_ends) {
            literal_offsets.insert(raw, folded);
        }
        let mut folded = String::new();
        let mut starts = std::collections::HashMap::new();
        let mut ends = std::collections::HashMap::new();
        let mut whitespace_count = 0usize;
        for (index, ch) in literal_folded.char_indices() {
            let end = index + ch.len_utf8();
            if ch.is_whitespace() {
                whitespace_count += 1;
                if whitespace_count == 1 {
                    if let Some(raw) = literal_starts.get(&index) {
                        starts.insert(folded.len(), *raw);
                    }
                    folded.push(' ');
                }
            } else {
                whitespace_count = 0;
                if let Some(raw) = literal_starts.get(&index) {
                    starts.insert(folded.len(), *raw);
                }
                folded.push(ch);
            }
            if let Some(raw) = literal_ends.get(&end) {
                ends.insert(folded.len(), *raw);
            }
        }
        Self {
            normalized,
            folded,
            starts,
            ends,
            literal_folded,
            literal_offsets,
        }
    }

    /// Both forms must fit the host's per-view byte budget.
    pub fn budget_bytes(&self) -> usize {
        self.normalized.len().max(self.folded.len())
    }
}

/// One Aho–Corasick automaton for all terms, shared by every argument and join.
/// Returned match flags have the same order as the constructor's terms.
pub struct RecordGuessMatcher<'a> {
    automaton: NFA,
    terms: Vec<RecordGuessTerm<'a>>,
    lengths: Vec<usize>,
    folded_terms: Vec<String>,
    corroborate_single: Vec<bool>,
    max_length: usize,
    matched: Vec<bool>,
}

impl<'a> RecordGuessMatcher<'a> {
    pub fn new(terms: Vec<RecordGuessTerm<'a>>) -> Result<Self, aho_corasick::BuildError> {
        let patterns = terms
            .iter()
            .map(|term| {
                let prepared = RecordGuessText::from_normalized(term.value.to_owned());
                (prepared.folded, prepared.literal_folded)
            })
            .collect::<Vec<_>>();
        let (patterns, folded_terms): (Vec<_>, Vec<_>) = patterns.into_iter().unzip();
        let corroborate_single = terms
            .iter()
            .zip(&folded_terms)
            .map(|(term, folded)| {
                matches!(
                    &term.mode,
                    RecordGuessMode::Record {
                        class: PiiClass::Name,
                        ..
                    }
                ) && !term.value.chars().any(char::is_whitespace)
                    && record_name_is_common_folded(folded)
            })
            .collect();
        let lengths = patterns.iter().map(String::len).collect::<Vec<_>>();
        let max_length = lengths.iter().copied().max().unwrap_or(0);
        Ok(Self {
            automaton: NFA::new(&patterns)?,
            matched: vec![false; terms.len()],
            terms,
            lengths,
            folded_terms,
            corroborate_single,
            max_length,
        })
    }

    pub fn matches(&self) -> &[bool] {
        &self.matched
    }

    pub fn check(&mut self, text: &RecordGuessText) {
        self.scan(&[text], false);
    }

    /// All scalar fields in their original order, with one prepared separator.
    pub fn check_join(&mut self, texts: &[&RecordGuessText], separator: &RecordGuessText) {
        let mut parts = Vec::with_capacity(texts.len().saturating_mul(2));
        for text in texts {
            if !parts.is_empty() {
                parts.push(separator);
            }
            parts.push(*text);
        }
        self.scan(&parts, false);
    }

    /// Stream only a term-length suffix/prefix around the junction. No pair
    /// strings are allocated and neither argument is normalized or folded again.
    pub fn check_pair(
        &mut self,
        left: &RecordGuessText,
        separator: &RecordGuessText,
        right: &RecordGuessText,
    ) {
        self.scan(&[left, separator, right], true);
    }

    fn scan(&mut self, parts: &[&RecordGuessText], junction_only: bool) {
        if self.max_length == 0 {
            return;
        }
        let mut state = self
            .automaton
            .start_state(Anchored::No)
            .expect("unanchored NFA");
        let ring_size = self.max_length + 1;
        let mut starts = vec![None; ring_size];
        let mut ends = vec![None; ring_size];
        let mut offset = 0;
        let mut raw_offset = 0;
        let mut previous_space = false;
        for (part_index, part) in parts.iter().enumerate() {
            let mut range = 0..part.folded.len();
            if junction_only {
                if part_index == 0 {
                    range.start = part.folded.len().saturating_sub(self.max_length);
                }
                if part_index == parts.len() - 1 {
                    range.end = part.folded.len().min(self.max_length);
                }
            }
            for index in range {
                let byte = part.folded.as_bytes()[index];
                // Each part already collapses whitespace; collapse at seams too.
                if index == 0 && byte == b' ' && previous_space {
                    ends[offset % ring_size] = part.ends.get(&1).map(|end| raw_offset + end);
                    continue;
                }
                previous_space = byte == b' ';
                starts[offset % ring_size] =
                    part.starts.get(&index).map(|start| raw_offset + start);
                offset += 1;
                ends[offset % ring_size] = part.ends.get(&(index + 1)).map(|end| raw_offset + end);
                state = self.automaton.next_state(Anchored::No, state, byte);
                if !self.automaton.is_match(state) {
                    continue;
                }
                for hit_index in 0..self.automaton.match_len(state) {
                    let pattern = self.automaton.match_pattern(state, hit_index).as_usize();
                    let length = self.lengths[pattern];
                    if length == 0 || self.matched[pattern] {
                        continue;
                    }
                    let Some(start) = starts[(offset - length) % ring_size] else {
                        continue;
                    };
                    let Some(end) = ends[offset % ring_size] else {
                        continue;
                    };
                    if token_boundary(parts, start, end) && self.accepts(pattern, parts, start, end)
                    {
                        self.matched[pattern] = true;
                    }
                }
            }
            raw_offset += part.normalized.len();
        }
    }

    fn accepts(
        &self,
        pattern: usize,
        parts: &[&RecordGuessText],
        start: usize,
        end: usize,
    ) -> bool {
        let term = &self.terms[pattern];
        let matched = slice(parts, start, end);
        match &term.mode {
            RecordGuessMode::Dictionary {
                case_sensitive: true,
            } => matched == term.value,
            RecordGuessMode::Dictionary {
                case_sensitive: false,
            } => matched.eq_ignore_ascii_case(term.value),
            RecordGuessMode::Record {
                class,
                allowed_kinds,
            } => {
                let kind = if self.corroborate_single[pattern] {
                    RecordMatchKind::CorroboratedSingle
                } else {
                    record_match_kind_with_folded(&matched, term.value, || {
                        folded_slice(parts, start, end) == self.folded_terms[pattern]
                    })
                };
                let compiled_match = *class == PiiClass::Name
                    || matches!(
                        kind,
                        RecordMatchKind::Exact | RecordMatchKind::WhitespaceFlexible
                    );
                compiled_match && allowed_kinds.contains(&kind)
            }
        }
    }
}

fn slice(parts: &[&RecordGuessText], start: usize, end: usize) -> String {
    let mut output = String::new();
    let mut base = 0;
    for part in parts {
        let limit = base + part.normalized.len();
        if start < limit && end > base {
            output.push_str(
                &part.normalized
                    [start.saturating_sub(base)..(end - base).min(part.normalized.len())],
            );
        }
        base = limit;
    }
    output
}

fn folded_slice(parts: &[&RecordGuessText], start: usize, end: usize) -> String {
    let mut output = String::new();
    let mut base = 0;
    for part in parts {
        let limit = base + part.normalized.len();
        if start < limit && end > base {
            let left = start.saturating_sub(base);
            let right = (end - base).min(part.normalized.len());
            let from = part.literal_offsets[&left];
            let to = part.literal_offsets[&right];
            output.push_str(&part.literal_folded[from..to]);
        }
        base = limit;
    }
    output
}

fn token_boundary(parts: &[&RecordGuessText], start: usize, end: usize) -> bool {
    let mut base = 0;
    let mut ranges = Vec::with_capacity(parts.len());
    for part in parts {
        ranges.push((base, base + part.normalized.len(), part.normalized.as_str()));
        base += part.normalized.len();
    }
    let before = ranges
        .iter()
        .rev()
        .filter(|(base, _, _)| *base < start)
        .flat_map(|(base, limit, text)| text[..start.min(*limit) - base].chars().rev());
    let after = ranges
        .iter()
        .filter(|(_, limit, _)| *limit > end)
        .flat_map(|(base, _, text)| text[end.saturating_sub(*base)..].chars());
    !has_identifier_component(before) && !has_identifier_component(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_guess_preparation_is_independent_of_terms_and_pairs() {
        WORK.with(|count| count.set((0, 0)));
        let texts = (0..32)
            .map(|i| RecordGuessText::new(&format!("Synthetic{i} ")))
            .collect::<Vec<_>>();
        let terms = (0..32)
            .map(|i| format!("Private{i} Qorvex"))
            .collect::<Vec<_>>();
        let mut matcher = RecordGuessMatcher::new(
            terms
                .iter()
                .map(|value| RecordGuessTerm {
                    value,
                    mode: RecordGuessMode::Dictionary {
                        case_sensitive: true,
                    },
                })
                .collect(),
        )
        .unwrap();
        let separator = RecordGuessText::new(" ");
        for left in &texts {
            matcher.check(left);
            for right in &texts {
                matcher.check_pair(left, &separator, right);
            }
        }
        assert!(matcher.matches().iter().all(|matched| !matched));
        assert_eq!(WORK.with(|count| count.get()), (33, 65));
    }
    fn name_mode() -> RecordGuessMode {
        RecordGuessMode::Record {
            class: PiiClass::Name,
            allowed_kinds: [
                RecordMatchKind::Exact,
                RecordMatchKind::WhitespaceFlexible,
                RecordMatchKind::CaseFolded,
                RecordMatchKind::WhitespaceCaseFolded,
                RecordMatchKind::CorroboratedSingle,
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn record_guess_junctions_match_materialized_reference() {
        let values = [
            "",
            "Synthetic",
            "Qorvex",
            "Synthetic ",
            "\tQorvex",
            "Anna",
            " Li",
            "Joanna",
            "Lim",
            "Li-suffix",
            "İAnna",
            "ß",
            "SS",
            "Ｓynthetic",
            "Qor\u{200d}vex",
            "000",
            "01",
            "\t\t",
        ];
        let terms = [
            "Synthetic Qorvex",
            "SyntheticQorvex",
            "Synthetic-Qorvex",
            "Anna Li",
            "ssSS",
            "ßSS",
            "00001",
            "İAnna Li",
        ];
        for left in values {
            for right in values {
                for separator in ["", " ", "@", "-", "/"] {
                    let make = || {
                        RecordGuessMatcher::new(
                            terms
                                .iter()
                                .map(|value| RecordGuessTerm {
                                    value,
                                    mode: name_mode(),
                                })
                                .collect(),
                        )
                        .unwrap()
                    };
                    let mut streamed = make();
                    let mut reference = make();
                    let left_view = RecordGuessText::new(left);
                    let right_view = RecordGuessText::new(right);
                    for matcher in [&mut streamed, &mut reference] {
                        matcher.check(&left_view);
                        matcher.check(&right_view);
                    }
                    streamed.check_pair(&left_view, &RecordGuessText::new(separator), &right_view);
                    reference.check(&RecordGuessText::new(&format!("{left}{separator}{right}")));
                    assert_eq!(
                        streamed.matches(),
                        reference.matches(),
                        "left={left:?} separator={separator:?} right={right:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn record_guess_allowed_kinds_and_compilation_modes_are_independent() {
        for (class, kinds, input, expected) in [
            (
                PiiClass::Name,
                vec![RecordMatchKind::Exact],
                "synthetic qorvex",
                false,
            ),
            (
                PiiClass::Name,
                vec![RecordMatchKind::Exact],
                "Synthetic  Qorvex",
                false,
            ),
            (
                PiiClass::Name,
                vec![RecordMatchKind::CaseFolded],
                "synthetic qorvex",
                true,
            ),
            (
                PiiClass::Name,
                vec![RecordMatchKind::CaseFolded],
                "synthetic  qorvex",
                false,
            ),
            (
                PiiClass::Name,
                vec![RecordMatchKind::WhitespaceFlexible],
                "Synthetic  Qorvex",
                true,
            ),
            (
                PiiClass::Custom("private_value".into()),
                vec![RecordMatchKind::CaseFolded],
                "synthetic qorvex",
                false,
            ),
            (
                PiiClass::Custom("private_value".into()),
                vec![RecordMatchKind::WhitespaceFlexible],
                "Synthetic  Qorvex",
                true,
            ),
        ] {
            let mut matcher = RecordGuessMatcher::new(vec![RecordGuessTerm {
                value: "Synthetic Qorvex",
                mode: RecordGuessMode::Record {
                    class,
                    allowed_kinds: kinds.into_iter().collect(),
                },
            }])
            .unwrap();
            matcher.check(&RecordGuessText::new(input));
            assert_eq!(matcher.matches(), &[expected], "{input}");
        }
    }
}
