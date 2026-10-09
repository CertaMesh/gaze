use gaze_types::{
    Candidate, ConflictTier, DetectContext, Detection, Detector, LabelledValueScanReason,
    LocaleBasis, LocaleTag, PiiClass, Recognizer, ValidatorKind, ValidatorOnFail,
};
use regex::Regex;

use crate::{RecognizerError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NormalizerKind {
    EmailCanonical,
    IbanCanonical,
}

impl NormalizerKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "email_canonical" => Ok(Self::EmailCanonical),
            "iban_canonical" => Ok(Self::IbanCanonical),
            other => Err(RecognizerError::UnsupportedNormalizer {
                kind: other.to_string(),
            }),
        }
    }

    pub fn normalize(self, input: &str) -> String {
        match self {
            Self::EmailCanonical => input.to_ascii_lowercase(),
            Self::IbanCanonical => iban_canonicalize(input),
        }
    }
}

/// Regex-backed [`Recognizer`] implementation.
///
/// Construct via [`RegexDetector::emails`] for the bundled email recognizer, or
/// supply a custom pattern through the rulepack mechanism. Patterns use Rust
/// regex syntax: no lookahead, lookbehind, or backreferences. Prefer TOML
/// literal strings (`'...'`) in policy files to avoid double-escaping.
///
/// [`Candidate::span`] uses byte ranges, not char indices.
///
/// [`Candidate::span`]: gaze_types::Candidate::span
pub struct RegexDetector {
    regex: Regex,
    class: PiiClass,
    source: String,
    locales: Vec<LocaleTag>,
    locale_basis: LocaleBasis,
    base_score: f32,
    priority: i32,
    token_family: String,
    capture_groups: Option<Vec<u32>>,
    complete_labelled_value: bool,
    exclusions: Vec<String>,
    reject_match_regex: Option<Regex>,
    validator_kind: Option<ValidatorKind>,
    validator_on_fail: ValidatorOnFail,
    normalizer_kind: Option<NormalizerKind>,
    ascii_email_boundary: bool,
    /// The candidate must not be a prefix of a longer identifier: the word run after it may hold
    /// letters only (`gaze_types::word_run_extends_identifier`). Set for `iban_mod97`, whose
    /// pattern carries no trailing `\b` so a compact IBAN glued to a label is still a candidate.
    identifier_run_boundary: bool,
    /// Each match is a digit run that may hold a payment card among other digits
    /// (`gaze_types::payment_card::scan_card_run`). Set for a `luhn` recognizer whose pattern is
    /// `gaze_types::payment_card::CARD_RUN_PATTERN`, as `card.structural` is.
    card_runs: bool,
}

impl RegexDetector {
    pub fn new(pattern: &str, class: PiiClass) -> Result<Self> {
        Self::with_source(pattern, class, "regex")
    }

    pub fn with_source(pattern: &str, class: PiiClass, source: &str) -> Result<Self> {
        Self::with_rulepack_fields(
            pattern,
            class,
            source,
            vec![LocaleTag::Global],
            0.70,
            0,
            "counter",
            None,
            Vec::new(),
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_rulepack_fields(
        pattern: &str,
        class: PiiClass,
        source: &str,
        locales: Vec<LocaleTag>,
        base_score: f32,
        priority: i32,
        token_family: &str,
        capture_groups: Option<Vec<u32>>,
        exclusions: Vec<String>,
        validator_kind: Option<ValidatorKind>,
        normalizer_kind: Option<NormalizerKind>,
    ) -> Result<Self> {
        let regex = Regex::new(pattern).map_err(RecognizerError::InvalidRegex)?;
        let ascii_email_boundary = class == PiiClass::Email && source == "email.global";
        let identifier_run_boundary = validator_kind == Some(ValidatorKind::IbanMod97);
        let card_runs = validator_kind == Some(ValidatorKind::Luhn)
            && pattern == gaze_types::payment_card::CARD_RUN_PATTERN
            && capture_groups.is_none();

        Ok(Self {
            regex,
            class,
            source: source.to_string(),
            locales,
            locale_basis: LocaleBasis::Document,
            base_score,
            priority,
            token_family: token_family.to_string(),
            capture_groups,
            complete_labelled_value: false,
            exclusions: exclusions
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            reject_match_regex: None,
            validator_kind,
            validator_on_fail: ValidatorOnFail::Veto,
            normalizer_kind,
            ascii_email_boundary,
            identifier_run_boundary,
            card_runs,
        })
    }

    pub fn emails() -> Result<Self> {
        let mut detector = Self::new(
            r"(?i)[a-z0-9_][a-z0-9._%+\-]*@[a-z0-9.\-]+\.[a-z]{2,}",
            PiiClass::Email,
        )?;
        detector.ascii_email_boundary = true;
        Ok(detector)
    }

    /// Overrides how the recognizer's locale metadata affects eligibility.
    pub fn with_locale_basis(mut self, locale_basis: LocaleBasis) -> Self {
        self.locale_basis = locale_basis;
        self
    }

    /// Overrides the confidence every candidate is emitted with.
    ///
    /// The conflict ladder compares score after class and rule priority, so a
    /// recognizer registered through this type competes at this value.
    pub fn with_base_score(mut self, base_score: f32) -> Self {
        self.base_score = base_score;
        self
    }
}

impl Detector for RegexDetector {
    /// A regex match is rule evidence.
    fn evidence(&self) -> gaze_types::EvidenceKind {
        gaze_types::EvidenceKind::Rule
    }

    fn detect(&self, input: &str) -> Vec<Detection> {
        self.spans(input, None)
            .into_iter()
            .map(|span| Detection::new(span, self.class.clone(), self.source.clone()))
            .collect()
    }
}

impl Recognizer for RegexDetector {
    /// A regex match is rule evidence.
    fn evidence(&self) -> gaze_types::EvidenceKind {
        gaze_types::EvidenceKind::Rule
    }

    fn id(&self) -> &str {
        &self.source
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .candidates(input, ctx.source_spans)
            .into_iter()
            .filter(|candidate| !candidate.regex_guard_rejected)
            .collect())
    }

    fn detect_for_registry(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self.candidates(input, ctx.source_spans))
    }

