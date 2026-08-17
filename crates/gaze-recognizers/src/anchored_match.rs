use gaze_types::{
    Candidate, ConflictTier, DetectContext, LocaleBasis, LocaleTag, PiiClass, Recognizer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchoredBoundary {
    Punctuation,
    Whitespace,
    LineEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameShape {
    PersonName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuePosition {
    Before,
    After,
}

/// Recognizes person names near literal locale cues.
///
/// The `person_name` grammar accepts Unicode letter components with internal
/// apostrophe or hyphen joins, initials written as a single uppercase letter
/// followed by `.`, and non-leading particles (`de`, `la`, `van`, `der`) only
/// when sandwiched between uppercase components. Matching is max-greedy,
/// capped at four components and 64 bytes.
pub struct AnchoredMatchRecognizer {
    id: String,
    /// Cues pre-folded with [`fold_char`], so `detect` folds only the input.
    folded_cues: Vec<String>,
    boundary: AnchoredBoundary,
    right_window_chars: u16,
    name_shape: NameShape,
    cue_position: CuePosition,
    source: String,
    score: f32,
    priority: i32,
    token_family: String,
    locales: Vec<LocaleTag>,
    locale_basis: LocaleBasis,
    min_components: usize,
}

impl AnchoredMatchRecognizer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        cues: Vec<String>,
        boundary: AnchoredBoundary,
        right_window_chars: u16,
        name_shape: NameShape,
        cue_position: CuePosition,
        source_short_label: String,
        min_components: usize,
        score: f32,
        priority: i32,
    ) -> Self {
        let mut folded_cues = cues
            .into_iter()
            .filter(|cue| !cue.trim().is_empty())
            .map(|cue| cue.chars().flat_map(fold_char).collect::<String>())
            .collect::<Vec<_>>();
        folded_cues.sort_by_key(|cue| std::cmp::Reverse(cue.chars().count()));
        Self {
            id,
            folded_cues,
            boundary,
            right_window_chars,
            name_shape,
            cue_position,
            source: format!("structural.{source_short_label}"),
            score,
            priority,
            token_family: "name.counter".to_string(),
            locales: vec![LocaleTag::Global],
            locale_basis: LocaleBasis::Document,
            min_components,
        }
    }

    /// Overrides the locale provenance and eligibility interpretation.
    pub fn with_locale_metadata(
        mut self,
        locales: Vec<LocaleTag>,
        locale_basis: LocaleBasis,
    ) -> Self {
        self.locales = locales;
        self.locale_basis = locale_basis;
        self
    }

    fn candidate_after_cue(&self, input: &str, cue_end: usize) -> Option<std::ops::Range<usize>> {
        let window = bounded_window_after(input, cue_end, self.right_window_chars);
        let window = if matches!(self.boundary, AnchoredBoundary::LineEnd) {
            window
                .split_once(['\n', '\r'])
                .map_or(window, |(line, _)| line)
        } else {
            window
        };
        let relative = is_person_name_candidate_with_min(window, self.min_components)?;
        Some((cue_end + relative.start)..(cue_end + relative.end))
    }

    fn candidate_before_cue(
        &self,
        input: &str,
        cue_start: usize,
    ) -> Option<std::ops::Range<usize>> {
        let window = bounded_window_before(input, cue_start, self.right_window_chars);
        let relative = last_person_name_candidate_with_min(window, self.min_components)?;
        let window_start = cue_start - window.len();
        Some((window_start + relative.start)..(window_start + relative.end))
    }
}

impl Recognizer for AnchoredMatchRecognizer {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }

    fn detect(
        &self,
        input: &str,
        _ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        if !matches!(self.name_shape, NameShape::PersonName) {
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        for cue in &self.folded_cues {
            for cue_range in find_cue_ranges(input, cue) {
                let span = match self.cue_position {
                    CuePosition::Before => self.candidate_after_cue(input, cue_range.end),
                    CuePosition::After => self.candidate_before_cue(input, cue_range.start),
                };
                let Some(span) = span else {
                    continue;
                };
                if !self.boundary_allows(input, &span) {
                    continue;
                }
                candidates.push(Candidate::new(
                    span,
                    PiiClass::Name,
                    self.id.clone(),
                    self.score,
                    self.priority,
                    None,
                    self.token_family.clone(),
                    self.source.clone(),
                    ConflictTier::None,
                    Vec::new(),
                ));
            }
        }
        candidates.sort_by_key(|candidate| candidate.span.start);
        candidates.dedup_by(|a, b| a.span == b.span);
        Ok(candidates)
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn locale_basis(&self) -> LocaleBasis {
        self.locale_basis
    }
}

impl AnchoredMatchRecognizer {
    fn boundary_allows(&self, input: &str, span: &std::ops::Range<usize>) -> bool {
        match self.boundary {
            AnchoredBoundary::Punctuation => input[span.end..]
                .chars()
                .find(|ch| !ch.is_whitespace())
                .is_none_or(|ch| matches!(ch, '.' | ',' | ':' | ';' | ')' | ']' | '>' | '\n')),
            AnchoredBoundary::Whitespace => input[span.end..]
                .chars()
                .next()
                .is_none_or(|ch| ch.is_whitespace() || matches!(ch, '.' | ',' | ':' | ';')),
            AnchoredBoundary::LineEnd => input[span.end..]
                .chars()
                .next()
                .is_none_or(|ch| ch == '\n' || ch == '\r'),
        }
    }
}

pub fn is_person_name_candidate(input: &str) -> Option<std::ops::Range<usize>> {
    is_person_name_candidate_with_min(input, 1)
}

fn is_person_name_candidate_with_min(
    input: &str,
    min_components: usize,
) -> Option<std::ops::Range<usize>> {
    for (start, _) in input.char_indices().filter(|(_, ch)| ch.is_uppercase()) {
        if let Some(span) = person_name_at(input, start, min_components) {
            return Some(span);
        }
    }
    None
}

fn last_person_name_candidate_with_min(
    input: &str,
    min_components: usize,
) -> Option<std::ops::Range<usize>> {
    input
        .char_indices()
        .filter(|(_, ch)| ch.is_uppercase())
        .filter_map(|(start, _)| person_name_at(input, start, min_components))
        .next_back()
}

fn person_name_at(
    input: &str,
    start: usize,
    min_components: usize,
) -> Option<std::ops::Range<usize>> {
    let mut end = start;
    let mut components = 0usize;
    let mut uppercase_components = 0usize;
    let mut pending_particles: Vec<(usize, usize)> = Vec::new();
    let mut cursor = start;

    while components < 4 {
        let Some((token_start, token_end)) = next_space_delimited(input, cursor) else {
            break;
        };
        if token_start != cursor && !input[cursor..token_start].chars().all(char::is_whitespace) {
            break;
        }
        let token = trim_trailing_boundary(&input[token_start..token_end]);
        let effective_end = token_start + token.len();
        if token.is_empty() {
            break;
        }

        if is_upper_component(token) {
            components += 1;
            uppercase_components += 1;
            pending_particles.clear();
            end = effective_end;
            cursor = token_end;
            continue;
        }

        if components > 0 && is_particle(token) {
            pending_particles.push((token_start, effective_end));
            components += 1;
            cursor = token_end;
            continue;
        }
        break;
    }

    if !pending_particles.is_empty() {
        end = pending_particles[0].0.saturating_sub(1);
        components = components.saturating_sub(pending_particles.len());
    }

    let candidate = &input[start..end];
    if has_organization_suffix(candidate) {
        return None;
    }
    (components >= min_components
        && uppercase_components > 0
        && candidate.len() <= 64
        && !candidate.is_empty())
    .then_some(start..end)
}

fn next_space_delimited(input: &str, cursor: usize) -> Option<(usize, usize)> {
    let start = input[cursor..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| cursor + offset)?;
    let end = input[start..]
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(input.len(), |(offset, _)| start + offset);
    Some((start, end))
}

fn trim_trailing_boundary(token: &str) -> &str {
    if token.len() == 2 {
        let mut chars = token.chars();
        if chars.next().is_some_and(|ch| ch.is_uppercase()) && chars.next() == Some('.') {
            return token;
        }
    }
    token.trim_end_matches(['.', ',', ':', ';', ')', ']', '>'])
}

fn is_upper_component(token: &str) -> bool {
    if token.len() == 2 {
        let mut chars = token.chars();
        if chars.next().is_some_and(|ch| ch.is_uppercase()) && chars.next() == Some('.') {
            return true;
        }
    }
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_uppercase()
        && !token.chars().all(|ch| ch.is_uppercase())
        && token.chars().all(|ch| {
            ch.is_alphabetic() || matches!(ch, '\'' | '-') || (ch == '.' && token.len() == 2)
        })
        && token
            .split(['\'', '-'])
            .all(|part| part.chars().next().is_some_and(|ch| ch.is_alphabetic()))
}

fn is_particle(token: &str) -> bool {
    matches!(token, "de" | "la" | "van" | "der")
}

fn has_organization_suffix(candidate: &str) -> bool {
    candidate
        .split_whitespace()
        .next_back()
        .is_some_and(|last| {
            matches!(
                trim_trailing_boundary(last),
                "Corp" | "Corporation" | "Inc" | "LLC" | "GmbH" | "AG"
            )
        })
}

/// Per-char case fold applied to both cues and input text.
///
/// `char::to_lowercase` is the same mapping `str::to_lowercase` applies,
/// minus its one context rule: a word-final Greek `Σ` becomes `ς` at the
/// string level but `σ` at the char level. Folding `ς` to `σ` on both sides
/// keeps that comparison position-independent instead of silently dropping
/// it.
fn fold_char(ch: char) -> impl Iterator<Item = char> {
    ch.to_lowercase().map(|c| if c == 'ς' { 'σ' } else { c })
}

/// Finds non-overlapping, word-bounded, case-insensitive occurrences of a
/// pre-folded cue and returns their ranges in `input`'s own byte space.
///
/// The scan only ever advances along `input.char_indices()`, so every offset
/// it returns is a char boundary of the ORIGINAL string. A lowercased copy
/// is not length-preserving (`İ` → `i̇` grows by one byte, `ẞ` → `ß` and the
/// Kelvin sign `K` → `k` shrink), so offsets taken from such a copy drift for
/// the rest of the document and can land inside a multi-byte char.
fn find_cue_ranges(input: &str, folded_cue: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    if folded_cue.is_empty() {
        return ranges;
    }
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some(end) = folded_prefix_end(input, cursor, folded_cue) {
            if is_boundary(input, cursor, true) && is_boundary(input, end, false) {
                ranges.push(cursor..end);
            }
            cursor = end;
        } else {
            cursor += input[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges
}

/// Returns the byte offset in `input` just past the run of chars starting at
/// `start` whose fold equals `folded_cue`, or `None` when it does not match.
///
/// A cue that ends inside one char's multi-char fold (`İ` folds to `i` +
/// U+0307) does not match, so the returned end always sits on a char
/// boundary of `input`.
fn folded_prefix_end(input: &str, start: usize, folded_cue: &str) -> Option<usize> {
    let mut expected = folded_cue.chars();
    for (offset, ch) in input[start..].char_indices() {
        for folded in fold_char(ch) {
            if expected.next() != Some(folded) {
                return None;
            }
        }
        if expected.as_str().is_empty() {
            return Some(start + offset + ch.len_utf8());
        }
    }
    None
}

fn is_boundary(input: &str, index: usize, before: bool) -> bool {
    let ch = if before {
        input[..index].chars().next_back()
    } else {
        input[index..].chars().next()
    };
    ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_' && ch != '-')
}

fn bounded_window_after(input: &str, start: usize, max_chars: u16) -> &str {
    let end = input[start..]
        .char_indices()
        .nth(max_chars as usize)
        .map_or(input.len(), |(offset, _)| start + offset);
    &input[start..end]
}

fn bounded_window_before(input: &str, end: usize, max_chars: u16) -> &str {
    let start = input[..end]
        .char_indices()
        .rev()
        .nth(max_chars as usize)
        .map_or(0, |(offset, _)| offset);
    &input[start..end]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use gaze_types::{DictionaryBundle, LocaleTag};

    use super::*;

    #[test]
    fn person_name_shape_accepts_required_matches() {
        for value in [
            "Łukasz Müller",
            "O'Brien",
            "Alice de la Cruz",
            "Jean-Paul Sartre",
            "Müller-Schmidt",
            "José María García",
        ] {
            assert!(
                is_person_name_candidate(value).is_some(),
                "{value} should match"
            );
        }
    }

    #[test]
    fn person_name_shape_rejects_required_non_matches() {
        for value in ["IBM", "mailgun", "the alice"] {
            assert!(
                is_person_name_candidate(value).is_none(),
                "{value} should not match"
            );
        }
        assert!(is_person_name_candidate_with_min("Acme Corp", 2).is_none());
    }

    #[test]
    fn detects_cue_followed_by_name_with_punctuation_boundary() {
        let recognizer = test_recognizer(
            "test.from_cue",
            vec!["from"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "from",
        );
        let hits = recognizer
            .detect("Hello from Alice Example, please reply.", &ctx())
            .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].span, 11..24);
        assert_eq!(hits[0].recognizer_id, "test.from_cue");
        assert_eq!(hits[0].source, "structural.from");
    }

    #[test]
    fn detects_line_end_and_whitespace_bounded_names() {
        let line_end = test_recognizer(
            "test.line",
            vec!["from"],
            AnchoredBoundary::LineEnd,
            CuePosition::Before,
            "from",
        );
        assert_eq!(
            line_end
                .detect("Forwarded from Alice Example\nBody", &ctx())
                .unwrap()[0]
                .span,
            15..28
        );

        let whitespace = test_recognizer(
            "test.ws",
            vec!["reply to"],
            AnchoredBoundary::Whitespace,
            CuePosition::Before,
            "agent_recipient",
        );
        assert_eq!(
            whitespace
                .detect("Please reply to Alice Example today", &ctx())
                .unwrap()[0]
                .span,
            16..29
        );
    }

    #[test]
    fn detects_cue_after_name_and_rejects_window_overflow() {
        let recognizer = test_recognizer(
            "test.after",
            vec!["sent"],
            AnchoredBoundary::Whitespace,
            CuePosition::After,
            "footer",
        );
        assert_eq!(
            recognizer
                .detect("Alice Example sent this", &ctx())
                .unwrap()[0]
                .span,
            0..13
        );

        let short = AnchoredMatchRecognizer::new(
            "test.short".to_string(),
            vec!["from".to_string()],
            AnchoredBoundary::Punctuation,
            4,
            NameShape::PersonName,
            CuePosition::Before,
            "from".to_string(),
            2,
            0.9,
            10,
        );
        assert!(short
            .detect("from Alice Example.", &ctx())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn min_components_is_explicit_not_source_label_derived() {
        let one_component_non_forward = AnchoredMatchRecognizer::new(
            "test.one_component".to_string(),
            vec!["from".to_string()],
            AnchoredBoundary::Punctuation,
            64,
            NameShape::PersonName,
            CuePosition::Before,
            "agent_recipient".to_string(),
            1,
            0.9,
            10,
        );
        assert_eq!(
            one_component_non_forward
                .detect("from Alice:", &ctx())
                .unwrap()[0]
                .span,
            5..10
        );

        let two_component_forward = AnchoredMatchRecognizer::new(
            "test.two_component".to_string(),
            vec!["from".to_string()],
            AnchoredBoundary::Punctuation,
            64,
            NameShape::PersonName,
            CuePosition::Before,
            "forward_marker".to_string(),
            2,
            0.9,
            10,
        );
        assert!(two_component_forward
            .detect("from Alice:", &ctx())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn handles_unicode_overlap_and_non_name_rejection() {
        let recognizer = test_recognizer(
            "test.forward",
            vec!["Forwarded", "Forwarded message from"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "forward_marker",
        );
        let hits = recognizer
            .detect("Forwarded message from Łukasz Müller:", &ctx())
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            &"Forwarded message from Łukasz Müller:"[hits[0].span.clone()],
            "Łukasz Müller"
        );

        assert!(recognizer
            .detect("Forwarded message from mailgun:", &ctx())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn emits_structural_source_labels_for_core_anchored_match_families() {
        for (short_label, cue, boundary, input, expected_source) in [
            (
                "forward_marker",
                "Forwarded message from",
                AnchoredBoundary::Punctuation,
                "Forwarded message from Alice Example:",
                "structural.forward_marker",
            ),
            (
                "agent_recipient",
                "antwortest",
                AnchoredBoundary::Punctuation,
                "Du antwortest als Artistfy-Support an Alice Example.",
                "structural.agent_recipient",
            ),
            (
                "footer",
                "Sent by",
                AnchoredBoundary::Whitespace,
                "Sent by Alice Example via Mailgun",
                "structural.footer",
            ),
        ] {
            let recognizer = test_recognizer(
                &format!("test.{short_label}"),
                vec![cue],
                boundary,
                CuePosition::Before,
                short_label,
            );
            let hits = recognizer.detect(input, &ctx()).unwrap();

            assert_eq!(
                hits.first().map(|hit| hit.source.as_str()),
                Some(expected_source),
                "{short_label} should persist the structural source label"
            );
        }
    }

    #[test]
    fn cue_offsets_survive_length_changing_case_folds_before_the_cue() {
        // `İ` (U+0130, 2 bytes) lowercases to `i̇` (`i` + U+0307, 3 bytes) and
        // `ẞ` (U+1E9E, 3 bytes) lowercases to `ß` (2 bytes). Every byte offset
        // after such a character differs between the original text and a
        // lowercased copy; the cue must still anchor the name in ORIGINAL
        // byte space.
        let recognizer = test_recognizer(
            "test.de.forward",
            vec!["Weitergeleitete Nachricht von"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "forward_marker",
        );
        for text in [
            "İstanbul Büro — Weitergeleitete Nachricht von Anna Müller:",
            "GRÜẞE AUS BERLIN — Weitergeleitete Nachricht von Anna Müller:",
            "\u{212A}elvin-Skala (K) — Weitergeleitete Nachricht von Anna Müller:",
        ] {
            let hits = recognizer.detect(text, &ctx()).unwrap();
            assert_eq!(
                hits.len(),
                1,
                "cue after a length-changing case fold must still anchor the name in {text:?}"
            );
            let expected = text.find("Anna Müller").unwrap();
            assert_eq!(hits[0].span, expected..expected + "Anna Müller".len());
            assert_eq!(&text[hits[0].span.clone()], "Anna Müller");
        }
    }

    #[test]
    fn cue_offsets_never_slice_inside_a_multi_byte_char() {
        // With `İ` shifting lowercased offsets by +1, a cue that starts with a
        // two-byte `Ü` puts the shifted start inside `Ü` in the original text.
        // Slicing there panics; the recognizer must instead return the exact
        // original span.
        let recognizer = test_recognizer(
            "test.de.uebermittelt",
            vec!["Übermittelt von"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "forward_marker",
        );
        let text = "İstanbul: Übermittelt von Anna Müller.";
        let hits = recognizer.detect(text, &ctx()).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "expected exactly one anchored name in {text:?}"
        );
        assert_eq!(hits[0].span, 28..40);
        assert_eq!(&text[hits[0].span.clone()], "Anna Müller");

        // Shrinking direction: `ẞ` shifts lowercased offsets by -1, so the
        // shifted cue start lands inside the em dash before the cue.
        let recognizer = test_recognizer(
            "test.de.forward",
            vec!["Weitergeleitete Nachricht von"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "forward_marker",
        );
        let text = "STRAẞE—Weitergeleitete Nachricht von Anna Müller:";
        let hits = recognizer.detect(text, &ctx()).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "expected exactly one anchored name in {text:?}"
        );
        assert_eq!(&text[hits[0].span.clone()], "Anna Müller");
    }

    #[test]
    fn ascii_cue_matching_is_byte_identical() {
        // Pins the pre-fix ASCII contract: case-insensitive cue, word-bounded,
        // non-overlapping cue occurrences, original byte offsets.
        let recognizer = test_recognizer(
            "test.from",
            vec!["From"],
            AnchoredBoundary::Punctuation,
            CuePosition::Before,
            "from",
        );
        for (text, expected) in [
            ("Hello from Alice Example, please reply.", 11..24),
            ("Hello FROM Alice Example, please reply.", 11..24),
            ("fromage from Alice Example.", 13..26),
            ("from from Alice Example.", 10..23),
        ] {
            let hits = recognizer.detect(text, &ctx()).unwrap();
            assert_eq!(hits.len(), 1, "{text:?}");
            assert_eq!(hits[0].span, expected, "{text:?}");
            assert_eq!(&text[hits[0].span.clone()], "Alice Example");
        }
        assert!(recognizer
            .detect("Hello fromAlice Example.", &ctx())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn find_cue_ranges_returns_original_byte_offsets_and_char_boundaries() {
        let folded = |cue: &str| cue.chars().flat_map(fold_char).collect::<String>();

        // Length-changing folds before the cue: offsets stay in original space.
        for (text, cue) in [
            ("İ von Anna", "von"),
            ("ẞ von Anna", "von"),
            ("\u{212A} von Anna", "von"),
            ("İẞ\u{212A} VON Anna", "von"),
        ] {
            let expected = text.find("von").or_else(|| text.find("VON")).unwrap();
            assert_eq!(
                find_cue_ranges(text, &folded(cue)),
                vec![expected..expected + 3],
                "{text:?}"
            );
        }

        // A cue whose fold ends inside one input char's multi-char fold does
        // not match, so no returned end can sit inside a char.
        assert!(find_cue_ranges("İ", &folded("i")).is_empty());
        assert!(find_cue_ranges("İ x", &folded("i")).is_empty());
        // The full fold of `İ` is `i` + U+0307, so a cue folded from `İ`
        // matches `İ` itself (either case) at its real byte range.
        assert_eq!(find_cue_ranges("x İ y", &folded("İ")), vec![2..4]);
        assert_eq!(find_cue_ranges("x i\u{307} y", &folded("İ")), vec![2..5]);

        // Greek final sigma: `str::to_lowercase` maps a word-final Σ to ς,
        // per-char folding maps it to σ; both directions must keep matching.
        assert_eq!(find_cue_ranges("ΛΌΓΟΣ Anna", &folded("λόγος")), vec![0..10]);
        assert_eq!(find_cue_ranges("λόγος Anna", &folded("ΛΌΓΟΣ")), vec![0..10]);

        // Non-overlapping, word-bounded occurrences (match_indices semantics).
        assert_eq!(
            find_cue_ranges("aa aa aaa", &folded("aa")),
            vec![0..2, 3..5]
        );
        assert_eq!(
            find_cue_ranges("aaaa", &folded("aa")),
            Vec::<std::ops::Range<usize>>::new()
        );
    }

    fn test_recognizer(
        id: &str,
        cues: Vec<&str>,
        boundary: AnchoredBoundary,
        cue_position: CuePosition,
        source_short_label: &str,
    ) -> AnchoredMatchRecognizer {
        AnchoredMatchRecognizer::new(
            id.to_string(),
            cues.into_iter().map(str::to_string).collect(),
            boundary,
            64,
            NameShape::PersonName,
            cue_position,
            source_short_label.to_string(),
            2,
            0.91,
            60,
        )
    }

    fn ctx() -> DetectContext<'static> {
        let dictionaries = Box::leak(Box::new(DictionaryBundle::from_entries(HashMap::new())));
        let locale_chain = Box::leak(Box::new([LocaleTag::Global]));
        DetectContext::new(locale_chain, dictionaries)
    }
}