    fn token_family(&self) -> &str {
        &self.token_family
    }

    fn validator_kind(&self) -> Option<ValidatorKind> {
        self.validator_kind
    }

    fn validator_on_fail(&self) -> ValidatorOnFail {
        self.validator_on_fail
    }

    fn locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn locale_basis(&self) -> LocaleBasis {
        self.locale_basis
    }

    // `detect` ignores its context: the pattern and its filters are fixed at build time.
    fn detect_is_locale_invariant(&self) -> bool {
        true
    }
}

impl RegexDetector {
    fn candidates(&self, input: &str, source_spans: Option<&[(usize, usize)]>) -> Vec<Candidate> {
        self.scanned_spans(input, source_spans)
            .into_iter()
            .filter_map(|scan| {
                let matched = &input[scan.span.clone()];
                (!self.is_excluded(matched)).then_some((scan, matched))
            })
            .map(|(scan, matched)| {
                let canonical_form = self.canonical_form(matched);
                let mut candidate = Candidate::new(
                    scan.span,
                    self.class.clone(),
                    self.source.clone(),
                    self.base_score,
                    self.priority,
                    canonical_form,
                    self.token_family(),
                    self.source.clone(),
                    ConflictTier::None,
                    Vec::new(),
                );
                candidate.labelled_value_scan_reason = scan.reason;
                candidate.labelled_value_capture_end = (self.complete_labelled_value
                    || (self.phone_validator_enabled()
                        && self.validator_on_fail == ValidatorOnFail::Record))
                    .then_some(scan.extension.start);
                candidate.regex_guard_rejected = scan.rejected;
                candidate
            })
            .collect()
    }
}

impl RegexDetector {
    /// What validator veto does when this recognizer's validator fails. `Record` is accepted
    /// only for eligible validators (`ValidatorKind::allows_recorded_failure`), and never on a
    /// card-run recognizer: it offers every digit run in the text, so keeping the ones that fail
    /// Luhn would tokenize every long number. Core rules opt in only when the candidate shape
    /// and label make a failed value credible.
    pub fn with_validator_on_fail(mut self, on_fail: ValidatorOnFail) -> Result<Self> {
        if on_fail == ValidatorOnFail::Record {
            let refuse = |kind: String, reason| RecognizerError::UnsupportedValidatorOnFail {
                recognizer_id: self.source.clone(),
                kind,
                reason,
            };
            let Some(kind) = self.validator_kind else {
                return Err(refuse("none".into(), "the recognizer has no validator"));
            };
            if !kind.allows_recorded_failure() {
                return Err(refuse(
                    format!("{kind:?}"),
                    "this validator cannot keep a failed candidate",
                ));
            }
            if self.card_runs {
                return Err(refuse(
                    format!("{kind:?}"),
                    "a card-run recognizer offers every digit run; anchor the card with a cue",
                ));
            }
        }
        self.validator_on_fail = on_fail;
        Ok(self)
    }

    /// A rulepack guard can refuse a full regex match before its capture is emitted.
    pub fn with_rejection_pattern(mut self, match_pattern: Option<&str>) -> Result<Self> {
        self.reject_match_regex = match_pattern
            .map(Regex::new)
            .transpose()
            .map_err(RecognizerError::InvalidRegex)?;
        Ok(self)
    }

    /// Extend the captured value through adjacent identifier-like groups. This closes the gap
    /// where a bounded regex can otherwise emit only a prefix of a labelled value.
    pub fn with_complete_labelled_value(mut self, enabled: bool) -> Self {
        self.complete_labelled_value = enabled;
        self
    }

    /// The candidate spans in `input`: pattern matches that pass the boundary checks, or for a
    /// card-run recognizer the cards in each run plus the Luhn-failing pattern windows that hold
    /// none, so validator veto still records those.
    fn spans(
        &self,
        input: &str,
        source_spans: Option<&[(usize, usize)]>,
    ) -> Vec<std::ops::Range<usize>> {
        self.scanned_spans(input, source_spans)
            .into_iter()
            .filter(|scan| !scan.rejected)
            .map(|scan| scan.span)
            .collect()
    }

    fn scanned_spans(
        &self,
        input: &str,
        source_spans: Option<&[(usize, usize)]>,
    ) -> Vec<LabelledValueScan> {
        let mut search_at = Some(0);
        let matches = std::iter::from_fn(|| {
            while let Some(at) = search_at {
                let caps = self.regex.captures_at(input, at)?;
                let full = caps.get(0)?;
                let captured = self.span_from_captures(&caps);
                let phone_parts = captured
                    .as_ref()
                    .filter(|_| self.phone_validator_enabled())
                    .map(|span| self.phone_parts(input, span.clone(), full.start()));
                let captured = phone_parts
                    .as_ref()
                    .and_then(|parts| Some(parts.first()?.start..parts.last()?.end))
                    .or(captured);
                let span = captured.clone().map(|span| {
                    if self.complete_labelled_value {
                        // Only these new fallbacks may trim a field cue inside their broad
                        // capture. Existing cue rules must retain every originally matched byte.
                        let trim_internal_field_boundary = matches!(
                            self.source.as_str(),
                            "tax_number.labelled" | "driver_license.labelled" | "id_card.labelled"
                        );
                        scan_labelled_value(input, span, trim_internal_field_boundary)
                    } else {
                        LabelledValueScan {
                            capture: span.clone(),
                            extension: span.clone(),
                            span,
                            reason: None,
                            rejected: false,
                        }
                    }
                });
                // Resume at the capture end. The regex may have consumed the separator needed
                // by the next match, and the scanner may have passed another labelled field.
                let next = span.as_ref().map_or(full.end(), |scan| scan.span.end);
                search_at = if next > full.start() {
                    Some(next)
                } else {
                    input[full.start()..]
                        .chars()
                        .next()
                        .map(|ch| full.start() + ch.len_utf8())
                };
                if let Some(mut span) = span.filter(|scan| self.boundary_accepts(input, &scan.span))
                {
                    // The guard judges only the regex evidence, never groups found later by the
                    // scanner. A rejected capture still reaches the audit veto path.
                    span.rejected = self.reject_match_regex.as_ref().is_some_and(|guard| {
                        let checked = if self.complete_labelled_value {
                            &input[full.start()..span.capture.end]
                        } else {
                            full.as_str()
                        };
                        guard.is_match(checked)
                    });
                    span.rejected |= self.ipv4_phone_tail(input, &span.span);
                    return Some(match phone_parts {
                        None => vec![span],
                        Some(parts) => parts
                            .into_iter()
                            .map(|part| LabelledValueScan {
                                capture: part.clone(),
                                extension: part.clone(),
                                span: part,
                                reason: span.reason,
                                rejected: span.rejected,
                            })
                            .collect::<Vec<_>>(),
                    });
                }
            }
            None
        })
        .flatten();
        if !self.card_runs {
            return matches.collect();
        }
        matches
            .flat_map(|run| {
                let scan = gaze_types::payment_card::scan_card_run(input, run.span, source_spans);
                let mut spans = scan.cards;
                spans.extend(scan.rejected);
                spans.sort_by_key(|span| span.start);
                spans
                    .into_iter()
                    .map(|span| LabelledValueScan {
                        capture: span.clone(),
                        extension: span.clone(),
                        span,
                        reason: None,
                        rejected: false,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn phone_validator_enabled(&self) -> bool {
        #[cfg(feature = "phone-parser")]
        {
            matches!(
                self.validator_kind,
                Some(
                    ValidatorKind::E164Phone
                        | ValidatorKind::E164PhoneNational(_)
                        | ValidatorKind::PhoneNumber
                )
            )
        }
        #[cfg(not(feature = "phone-parser"))]
        {
            false
        }
    }

    /// A bounded regex is candidate evidence, never permission to leave a labelled suffix raw.
    /// Split independently valid values under the original cue; otherwise a recording rule
    /// protects the complete run. Veto rules can recover a valid prefix before a malformed tail.
    #[cfg(feature = "phone-parser")]
    fn phone_parts(
        &self,
        input: &str,
        span: std::ops::Range<usize>,
        full_start: usize,
    ) -> Vec<std::ops::Range<usize>> {
        let Some(
            kind @ (ValidatorKind::E164Phone
            | ValidatorKind::E164PhoneNational(_)
            | ValidatorKind::PhoneNumber),
        ) = self.validator_kind
        else {
            return vec![span];
        };
        let records = self.validator_on_fail == ValidatorOnFail::Record;
        if !records && kind.validates(&input[span.clone()]) {
            return vec![span];
        }
        let run_end = input[span.start..]
            .char_indices()
            .take_while(|(at, ch)| {
                (records || *at < span.len().max(128))
                    && (ch.is_ascii_digit()
                        || (ch.is_whitespace() && (!records || !matches!(ch, '\n' | '\r')))
                        || matches!(ch, '+' | '-' | '/' | '.' | '(' | ')'))
            })
            .last()
            .map_or(span.end, |(at, ch)| span.start + at + ch.len_utf8());
        let run = input[span.start..run_end].trim_end_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '/' | '.' | '-' | '(' | ')')
        });
        if records && kind.validates(run) {
            return std::iter::once(span.start..span.start + run.len()).collect();
        }
        // Recorded runs of any size stay protected. Partition work itself is bounded;
        // veto rules resume after a complete validated prefix of the bounded window.
        let run = if run.len() > 128 {
            if records {
                return std::iter::once(span.start..span.start + run.len()).collect();
            }
            let end = run
                .char_indices()
                .take_while(|(at, _)| *at <= 128)
                .last()
                .map_or(0, |(at, _)| at);
            &run[..end]
        } else {
            run
        };
        let prefix = &input[full_start..span.start];
        let is_value = |piece: &str| {
            if piece.bytes().filter(u8::is_ascii_digit).count() > 17 {
                return false;
            }
            let evidence = format!("{prefix}{piece}");
            self.regex
                .captures(&evidence)
                .and_then(|caps| self.span_from_captures(&caps))
                .is_some_and(|matched| {
                    matched.start == prefix.len() && matched.end == evidence.len()
                })
                && kind.validates(piece)
        };
        let mut boundaries = vec![0];
        boundaries.extend(run.char_indices().filter_map(|(at, ch)| {
            (ch.is_whitespace() || ch == '/').then_some(at + ch.len_utf8())
        }));
        boundaries.push(run.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut paths: Vec<Option<Vec<std::ops::Range<usize>>>> = vec![None; boundaries.len()];
        for (index, &at) in boundaries.iter().enumerate() {
            if at == run.len() || (!records && at >= span.len()) {
                paths[index] = Some(Vec::new());
            }
        }
        for start in (0..boundaries.len() - 1).rev() {
            for end in start + 1..boundaries.len() {
                let Some(suffix) = &paths[end] else {
                    continue;
                };
                let raw = &run[boundaries[start]..boundaries[end]];
                let piece = raw.trim_matches(|ch: char| ch.is_whitespace() || ch == '/');
                if !is_value(piece) {
                    continue;
                }
                let leading = raw.len()
                    - raw
                        .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '/')
                        .len();
                let part_start = span.start + boundaries[start] + leading;
                let mut path: Vec<_> =
                    std::iter::once(part_start..part_start + piece.len()).collect();
                path.extend(suffix.iter().cloned());
                paths[start] = Some(path);
                break;
            }
        }
        if let Some(path) = paths[0].take().filter(|path| !path.is_empty()) {
            return path;
        }
        if records {
            return std::iter::once(span.start..span.start + run.len()).collect();
        }
        // The extension can be malformed without invalidating a complete preceding number.
        for &end in boundaries.iter().rev() {
            let piece = run[..end].trim_end_matches(|ch: char| ch.is_whitespace() || ch == '/');
            if is_value(piece) {
                return std::iter::once(span.start..span.start + piece.len()).collect();
            }
        }
        vec![span]
    }

    #[cfg(not(feature = "phone-parser"))]
    fn phone_parts(
        &self,
        _: &str,
        span: std::ops::Range<usize>,
        _: usize,
    ) -> Vec<std::ops::Range<usize>> {
        vec![span]
    }

    /// Only a complete French dotted pair shape suppresses a competing IPv4 tail.
    /// The rejected candidate still reaches the registry's audit veto path.
    fn ipv4_phone_tail(&self, input: &str, span: &std::ops::Range<usize>) -> bool {
        if !matches!(
            self.validator_kind,
            Some(ValidatorKind::Ipv4Parse | ValidatorKind::Ipv4ParseNonDocumentation)
        ) {
            return false;
        }
        let start = input[..span.start]
            .bytes()
            .rev()
            .take_while(|ch| ch.is_ascii_digit() || *ch == b'.')
            .count();
        let end = input[span.end..]
            .bytes()
            .take_while(|ch| ch.is_ascii_digit() || *ch == b'.')
            .count();
        let groups = input[span.start - start..span.end + end]
            .trim_matches('.')
            .split('.')
            .collect::<Vec<_>>();
        groups.len() == 5
            && groups
                .iter()
                .all(|group| group.len() == 2 && group.bytes().all(|ch| ch.is_ascii_digit()))
            && groups[0].starts_with('0')
            && groups[0].as_bytes()[1] != b'0'
    }

    fn is_excluded(&self, matched: &str) -> bool {
        if self.exclusions.is_empty() {
            return false;
        }
        let lowered = matched.to_ascii_lowercase();
        self.exclusions
            .iter()
            .any(|excluded| lowered.contains(excluded))
    }

    fn canonical_form(&self, matched: &str) -> Option<String> {
        match self.validator_kind {
            #[cfg(feature = "phone-parser")]
            Some(ValidatorKind::E164PhoneNational(_)) => {
                self.validator_kind?.canonical_form(matched)
            }
            Some(validator_kind) if validator_kind.validates(matched) => {
                Some(self.normalizer_kind.map_or_else(
                    || matched.to_string(),
                    |normalizer| normalizer.normalize(matched),
                ))
            }
            Some(_) => None,
            None => None,
        }
    }

    fn span_from_captures(&self, caps: &regex::Captures<'_>) -> Option<std::ops::Range<usize>> {
        if let Some(groups) = &self.capture_groups {
            groups
                .iter()
                .filter_map(|group| caps.get(*group as usize))
                .find(|m| !m.as_str().is_empty())
                .map(|m| m.range())
        } else {
            caps.get(0).map(|m| m.range())
        }
    }

    fn boundary_accepts(&self, input: &str, span: &std::ops::Range<usize>) -> bool {
        if matches!(self.source.as_str(), "postal.de" | "postal.us")
            && has_sku_identifier_prefix(input, span.start)
        {
            return false;
        }
        if self.identifier_run_boundary && gaze_types::word_run_extends_identifier(input, span.end)
        {
            return false;
        }
        if !self.ascii_email_boundary {
            return true;
        }

        let previous_ok = input[..span.start]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_ascii_email_continuation(ch));
        let next_ok = input[span.end..]
            .chars()
            .next()
            .is_none_or(|ch| !is_ascii_email_continuation(ch));

        previous_ok && next_ok
    }
}

/// `SKU-` plus an alphabetic product component identifies a stock-keeping unit.
/// A numeric-only suffix is ambiguous and keeps postal detection.
/// Check the complete connected prefix, so country prefixes and hyphenated
/// place names remain eligible. Adopter recognizers can still protect the SKU
/// itself when inventory identifiers are part of their PII contract.
fn has_sku_identifier_prefix(input: &str, start: usize) -> bool {
    let before = &input[..start];
    // Bound work per match. If a connected prefix is too long to establish its
    // namespace, retain the detection rather than suppress uncertain evidence.
    const MAX_PREFIX_CHARS: usize = 256;
    let mut prefix_start = 0;
    for (count, (at, ch)) in before.char_indices().rev().enumerate() {
        if !ch.is_alphanumeric() && !matches!(ch, '-' | '_') {
            prefix_start = at + ch.len_utf8();
            break;
        }
        if count >= MAX_PREFIX_CHARS {
            return false;
        }
    }
    let prefix = &before[prefix_start..];
    prefix
        .get(..4)
        .is_some_and(|tag| tag.eq_ignore_ascii_case("sku-"))
        && prefix
            .get(4..)
            .is_some_and(|body| body.chars().any(char::is_alphabetic))
}

/// A regex capture proves the first group. Scan the rest as a value run, stopping at the first
/// prose token or field delimiter. Limits are audit signals, never a reason to leave PII raw.
struct LabelledValueScan {
    capture: std::ops::Range<usize>,
    extension: std::ops::Range<usize>,
    span: std::ops::Range<usize>,
    reason: Option<LabelledValueScanReason>,
    rejected: bool,
}

fn scan_labelled_value(
    input: &str,
    capture: std::ops::Range<usize>,
    trim_internal_field_boundary: bool,
) -> LabelledValueScan {
    let mut end = capture.start;
    let mut stop_reason = None;
    let initial_lowercase = input[capture.start..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase());
    loop {
        let group_start = end;
        end += input[end..]
            .bytes()
            .take_while(u8::is_ascii_alphanumeric)
            .count();
        if end == group_start {
            break;
        }
        let separator_start = end;
        let rest = &input[end..];
        // Zero-width space can be inserted inside a copied identifier without a visible break.
        let separator_len = rest
            .char_indices()
            .take_while(|(_, ch)| {
                matches!(
                    ch,
                    ' ' | '\u{00A0}'
                        | '\u{202F}'
                        | '\u{200B}'
                        | '\u{2060}'
                        | '\u{FEFF}'
                        | '\u{200F}'
                        | '.'
                        | '/'
                        | '_'
                ) || gaze_types::LABELLED_FIELD_CONNECTORS.contains(ch)
                    || ('\u{0300}'..='\u{036F}').contains(ch)
            })
            .last()
            .map_or(0, |(at, ch)| at + ch.len_utf8());
        if separator_len == 0 {
            break;
        }
        let next_start = end + separator_len;
        let next_len = input[next_start..]
            .bytes()
            .take_while(u8::is_ascii_alphanumeric)
            .count();
        if next_len == 0 {
            break;
        }
        let next = &input[next_start..next_start + next_len];
        // Dates never cut a proven capture. A following uppercase field stays covered until
        // another recognizer actually claims its value; the registry then exposes the label.
        let beyond_capture = next_start >= capture.end;
        if beyond_capture && starts_with_date(&input[next_start..]) {
            stop_reason = Some(LabelledValueScanReason::DateBoundary);
            break;
        }
        let uppercase_label = is_uppercase_field_boundary(&input[next_start..])
            || is_uppercase_field_boundary(&input[group_start..]);
        if (beyond_capture || trim_internal_field_boundary)
            && is_field_boundary(next)
            && !uppercase_label
        {
            stop_reason = Some(LabelledValueScanReason::LabelBoundary);
            break;
        }
        let value_like = next.bytes().any(|byte| byte.is_ascii_digit())
            || next.bytes().all(|byte| byte.is_ascii_uppercase())
            || (initial_lowercase
                && next_len <= 2
                && next.bytes().all(|byte| byte.is_ascii_lowercase()));
        if !value_like {
            break;
        }
        end = next_start;
        debug_assert!(end > separator_start);
    }
    if stop_reason.is_none() {
        end = end.max(capture.end);
    }
    let capture = capture.start..capture.end.min(end);
    // Strict restore treats an angle bracket immediately beside a token as a malformed
    // nested token. Keep adjacent wrapper brackets in the same reversible value span.
    let start = if capture.start > 0 && input.as_bytes()[capture.start - 1] == b'<' {
        capture.start - 1
    } else {
        capture.start
    };
    end += input[end..]
        .bytes()
        .take_while(|byte| *byte == b'>')
        .count();
    let extension = capture.end..end;
    let span = start..end;
    LabelledValueScan {
        reason: labelled_value_over_limit(&input[span.clone()])
            .then_some(LabelledValueScanReason::LimitExceeded)
            .or(stop_reason),
        capture,
        extension,
        span,
        rejected: false,
    }
}

fn starts_with_date(rest: &str) -> bool {
    static DATE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    DATE.get_or_init(|| {
        Regex::new(r"\A(?:(?:19|20)\d{2}[-/.](?:0?[1-9]|1[0-2])[-/.](?:0?[1-9]|[12]\d|3[01])|(?:0?[1-9]|[12]\d|3[01])[-/.](?:0?[1-9]|1[0-2])[-/.](?:19|20)\d{2})(?:\b|$)").expect("static date pattern")
    }).is_match(rest)
}

fn is_field_boundary(group: &str) -> bool {
    // These short field cues occur in the core rulepack's locale cue vocabulary.
    // US/UK alone do not prove another recognizer will protect the following digits.
    matches!(
        group.to_ascii_uppercase().as_str(),
        "SSN" | "TIN" | "DOB" | "ID" | "TAX" | "UTR" | "TFN" | "EXP" | "VALID"
    )
}

fn is_uppercase_field_boundary(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    let mut at = 0;
    for _ in 0..4 {
        let word_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_uppercase) {
            at += 1;
        }
        if at - word_start < 2 {
            return false;
        }
        let spaces_start = at;
        while bytes.get(at) == Some(&b' ') {
            at += 1;
        }
        if bytes
            .get(at)
            .is_some_and(|byte| gaze_types::LABELLED_FIELD_CONNECTORS.contains(&(*byte as char)))
        {
            return true;
        }
        if at == spaces_start {
            return false;
        }
    }
    false
}

fn labelled_value_over_limit(value: &str) -> bool {
    value.len() > 40
        || value
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|group| !group.is_empty())
            .count()
            > 4
}

fn iban_canonicalize(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn is_ascii_email_continuation(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_postal_rules_ignore_sku_identifier_suffixes() {
        for (id, pattern) in [
            ("postal.de", r"\b\d{5}\b"),
            ("postal.us", r"\b\d{5}(-\d{4})?\b"),
        ] {
            let detector =
                RegexDetector::with_source(pattern, PiiClass::custom("postal_code").unwrap(), id)
                    .unwrap();
            for text in [
                "SKU-WIDGET-54321",
                "sku-widget-54321",
                "SKU-SECTION-ITEM-54321",
                "SKU-ÄNDERUNG-54321",
                "(SKU-WIDGET-54321)",
                "SKU-PART12-54321-6789",
            ] {
                assert!(detector.spans(text, None).is_empty(), "{id}: {text}");
            }
        }
    }

    #[test]
    fn numeric_postal_rules_keep_country_address_and_label_prefixes() {
        for id in ["postal.de", "postal.us"] {
            let detector = RegexDetector::with_source(
                r"\b\d{5}\b",
                PiiClass::custom("postal_code").unwrap(),
                id,
            )
            .unwrap();
            for text in [
                "54321 Musterstadt",
                "DE-54321 Musterstadt",
                "US-54321",
                "12-A-54321 Musterstadt",
                "Musterweg-12-A-54321 Musterstadt",
                "postal-code-54321",
                "POST-CODE-54321",
                "ZIP-CODE-54321",
                "SKU: 54321",
                "SKU-54321",
                "SKU-123-54321",
                "SKU-54321\n\nDelivery ZIP above.",
                "Muster-Stadt-54321",
                "INVENTORY-PART-54321",
                "ÄNDERUNG-ARTIKEL-54321",
            ] {
                let start = text.find("54321").unwrap();
                assert_eq!(
                    detector.spans(text, None),
                    vec![start..start + 5],
                    "{id}: {text}"
                );
            }
        }
        let us = RegexDetector::with_source(
            r"\b\d{5}(-\d{4})?\b",
            PiiClass::custom("postal_code").unwrap(),
            "postal.us",
        )
        .unwrap();
        assert_eq!(us.spans("US-54321-6789", None), vec![3..13]);
    }

    #[test]
    fn sku_postal_boundary_does_not_change_adopter_regexes() {
        let detector = RegexDetector::with_source(
            r"\b\d{5}\b",
            PiiClass::custom("part_number").unwrap(),
            "adopter.inventory",
        )
        .unwrap();
        assert_eq!(detector.spans("SKU-WIDGET-54321", None), vec![11..16]);
    }

    #[test]
    fn sku_prefix_scan_keeps_detection_when_its_work_budget_is_exhausted() {
        let text = format!("SKU-{}-54321", "a".repeat(300));
        assert!(!has_sku_identifier_prefix(&text, text.len() - 5));
        let text = format!("SKU-{}-54321", "ä".repeat(300));
        assert!(!has_sku_identifier_prefix(&text, text.len() - 5));
    }

    #[test]
    fn scanner_never_cuts_a_proven_capture_at_any_internal_boundary() {
        for (input, captured) in [
            ("181/12/03/2019 bitte", "181/12/03/2019"),
            ("AB12 SSN 12345", "AB12 SSN"),
            ("AB12 DRIVER LICENSE: EF34", "AB12 DRIVER LICENSE"),
            ("AB12 verified", "AB12 verified"),
        ] {
            let scan = scan_labelled_value(input, 0..captured.len(), false);
            assert_eq!(&input[scan.capture], captured, "{input:?}");
            assert!(scan.span.end >= captured.len(), "{input:?}");
        }
    }

    #[test]
    fn uppercase_field_boundary_waits_for_a_verified_next_value() {
        for separator in gaze_types::LABELLED_FIELD_CONNECTORS {
            let input = format!("AB12 CD3456 DRIVER LICENSE{separator} EF34 GH5678");
            let scan = scan_labelled_value(&input, 0..11, true);
            assert!(is_uppercase_field_boundary(&input["AB12 CD3456 ".len()..]));
            assert_eq!(&input[scan.span], input, "{input:?}");
        }
        let input = "AB12 CD3456 XYZ123456";
        assert!(!is_uppercase_field_boundary(&input["AB12 CD3456 ".len()..]));
        let scan = scan_labelled_value(input, 0..11, true);
        assert_eq!(&input[scan.span], input);
    }

    #[test]
    fn rejection_guard_checks_capture_instead_of_scanner_extension() {
        let detector = RegexDetector::with_rulepack_fields(
            r"Tax number: ([A-Z0-9]+ [A-Z0-9]+)",
            PiiClass::custom("tax_number").unwrap(),
            "synthetic.labelled",
            vec![LocaleTag::Global],
            0.84,
            84,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap()
        .with_complete_labelled_value(true)
        .with_rejection_pattern(Some("XYZ123456"))
        .unwrap();
        let input = "Tax number: AB12 CD3456 XYZ123456";
        let scans = detector.scanned_spans(input, None);
        assert_eq!(scans.len(), 1);
        assert_eq!(&input[scans[0].capture.clone()], "AB12 CD3456");
        assert_eq!(&input[scans[0].span.clone()], "AB12 CD3456 XYZ123456");
        assert!(!scans[0].rejected, "guard must not inspect the extension");
    }

    #[test]
    fn direct_detection_hides_guard_vetoes_but_registry_can_audit_them() {
        let detector = RegexDetector::with_rulepack_fields(
            r"Tax number: ([A-Z0-9]+)",
            PiiClass::custom("tax_number").unwrap(),
            "synthetic.labelled",
            vec![LocaleTag::Global],
            0.84,
            84,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap()
        .with_rejection_pattern(Some("AB123456"))
        .unwrap();
        let dictionaries = gaze_types::DictionaryBundle::default();
        let locales = [LocaleTag::Global];
        let ctx = DetectContext::new(&locales, &dictionaries);
        assert!(Recognizer::detect(&detector, "Tax number: AB123456", &ctx)
            .unwrap()
            .is_empty());
        let audit =
            Recognizer::detect_for_registry(&detector, "Tax number: AB123456", &ctx).unwrap();
        assert_eq!(audit.len(), 1);
        assert!(audit[0].regex_guard_rejected);
    }

    #[test]
    fn captured_values_reuse_a_consumed_separator_for_the_next_match() {
        let detector = RegexDetector::with_rulepack_fields(
            r"(?:^|[^[:alnum:]])(id\d+)(?:$|[^[:alnum:]])",
            PiiClass::custom("synthetic_id").unwrap(),
            "synthetic.boundary",
            vec![LocaleTag::Global],
            0.7,
            0,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(detector.spans("id1 id2 id3", None), vec![0..3, 4..7, 8..11]);
    }

    #[test]
    fn captured_values_reuse_a_two_character_suffix_guard() {
        let detector = RegexDetector::with_rulepack_fields(
            r"(?:^|[^[:alnum:]])(id\d+)(?:$|\.(?:$|[^0-9]))",
            PiiClass::custom("synthetic_id").unwrap(),
            "synthetic.boundary",
            vec![LocaleTag::Global],
            0.7,
            0,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(detector.spans("id1. id2.", None), vec![0..3, 5..8]);
    }

    #[test]
    fn zero_width_matches_advance_through_the_input_and_stop_at_the_end() {
        let detector = RegexDetector::with_source(
            r"\b",
            PiiClass::custom("synthetic_id").unwrap(),
            "synthetic.boundary",
        )
        .unwrap();
        assert_eq!(detector.spans("a b", None), vec![0..0, 1..1, 2..2, 3..3]);
    }

    #[test]
    fn bundled_email_detector_matches_before_non_ascii_letter() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalidø");

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].span, 0..17);
    }

    #[test]
    fn bundled_email_detector_matches_after_non_ascii_letter() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "øa@example.invalid");

        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].span, 2..19);
    }

    #[test]
    fn bundled_email_detector_matches_comma_separated_addresses() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalid,b@example.invalid");

        assert_eq!(detections.len(), 2);
        assert_eq!(detections[0].span, 0..17);
        assert_eq!(detections[1].span, 18..35);
    }

    #[test]
    fn bundled_email_detector_accepts_ascii_punctuation_suffixes() {
        let detector = RegexDetector::emails().expect("email detector");

        for (input, span) in [
            ("Contact a@example.invalid. Thanks", 8..25),
            ("a@example.invalid- call me", 0..17),
            ("a@example.invalid+", 0..17),
            ("a@example.invalid%", 0..17),
        ] {
            let detections = Detector::detect(&detector, input);
            assert_eq!(detections.len(), 1, "{input}");
            assert_eq!(detections[0].span, span, "{input}");
        }
    }

    #[test]
    fn bundled_email_detector_keeps_leading_delimiters_out_of_span() {
        let detector = RegexDetector::emails().expect("email detector");

        for input in [
            ".a@example.invalid",
            "-a@example.invalid",
            "+a@example.invalid",
        ] {
            let detections = Detector::detect(&detector, input);
            assert_eq!(detections.len(), 1, "{input}");
            assert_eq!(detections[0].span, 1..18, "{input}");
        }
    }

    #[test]
    fn bundled_email_detector_rejects_ascii_continuation_suffix() {
        let detector = RegexDetector::emails().expect("email detector");
        let detections = Detector::detect(&detector, "a@example.invalid1");

        assert!(detections.is_empty());
    }

    #[test]
    fn email_rfc_validator_kind_populates_canonical_form() {
        let detector = RegexDetector::with_rulepack_fields(
            r"(?i)\b[a-z0-9._%+\-]+@example\.invalid\b",
            PiiClass::Email,
            "email.test",
            vec![LocaleTag::Global],
            0.70,
            0,
            "counter",
            None,
            Vec::new(),
            Some(ValidatorKind::EmailRfc),
            Some(NormalizerKind::EmailCanonical),
        )
        .expect("regex detector");
        let dictionaries = gaze_types::DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        let detections =
            Recognizer::detect(&detector, "Email Alice@Example.invalid", &ctx).unwrap();

        assert_eq!(
            detections[0].canonical_form.as_deref(),
            Some("alice@example.invalid")
        );
    }

    fn detector_with_exclusions(exclusions: &[&str]) -> RegexDetector {
        RegexDetector::with_rulepack_fields(
            r"(?i)(?-u:\b)([a-z0-9._%+\-]+@(?:test\.local|example\.invalid))(?-u:\b)",
            PiiClass::Email,
            "email.global",
            vec![LocaleTag::Global],
            0.70,
            90,
            "counter",
            Some(vec![1]),
            exclusions.iter().map(|&s| s.to_string()).collect(),
            Some(ValidatorKind::EmailRfc),
            Some(NormalizerKind::EmailCanonical),
        )
        .expect("regex detector")
    }

    #[test]
    fn is_excluded_is_ascii_case_insensitive_for_both_branches_and_cased_exclusion_values() {
        // Substring and exact-match branches must both be ASCII-case-insensitive,
        // and a cased exclusion value must match a differently-cased matched text.
        let detector = detector_with_exclusions(&["test.local"]);
        assert!(detector.is_excluded("user@test.local"));
        assert!(detector.is_excluded("User@Test.Local"));
        assert!(detector.is_excluded("USER@TEST.LOCAL"));
        assert!(detector.is_excluded("test.local"));
        assert!(detector.is_excluded("Test.Local"));
        assert!(!detector.is_excluded("user@example.invalid"));
        assert!(!detector.is_excluded("example.invalid"));

        let detector = detector_with_exclusions(&["TEST.LOCAL"]);
        assert!(detector.is_excluded("user@test.local"));
        assert!(detector.is_excluded("User@Test.Local"));
        assert!(!detector.is_excluded("user@example.invalid"));
    }

    #[test]
    #[cfg(feature = "phone-parser")]
    fn national_phone_validator_kind_accepts_safe_fixtures() {
        let us = ValidatorKind::parse("e164_phone_national_us").expect("US validator");
        assert_eq!(
            us.canonical_form(
                // Source: NANPA 555-LINE Number Reservation.
                // https://nationalnanpa.com/number_resource_info/555_numbers.html
                "+1 555 0100"
            )
            .as_deref(),
            Some("+15550100")
        );

        let de = ValidatorKind::parse("e164_phone_national_de").expect("DE validator");
        assert_eq!(
            de.canonical_form(
                // Source: synthetic-non-reachable; no DE equivalent of NANPA 555-01XX exists;
                // literals chosen for parser-valid + non-routable.
                "+49 30 0000 0000"
            )
            .as_deref(),
            Some("+493000000000")
        );
    }

    #[test]
    #[cfg(not(feature = "phone-parser"))]
    fn national_phone_validator_kind_fails_closed_without_feature() {
        let err = ValidatorKind::parse("e164_phone_national_us")
            .expect_err("phone parser feature is disabled");
        assert!(matches!(
            err,
            gaze_types::ValidatorKindParseError::UnsupportedValidator { kind }
                if kind == "e164_phone_national_us"
        ));
    }

    #[test]
    fn regex_recognizer_uses_first_non_empty_capture_group() {
        let detector = RegexDetector::with_rulepack_fields(
            r#"(?m)^From:\s+(?:"([^"]+)"|([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+))\s+<[^>]+>"#,
            PiiClass::Name,
            "email.header.name",
            vec![LocaleTag::Global],
            0.90,
            0,
            "email.header.name",
            Some(vec![1, 2]),
            Vec::new(),
            None,
            None,
        )
        .expect("regex detector");
        let dictionaries = gaze_types::DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        let input =
            "From: Dana Weber <user@example.invalid>\nFrom: \"Prof. Weber\" <other@example.invalid>";

        let candidates = Recognizer::detect(&detector, input, &ctx).unwrap();
        let matched = candidates
            .iter()
            .map(|candidate| &input[candidate.span.clone()])
            .collect::<Vec<_>>();

        assert_eq!(matched, vec!["Dana Weber", "Prof. Weber"]);
    }
}
