use gaze_types::{
    Candidate, ConflictTier, DetectContext, Detection, Detector, LabelledValueMode,
    LabelledValueScanReason, LocaleBasis, LocaleTag, PiiClass, Recognizer, ValidatorKind,
    ValidatorOnFail,
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
    labelled_value_mode: LabelledValueMode,
    exclusions: Vec<String>,
    reject_match_regex: Option<Regex>,
    reject_unquoted_capture_regex: Option<Regex>,
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
            labelled_value_mode: LabelledValueMode::Grouped,
            exclusions: exclusions
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            reject_match_regex: None,
            reject_unquoted_capture_regex: None,
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
                let repeat_seed = scan.repeat_seed.filter(|seed| {
                    let scanner_prepended_wrapper = self.complete_labelled_value
                        && self.labelled_value_mode == LabelledValueMode::Grouped
                        && scan.span.start.checked_add(1) == Some(scan.capture.start)
                        && input.get(scan.span.start..scan.capture.start) == Some("<")
                        && seed.kind == gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix;
                    // Recognizers see pipeline-normalized input, the same domain
                    // used by the sweep. Only a scanner-prepended wrapper is
                    // excluded from the eligibility query; eligible seed bytes
                    // and the whole primary owner retain their original geometry.
                    !scanner_prepended_wrapper
                        || self
                            .class
                            .clears_repeat_floor(input[scan.capture.start..seed.span.end].trim())
                });
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
                candidate.labelled_value_repeat_seed = repeat_seed;
                candidate.labelled_value_capture_end = (self.complete_labelled_value
                    && self.labelled_value_mode == LabelledValueMode::Grouped
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

    /// Refuse matching capture text when the value itself has no opening quote. Quotes on
    /// a JSON key or prose label cannot exempt an unquoted value from this guard.
    pub fn with_unquoted_capture_rejection(mut self, pattern: Option<&str>) -> Result<Self> {
        self.reject_unquoted_capture_regex = pattern
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

    /// Select field ownership without changing legacy grouped-value rules.
    pub fn with_labelled_value_mode(mut self, mode: LabelledValueMode) -> Self {
        self.labelled_value_mode = mode;
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
        let mut scalar_index = ScalarBoundaryIndex::default();
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
                        if self.labelled_value_mode == LabelledValueMode::FieldOwned {
                            return scan_field_owned_value_indexed(input, span, &mut scalar_index);
                        }
                        // Only these new fallbacks may trim a field cue inside their broad
                        // capture. Existing cue rules must retain every originally matched byte.
                        let trim_internal_field_boundary = matches!(
                            self.source.as_str(),
                            "tax_number.labelled" | "driver_license.labelled" | "id_card.labelled"
                        );
                        scan_labelled_value(input, span, trim_internal_field_boundary)
                    } else {
                        LabelledValueScan {
                            quoted_value: false,
                            capture: span.clone(),
                            extension: span.clone(),
                            span,
                            repeat_seed: None,
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
                    span.rejected |= self.reject_match_regex.as_ref().is_some_and(|guard| {
                        let checked = if self.complete_labelled_value {
                            &input[full.start()..span.capture.end]
                        } else {
                            full.as_str()
                        };
                        guard.is_match(checked)
                    });
                    let unquoted_capture_rejected = self
                        .reject_unquoted_capture_regex
                        .as_ref()
                        .is_some_and(|guard| {
                            let value_quoted = span.quoted_value
                                || input[..span.capture.start]
                                    .chars()
                                    .next_back()
                                    .is_some_and(field_wrapper_opens);
                            let checked =
                                if self.labelled_value_mode == LabelledValueMode::FieldOwned {
                                    span.span.clone()
                                } else {
                                    span.capture.clone()
                                };
                            !value_quoted && guard.is_match(&input[checked])
                        });
                    span.rejected |= unquoted_capture_rejected;
                    if unquoted_capture_rejected && span.capture.end > full.start() {
                        // A rejected placeholder cannot own scanner extensions: an uppercase
                        // group may be the next field's cue. Retain the audit veto, but scan
                        // again from the original capture end so that cue remains visible.
                        search_at = Some(span.capture.end);
                    }
                    span.rejected |= self.ipv4_phone_tail(input, &span.span);
                    return Some(match phone_parts {
                        None => vec![span],
                        Some(parts) => parts
                            .into_iter()
                            .map(|part| LabelledValueScan {
                                quoted_value: false,
                                capture: part.clone(),
                                extension: part.clone(),
                                span: part,
                                repeat_seed: None,
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
                        quoted_value: false,
                        repeat_seed: None,
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
        let mut parts = self.phone_numeric_parts(input, span.clone(), full_start);
        if self.validator_on_fail != ValidatorOnFail::Record || full_start == span.start {
            return parts;
        }
        let prefix = &input[full_start..span.start];
        // A cue licenses a list of independently valid phone values. Replay it across
        // explicit list separators, never across an arbitrary word or numeric field.
        while let Some(last) = parts.last() {
            let tail = &input[last.end..];
            let trimmed = tail.trim_start_matches(char::is_whitespace);
            let separator_end = if trimmed.starts_with(',') {
                tail.len() - trimmed.len() + 1
            } else if let Some(rest) = trimmed
                .strip_prefix("oder")
                .or_else(|| trimmed.strip_prefix("or"))
            {
                if !rest.starts_with(char::is_whitespace) {
                    break;
                }
                tail.len() - rest.len()
            } else if tail[..tail.len() - trimmed.len()]
                .chars()
                .any(|ch| matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            {
                tail.len() - trimmed.len()
            } else {
                break;
            };
            let after = tail[separator_end..].trim_start_matches(char::is_whitespace);
            let next_start = input.len() - after.len();
            // The regex is bounded; a small cue-replayed window is sufficient to offer
            // the next candidate. Its continuation is scanned on the original input.
            let window: String = after.chars().take(128).collect();
            let evidence = format!("{prefix}{window}");
            let Some(next) = self
                .regex
                .captures(&evidence)
                .and_then(|caps| self.span_from_captures(&caps))
                .filter(|matched| matched.start == prefix.len())
            else {
                break;
            };
            let kind = self.validator_kind.expect("phone validator");
            let next_parts = self.phone_numeric_parts(&evidence, next, 0);
            if next_parts.is_empty()
                || next_parts
                    .iter()
                    .any(|part| !kind.validates(&evidence[part.clone()]))
            {
                break;
            }
            parts.extend(next_parts.into_iter().map(|part| {
                next_start + part.start - prefix.len()..next_start + part.end - prefix.len()
            }));
        }
        parts
    }

    #[cfg(feature = "phone-parser")]
    fn phone_numeric_parts(
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
                        || (ch.is_whitespace()
                            && (!records || !matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')))
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
#[cfg_attr(test, derive(Debug, PartialEq))]
struct LabelledValueScan {
    quoted_value: bool,
    capture: std::ops::Range<usize>,
    extension: std::ops::Range<usize>,
    span: std::ops::Range<usize>,
    reason: Option<LabelledValueScanReason>,
    repeat_seed: Option<gaze_types::LabelledValueRepeatSeed>,
    rejected: bool,
}

fn field_value_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || unicode_normalization::char::is_combining_mark(ch)
        || matches!(ch, '+' | '/' | '_' | '=' | '-' | '.' | ':' | '\\')
}

const LABELLED_CUE_HORIZONTAL: &str = r"[ \t\u{00A0}\u{202F}]";

fn labelled_assignment_connector() -> String {
    let horizontal = LABELLED_CUE_HORIZONTAL;
    format!(r"(?:{horizontal}*[:=]{horizontal}*|{horizontal}+(?:(?:is|ist|lautet){horizontal}+)?)")
}

fn labelled_licence_connector() -> String {
    let horizontal = LABELLED_CUE_HORIZONTAL;
    format!(r"(?:{horizontal}*[:=|,;\t-]{horizontal}*|{horizontal}+(?:is|ist|lautet){horizontal}+)")
}

fn assignment_prefix_end(rest: &str) -> Option<usize> {
    static FIELD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    if let Some(matched) = FIELD
        .get_or_init(|| {
            Regex::new(
                r"\A[\p{L}_][\p{L}\p{N}\p{M}_.-]*[ \t\u{00A0}\u{202F}]*[:=][ \t\u{00A0}\u{202F}]*",
            )
            .expect("static assignment syntax")
        })
        .find(rest)
    {
        return Some(matched.end());
    }
    // Complete human cue phrases use a field noun plus the same same-line
    // connectors as labelled seeds. An arbitrary uppercase word is no cue.
    // Check from the first label byte so neither labels nor their values join
    // the preceding identifier. Explicit machine assignments above remain open.
    static CUE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    CUE.get_or_init(|| {
        let horizontal = LABELLED_CUE_HORIZONTAL;
        let noun = r"(?:id|number|nummer|no\.?|nr\.?|key|code|reference|date|datum|value|wert|version)";
        let words = format!(r"[\p{{L}}\p{{M}}_-]+(?:{horizontal}+[\p{{L}}\p{{M}}_-]+)*?{horizontal}+{noun}");
        let compound = format!(r"(?:[\p{{L}}\p{{M}}]+[_-]{noun}|(?:customer|kunden|employee|personnel|personal|mitarbeiter|person|contact|member|membership|mitglieds|personen|kontakt)(?:[_-]?(?:record|datensatz))?[_-]?{noun})");
        let connector = labelled_assignment_connector();
        // These licence labels have their own grouped-value recognizers. Their
        // explicit connectors prove the entire label, not its final word alone.
        let licence_connector = labelled_licence_connector();
        let licence = format!(r"(?:(?:driver'?s?|driving){horizontal}+licen[cs]e|f[uü]hrerschein){licence_connector}");
        Regex::new(&format!(r"(?i)\A(?:(?:{words}|{compound}){connector}|{licence})"))
            .expect("static complete field cue grammar")
    }).find(rest).map(|matched| matched.end())
}

fn assignment_starts(rest: &str) -> bool {
    assignment_prefix_end(rest).is_some()
}

fn conservative_assignment_prefix_end(rest: &str) -> Option<usize> {
    // A decomposed German label proves conservative ownership only. It does
    // not establish a public boundary for field-owned values whose independent
    // next recognizer may not support that spelling.
    assignment_prefix_end(rest).or_else(|| unicode_licence_prefix_end(rest))
}

fn legacy_complete_seed_cue_end(rest: &str) -> Option<usize> {
    conservative_assignment_prefix_end(rest).or_else(|| {
        // Existing private labels can end in nouns outside the general cue
        // grammar. Recognize the complete label for seed geometry only: this
        // does not consume another byte or infer a public boundary.
        static CUE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        CUE.get_or_init(|| {
            let horizontal = LABELLED_CUE_HORIZONTAL;
            let connector = labelled_licence_connector();
            Regex::new(&format!(
                r"(?i)\A(?:national{horizontal}+insurance|(?:id|identity){horizontal}+card){connector}"
            )).expect("static complete legacy seed cues")
        }).find(rest).map(|matched| matched.end())
    })
}

fn unicode_licence_prefix_end(rest: &str) -> Option<usize> {
    static LICENCE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    LICENCE
        .get_or_init(|| {
            let connector = labelled_licence_connector();
            Regex::new(&format!(
                r"(?i)\Af(?:u\u{{0308}}|[uü])hrerschein{connector}"
            ))
            .expect("static conservative Unicode licence cue")
        })
        .find(rest)
        .map(|matched| matched.end())
}

fn labelled_group_byte_len(rest: &str) -> usize {
    rest.char_indices()
        .take_while(|(_, ch)| {
            ch.is_alphanumeric() || unicode_normalization::char::is_combining_mark(*ch)
        })
        .last()
        .map_or(0, |(at, ch)| at + ch.len_utf8())
}

fn labelled_group_is_uppercase(group: &str) -> bool {
    let mut uppercase_letter = false;
    for ch in group.chars() {
        if ch.is_uppercase() {
            uppercase_letter = true;
        } else if !unicode_normalization::char::is_combining_mark(ch) {
            return false;
        }
    }
    // Marks-only and uncased scripts never become uppercase vacuously.
    uppercase_letter
}

fn public_metadata_assignment_starts(rest: &str) -> bool {
    // Only these complete operational metadata namespaces establish public
    // text. Private and unknown assignments need the registry's existing proof
    // from an actual next recognizer before their conservative tail can shrink.
    static CUE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    CUE.get_or_init(|| {
        let horizontal = LABELLED_CUE_HORIZONTAL;
        let namespace = r"(?:build|release|schema|protocol|runtime|format|api|package|software)";
        let connector = labelled_assignment_connector();
        Regex::new(&format!(
            r"(?i)\A{namespace}(?:{horizontal}+|[_-])version{connector}"
        ))
        .expect("static public metadata cue grammar")
    })
    .is_match(rest)
}

fn field_wrapper_opens(ch: char) -> bool {
    // These separate unquoted fields; an empty field before one has no value.
    !field_value_char(ch) && !ch.is_whitespace() && !matches!(ch, ';' | ',' | '|')
}

fn field_wrapper_closes(open: char, close: char) -> bool {
    match open {
        '\u{28}' => matches!(close, '\u{29}'),
        '\u{5b}' => matches!(close, '\u{5d}'),
        '\u{7b}' => matches!(close, '\u{7d}'),
        '\u{ab}' => matches!(close, '\u{bb}'),
        '\u{f3a}' => matches!(close, '\u{f3b}'),
        '\u{f3c}' => matches!(close, '\u{f3d}'),
        '\u{169b}' => matches!(close, '\u{169c}'),
        '\u{2018}' => matches!(close, '\u{2019}'),
        '\u{201a}' => matches!(close, '\u{2019}' | '\u{2018}'),
        '\u{201b}' => matches!(close, '\u{2019}'),
        '\u{201c}' => matches!(close, '\u{201d}'),
        '\u{201e}' => matches!(close, '\u{201d}' | '\u{201c}'),
        '\u{201f}' => matches!(close, '\u{201d}'),
        '\u{2039}' => matches!(close, '\u{203a}'),
        '\u{2045}' => matches!(close, '\u{2046}'),
        '\u{207d}' => matches!(close, '\u{207e}'),
        '\u{208d}' => matches!(close, '\u{208e}'),
        '\u{2308}' => matches!(close, '\u{2309}'),
        '\u{230a}' => matches!(close, '\u{230b}'),
        '\u{2329}' => matches!(close, '\u{232a}'),
        '\u{2768}' => matches!(close, '\u{2769}'),
        '\u{276a}' => matches!(close, '\u{276b}'),
        '\u{276c}' => matches!(close, '\u{276d}'),
        '\u{276e}' => matches!(close, '\u{276f}'),
        '\u{2770}' => matches!(close, '\u{2771}'),
        '\u{2772}' => matches!(close, '\u{2773}'),
        '\u{2774}' => matches!(close, '\u{2775}'),
        '\u{27c5}' => matches!(close, '\u{27c6}'),
        '\u{27e6}' => matches!(close, '\u{27e7}'),
        '\u{27e8}' => matches!(close, '\u{27e9}'),
        '\u{27ea}' => matches!(close, '\u{27eb}'),
        '\u{27ec}' => matches!(close, '\u{27ed}'),
        '\u{27ee}' => matches!(close, '\u{27ef}'),
        '\u{2983}' => matches!(close, '\u{2984}'),
        '\u{2985}' => matches!(close, '\u{2986}'),
        '\u{2987}' => matches!(close, '\u{2988}'),
        '\u{2989}' => matches!(close, '\u{298a}'),
        '\u{298b}' => matches!(close, '\u{298c}'),
        '\u{298d}' => matches!(close, '\u{2990}'),
        '\u{298f}' => matches!(close, '\u{298e}'),
        '\u{2991}' => matches!(close, '\u{2992}'),
        '\u{2993}' => matches!(close, '\u{2994}'),
        '\u{2995}' => matches!(close, '\u{2996}'),
        '\u{2997}' => matches!(close, '\u{2998}'),
        '\u{29d8}' => matches!(close, '\u{29d9}'),
        '\u{29da}' => matches!(close, '\u{29db}'),
        '\u{29fc}' => matches!(close, '\u{29fd}'),
        '\u{2e02}' => matches!(close, '\u{2e03}'),
        '\u{2e04}' => matches!(close, '\u{2e05}'),
        '\u{2e09}' => matches!(close, '\u{2e0a}'),
        '\u{2e0c}' => matches!(close, '\u{2e0d}'),
        '\u{2e1c}' => matches!(close, '\u{2e1d}'),
        '\u{2e20}' => matches!(close, '\u{2e21}'),
        '\u{2e22}' => matches!(close, '\u{2e23}'),
        '\u{2e24}' => matches!(close, '\u{2e25}'),
        '\u{2e26}' => matches!(close, '\u{2e27}'),
        '\u{2e28}' => matches!(close, '\u{2e29}'),
        '\u{2e42}' => matches!(close, '\u{201d}'),
        '\u{3008}' => matches!(close, '\u{3009}'),
        '\u{300a}' => matches!(close, '\u{300b}'),
        '\u{300c}' => matches!(close, '\u{300d}'),
        '\u{300e}' => matches!(close, '\u{300f}'),
        '\u{3010}' => matches!(close, '\u{3011}'),
        '\u{3014}' => matches!(close, '\u{3015}'),
        '\u{3016}' => matches!(close, '\u{3017}'),
        '\u{3018}' => matches!(close, '\u{3019}'),
        '\u{301a}' => matches!(close, '\u{301b}'),
        '\u{301d}' => matches!(close, '\u{301e}' | '\u{301f}'),
        '\u{fd3f}' => matches!(close, '\u{fd3e}'),
        '\u{fe17}' => matches!(close, '\u{fe18}'),
        '\u{fe35}' => matches!(close, '\u{fe36}'),
        '\u{fe37}' => matches!(close, '\u{fe38}'),
        '\u{fe39}' => matches!(close, '\u{fe3a}'),
        '\u{fe3b}' => matches!(close, '\u{fe3c}'),
        '\u{fe3d}' => matches!(close, '\u{fe3e}'),
        '\u{fe3f}' => matches!(close, '\u{fe40}'),
        '\u{fe41}' => matches!(close, '\u{fe42}'),
        '\u{fe43}' => matches!(close, '\u{fe44}'),
        '\u{fe47}' => matches!(close, '\u{fe48}'),
        '\u{fe59}' => matches!(close, '\u{fe5a}'),
        '\u{fe5b}' => matches!(close, '\u{fe5c}'),
        '\u{fe5d}' => matches!(close, '\u{fe5e}'),
        '\u{ff08}' => matches!(close, '\u{ff09}'),
        '\u{ff3b}' => matches!(close, '\u{ff3d}'),
        '\u{ff5b}' => matches!(close, '\u{ff5d}'),
        '\u{ff5f}' => matches!(close, '\u{ff60}'),
        '\u{ff62}' => matches!(close, '\u{ff63}'),
        '\u{60}' => matches!(close, '\u{60}'),
        '\u{3c}' => matches!(close, '\u{3e}'),
        '\u{2e55}' => close == '\u{2e56}',
        '\u{2e57}' => close == '\u{2e58}',
        '\u{2e59}' => close == '\u{2e5a}',
        '\u{2e5b}' => close == '\u{2e5c}',
        _ => close == open,
    }
}

#[cfg(test)]
fn scan_field_owned_value(input: &str, capture: std::ops::Range<usize>) -> LabelledValueScan {
    scan_field_owned_value_indexed(input, capture, &mut ScalarBoundaryIndex::default())
}

#[cfg(test)]
thread_local! {
    static SCANNER_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn scanner_work(_steps: usize) {
    #[cfg(test)]
    SCANNER_WORK.with(|work| work.set(work.get() + _steps));
}

mod scalar_index;
use scalar_index::ScalarBoundaryIndex;

#[cfg(test)]
mod scalar_reference;

fn scan_field_owned_value_indexed(
    input: &str,
    capture: std::ops::Range<usize>,
    index: &mut ScalarBoundaryIndex,
) -> LabelledValueScan {
    if input[..capture.start]
        .chars()
        .next_back()
        .is_some_and(field_wrapper_opens)
    {
        return scan_quoted_field_value(input, capture, index);
    }
    if input.as_bytes().get(capture.start) == Some(&b'\\') {
        if let Some(scan) = scan_json_encoded_wrapper(input, capture.clone(), index) {
            return scan;
        }
    }
    // The seed includes an unfamiliar wrapper instead of consuming it in the
    // regex. A personal field still owns its value; punctuation cannot turn
    // that evidence into an empty unquoted capture. Leading escape bytes are
    // wrapper syntax (for example Markdown's escaped backtick).
    let escapes = input[capture.start..]
        .bytes()
        .take_while(|byte| *byte == b'\\')
        .count();
    if let Some(opener) = input[capture.start + escapes..]
        .chars()
        .next()
        .filter(|ch| field_wrapper_opens(*ch))
    {
        let start = capture.start + escapes + opener.len_utf8();
        return scan_paired_field_value(
            input,
            start..capture.end.max(start),
            opener,
            escapes,
            index,
        );
    }
    let mut end = capture.start;
    let mut reason = None;
    loop {
        let group_start = end;
        end += input[end..]
            .chars()
            .take_while(|ch| field_value_char(*ch))
            .map(char::len_utf8)
            .sum::<usize>();
        if end == group_start {
            break;
        }
        // A period or colon between components belongs to the identifier; at
        // the end of an unquoted group it is prose syntax. Quoted scalars use
        // their delimiter instead and retain every punctuation byte.
        let value_end = group_start + input[group_start..end].trim_end_matches(['.', ':']).len();
        if value_end != end {
            end = value_end;
            break;
        }
        let spaces = input[end..]
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t' | '\u{00a0}' | '\u{202f}'))
            .map(char::len_utf8)
            .sum::<usize>();
        if spaces == 0 {
            break;
        }
        let next = end + spaces;
        if assignment_starts(&input[next..]) {
            reason = Some(LabelledValueScanReason::LabelBoundary);
            break;
        }
        if starts_with_date(&input[next..]) {
            reason = Some(LabelledValueScanReason::DateBoundary);
            break;
        }
        let group: String = input[next..]
            .chars()
            .take_while(|ch| field_value_char(*ch))
            .collect();
        let value_like = group.chars().any(char::is_numeric)
            || (group.chars().any(char::is_alphabetic)
                && group
                    .chars()
                    .filter(|ch| ch.is_alphabetic())
                    .all(char::is_uppercase));
        if !value_like {
            break;
        }
        end = next;
    }
    let span = capture.start..end;
    let repeat_seed = input[span.clone()]
        .rsplit_once('\t')
        .and_then(|(prefix, last)| {
            (prefix.chars().any(char::is_numeric)
                && last.chars().any(char::is_alphabetic)
                && last.chars().all(|ch| {
                    ch.is_alphabetic() || unicode_normalization::char::is_combining_mark(ch)
                }))
            .then_some(gaze_types::LabelledValueRepeatSeed {
                span: span.start..span.start + prefix.len(),
                kind: gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix,
            })
        });
    LabelledValueScan {
        quoted_value: false,
        repeat_seed,
        capture: capture.start..capture.end.min(end),
        extension: capture.end.min(end)..end,
        reason: labelled_value_over_limit(&input[span.clone()])
            .then_some(LabelledValueScanReason::LimitExceeded)
            .or(reason),
        rejected: span.is_empty(),
        span,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct EnclosingScalarBoundary {
    end: usize,
    absent_value: bool,
    doubled_quote: Option<u8>,
    record_lines_are_content: bool,
    own_sql_literal: bool,
}

struct ScalarSourceView {
    text: String,
    source: Vec<std::ops::Range<usize>>,
}

impl ScalarSourceView {
    fn json_frame(&self, token: std::ops::Range<usize>, closed: bool) -> Option<Self> {
        if closed {
            return self.json_string(token);
        }
        // A streaming JSON string may stop before its close or halfway through
        // an escape. Validate the complete prefix with a virtual close; retain
        // incomplete escape bytes literally in the same original-byte domain.
        static PARTIAL_ESCAPE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let partial_escape = PARTIAL_ESCAPE.get_or_init(|| Regex::new(
            r"(?i)\A\\(?:u[0-9a-f]{0,3}|ud[89ab][0-9a-f]{2}(?:\\(?:u(?:d(?:[c-f][0-9a-f]?)?)?)?)?)?\z"
        ).expect("partial JSON escape"));
        for trim in 0..=12.min(token.len().saturating_sub(1)) {
            let end = token.end - trim;
            let Some(tail) = self.text.get(end..token.end) else {
                continue;
            };
            if trim != 0 && !partial_escape.is_match(tail) {
                continue;
            }
            let mut frame = Self {
                text: self.text[..end].to_owned(),
                source: self.source[..end].to_vec(),
            };
            frame.text.push('"');
            let point = self.source.get(end).map_or_else(
                || self.source.last().map_or(0, |range| range.end),
                |range| range.start,
            );
            frame.source.push(point..point);
            if let Some(mut decoded) = frame.json_string(token.start..end + 1) {
                decoded.text.push_str(tail);
                decoded
                    .source
                    .extend_from_slice(&self.source[end..token.end]);
                return Some(decoded);
            }
        }
        None
    }

    fn json_string(&self, token: std::ops::Range<usize>) -> Option<Self> {
        let raw = self.text.get(token.clone())?;
        let decoded: String = serde_json::from_str(raw).ok()?;
        let mut source = Vec::with_capacity(decoded.len());
        let mut at = token.start + 1;
        while at < token.end - 1 {
            scanner_work(1);
            let end = if self.text.as_bytes()[at] == b'\\' {
                if self.text.as_bytes().get(at + 1) == Some(&b'u') {
                    let first = u16::from_str_radix(self.text.get(at + 2..at + 6)?, 16).ok()?;
                    at + if (0xd800..=0xdbff).contains(&first) {
                        12
                    } else {
                        6
                    }
                } else {
                    at + 2
                }
            } else {
                at + self.text[at..].chars().next()?.len_utf8()
            };
            let unit = self.text.get(at..end)?;
            let width = if unit.starts_with('\\') {
                serde_json::from_str::<String>(&format!("\"{unit}\""))
                    .ok()?
                    .len()
            } else {
                unit.len()
            };
            let original = self.source.get(at)?.start..self.source.get(end - 1)?.end;
            source.extend(std::iter::repeat_n(original, width));
            at = end;
        }
        (source.len() == decoded.len()).then_some(Self {
            text: decoded,
            source,
        })
    }
}

fn scan_json_encoded_wrapper(
    input: &str,
    capture: std::ops::Range<usize>,
    index: &mut ScalarBoundaryIndex,
) -> Option<LabelledValueScan> {
    index.scan_encoded_wrapper(input, capture)
}

fn quoted_csv_cell_encloses(input: &str, opening: usize, close: usize) -> bool {
    let prefix = input[..opening].trim_end_matches([' ', '\t']);
    let starts_cell = prefix
        .chars()
        .next_back()
        .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'));
    let Some(suffix) = input.get(close..).and_then(|rest| rest.strip_prefix('"')) else {
        return false;
    };
    let suffix = suffix.trim_start_matches([' ', '\t']);
    starts_cell
        && suffix
            .chars()
            .next()
            .is_none_or(|ch| matches!(ch, ',' | '\r' | '\n'))
}

#[cfg(test)]
fn serialized_context_is_json(input: &str, enclosing: &[(usize, usize)], escapes: usize) -> bool {
    scanner_work(input.len());
    if !(escapes + 1).is_power_of_two() || serde_json::from_str::<serde_json::Value>(input).is_err()
    {
        return false;
    }
    serialized_context_contents_are_json(input, enclosing, escapes)
}

fn serialized_context_contents_are_json(
    input: &str,
    enclosing: &[(usize, usize)],
    escapes: usize,
) -> bool {
    let mut levels = 0;
    for &(depth, opening) in enclosing.iter().filter(|(depth, _)| *depth < escapes) {
        if (depth + 1).trailing_zeros() != levels {
            return false;
        }
        let close = serialized_string_end(input, opening + depth + 1, input.len(), Some(depth));
        let Some(raw) = input.get(opening..close + depth + 1) else {
            return false;
        };
        let mut raw = raw.to_owned();
        for _ in 0..levels {
            let Ok(decoded) = serde_json::from_str::<String>(&format!("\"{raw}\"")) else {
                return false;
            };
            raw = decoded;
        }
        let Ok(content) = serde_json::from_str::<String>(&raw) else {
            return false;
        };
        if serde_json::from_str::<serde_json::Value>(&content).is_err() {
            return false;
        }
        levels += 1;
    }
    levels == (escapes + 1).trailing_zeros()
}

fn serialized_string_end(input: &str, start: usize, limit: usize, depth: Option<usize>) -> usize {
    let Some(depth) = depth else { return limit };
    let mut escapes = 0usize;
    let mut at = start;
    while at < limit {
        scanner_work(1);
        let byte = input.as_bytes()[at];
        if byte == b'\\' {
            escapes += 1;
            at += 1;
            continue;
        }
        if byte == b'"' && (escapes + 1).is_power_of_two() && escapes <= depth {
            let opening = at - escapes;
            let run_end = quote_run_end(input, opening, byte, escapes, limit);
            let width = escapes + 1;
            let run = (run_end - opening) / width;
            if escapes == depth && run >= 2 {
                // At this depth a pair is CSV content. An odd trailing quote
                // ends the cell; ambiguous input protects the larger value.
                if run.is_multiple_of(2) {
                    at = run_end;
                    escapes = 0;
                    continue;
                }
                return run_end - width;
            }
            return opening;
        }
        escapes = 0;
        at += 1;
    }
    limit
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ScalarQuoteDelimiter {
    Single,
    Triple,
    DoubledSingle,
    DoubledTriple,
}

fn quote_run_end(input: &str, start: usize, quote: u8, escapes: usize, limit: usize) -> usize {
    let width = escapes + 1;
    let mut end = start;
    while end.checked_add(width).is_some_and(|next| next <= limit)
        && input.as_bytes()[end..end + escapes]
            .iter()
            .all(|byte| *byte == b'\\')
        && input.as_bytes()[end + escapes] == quote
    {
        end += width;
    }
    end
}

fn scalar_quote_close(
    input: &str,
    start: usize,
    limit: usize,
    quote: u8,
    opening_escapes: usize,
    delimiter: ScalarQuoteDelimiter,
) -> Option<usize> {
    let width = opening_escapes + 1;
    let stride = width.checked_mul(2)?;
    let mut escapes = 0usize;
    let mut at = start;
    while at < limit {
        scanner_work(1);
        let byte = input.as_bytes()[at];
        if byte == b'\\' {
            escapes += 1;
        } else {
            if byte == quote {
                let residue = escapes % stride;
                if residue == opening_escapes {
                    let run_start = at - opening_escapes;
                    let run_end = quote_run_end(input, run_start, quote, opening_escapes, limit);
                    let run = (run_end - run_start) / width;
                    let close = match delimiter {
                        ScalarQuoteDelimiter::Single if run % 2 == 1 => Some(run_end - width),
                        ScalarQuoteDelimiter::Single => None,
                        ScalarQuoteDelimiter::DoubledSingle if run % 4 == 2 => {
                            Some(run_end - 2 * width)
                        }
                        ScalarQuoteDelimiter::DoubledSingle => None,
                        ScalarQuoteDelimiter::DoubledTriple
                            if !run.is_multiple_of(2) || run > 10 =>
                        {
                            return None
                        }
                        ScalarQuoteDelimiter::DoubledTriple if run >= 6 => {
                            Some(run_end - 6 * width)
                        }
                        ScalarQuoteDelimiter::DoubledTriple => None,
                        ScalarQuoteDelimiter::Triple if run > 5 => return None,
                        ScalarQuoteDelimiter::Triple if run >= 3 => Some(run_end - 3 * width),
                        ScalarQuoteDelimiter::Triple => None,
                    };
                    if let Some(close) = close {
                        return (!input[run_end..]
                            .chars()
                            .next()
                            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'))
                        .then_some(close);
                    }
                    // Doubled delimiters are scalar content (YAML/SQL/CSV).
                    // A triple-quoted scalar also admits isolated quote pairs.
                    at = run_end;
                    escapes = 0;
                    continue;
                }
                if residue != stride - 1 {
                    return None;
                }
            }
            escapes = 0;
        }
        at += 1;
    }
    None
}

fn uncertain_scalar_end(input: &str, start: usize, limit: usize) -> usize {
    let mut escapes = 0usize;
    for (offset, byte) in input[start..limit].bytes().enumerate() {
        if matches!(byte, b'\n' | b'\r') && escapes.is_multiple_of(2) {
            return start + offset;
        }
        escapes = if byte == b'\\' { escapes + 1 } else { 0 };
    }
    limit
}

fn uncertain_field_end(input: &str, start: usize, enclosing: &EnclosingScalarBoundary) -> usize {
    if enclosing.record_lines_are_content {
        enclosing.end
    } else {
        uncertain_scalar_end(input, start, enclosing.end)
    }
}

fn scan_quoted_field_value(
    input: &str,
    capture: std::ops::Range<usize>,
    index: &mut ScalarBoundaryIndex,
) -> LabelledValueScan {
    let opener = input[..capture.start]
        .chars()
        .next_back()
        .expect("wrapper opener");
    if !matches!(opener, '"' | '\'') {
        return scan_paired_field_value(input, capture, opener, 0, index);
    }
    let quote = opener as u8;
    let opening_escapes = input[..capture.start - 1]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    // Both normal closes and uncertain fallbacks share this hard enclosure
    // boundary. A quote in a later sibling field can never close this value.
    let enclosing = index.boundary(input, capture.start - 1, capture.start, opening_escapes);
    let limit = enclosing.end;
    let width = opening_escapes + 1;
    let opening_start = capture.start - width;
    let opening_end = quote_run_end(input, opening_start, quote, opening_escapes, limit);
    let opening_run = if enclosing.own_sql_literal {
        1
    } else {
        (opening_end - opening_start) / width
    };
    let doubled_quote = enclosing.doubled_quote == Some(quote);
    let known_empty = enclosing.absent_value
        || (doubled_quote
            && matches!(opening_run, 4 | 12)
            && !input[opening_end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'))
        || (!doubled_quote
            && width.is_power_of_two()
            && matches!(opening_run, 2 | 6)
            && !input[opening_end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_'));
    let delimiter = match opening_run {
        2 if doubled_quote => Some(ScalarQuoteDelimiter::DoubledSingle),
        6..=10 if doubled_quote && opening_run.is_multiple_of(2) => {
            Some(ScalarQuoteDelimiter::DoubledTriple)
        }
        1 => Some(ScalarQuoteDelimiter::Single),
        3..=5 => Some(ScalarQuoteDelimiter::Triple),
        _ => None,
    }
    .filter(|_| width.is_power_of_two());
    let mut start = match delimiter {
        Some(ScalarQuoteDelimiter::Triple) => opening_start + 3 * width,
        Some(ScalarQuoteDelimiter::DoubledSingle) => opening_start + 2 * width,
        Some(ScalarQuoteDelimiter::DoubledTriple) => opening_start + 6 * width,
        _ => capture.start,
    };
    let known_empty = known_empty || (enclosing.record_lines_are_content && start == limit);
    let mut end = limit;
    let mut reason = Some(LabelledValueScanReason::UncertainQuoteBoundary);
    if known_empty {
        end = start;
        reason = None;
    } else if let Some(close) = delimiter.and_then(|delimiter| {
        scalar_quote_close(input, start, limit, quote, opening_escapes, delimiter)
    }) {
        end = close;
        reason = None;
    }
    if reason.is_some() {
        end = uncertain_field_end(input, start, &enclosing);
        if end <= start {
            // Unclassified opening runs must not turn real private text into
            // an empty rejected capture. Keep available opening bytes owned.
            start = opening_start;
        }
    }
    let span = start..end;
    let capture_end = capture.end.max(start).min(end);
    let repeat_seed = (!span.is_empty()).then_some(gaze_types::LabelledValueRepeatSeed {
        span: span.clone(),
        kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
    });
    LabelledValueScan {
        quoted_value: true,
        repeat_seed,
        capture: start..capture_end,
        extension: capture_end..end,
        reason: reason.or_else(|| {
            labelled_value_over_limit(&input[span.clone()])
                .then_some(LabelledValueScanReason::LimitExceeded)
        }),
        rejected: span.is_empty(),
        span,
    }
}

fn scan_paired_field_value(
    input: &str,
    capture: std::ops::Range<usize>,
    opener: char,
    wrapper_escapes: usize,
    index: &mut ScalarBoundaryIndex,
) -> LabelledValueScan {
    let opening = capture.start - opener.len_utf8();
    let enclosing = index.boundary(input, opening, capture.start, wrapper_escapes);
    let mut close = None;
    let mut at = capture.start;
    while at < enclosing.end {
        scanner_work(1);
        let ch = input[at..].chars().next().expect("scalar character");
        if field_wrapper_closes(opener, ch) {
            let run = input[at..enclosing.end]
                .chars()
                .take_while(|next| *next == ch)
                .count();
            let run_end = at + run * ch.len_utf8();
            if run % 2 == 1
                && !input[run_end..]
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_alphanumeric() || next == '_')
            {
                let delimiter = run_end - ch.len_utf8();
                let closing_escapes = input[..delimiter]
                    .bytes()
                    .rev()
                    .take_while(|byte| *byte == b'\\')
                    .count();
                close = Some(if closing_escapes == wrapper_escapes {
                    delimiter - wrapper_escapes
                } else {
                    delimiter
                });
                break;
            }
            at = run_end;
        } else {
            at += ch.len_utf8();
        }
    }
    let end = close.unwrap_or_else(|| uncertain_field_end(input, capture.start, &enclosing));
    let span = capture.start..end;
    let capture_end = capture.end.min(end);
    LabelledValueScan {
        quoted_value: true,
        capture: capture.start..capture_end,
        extension: capture_end..end,
        span: span.clone(),
        reason: close
            .is_none()
            .then_some(LabelledValueScanReason::UncertainQuoteBoundary)
            .or_else(|| {
                labelled_value_over_limit(&input[span.clone()])
                    .then_some(LabelledValueScanReason::LimitExceeded)
            }),
        repeat_seed: (!span.is_empty()).then_some(gaze_types::LabelledValueRepeatSeed {
            span: span.clone(),
            kind: gaze_types::LabelledValueRepeatSeedKind::ExactScalar,
        }),
        rejected: span.is_empty(),
    }
}

fn scan_labelled_value(
    input: &str,
    capture: std::ops::Range<usize>,
    trim_internal_field_boundary: bool,
) -> LabelledValueScan {
    let mut end = capture.start;
    let mut stop_reason = None;
    let mut assignment_connector_end = None;
    let mut uncertain_prefix = None;
    let initial_lowercase = input[capture.start..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase());
    loop {
        let group_start = end;
        end += labelled_group_byte_len(&input[end..]);
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
        let seed_separator = !input[separator_start..next_start].contains([';', '\n', '\r']);
        let next_len = labelled_group_byte_len(&input[next_start..]);
        if next_len == 0 {
            break;
        }
        let next = &input[next_start..next_start + next_len];
        // Dates never cut a proven capture. A following uppercase field stays covered until
        // another recognizer actually claims its value; the registry then exposes the label.
        let beyond_capture = next_start >= capture.end;
        if beyond_capture
            && (public_metadata_assignment_starts(&input[next_start..])
                || public_metadata_assignment_starts(&input[group_start..]))
        {
            end = end.max(capture.end);
            stop_reason = Some(LabelledValueScanReason::LabelBoundary);
            break;
        }
        if beyond_capture && starts_with_date(&input[next_start..]) {
            stop_reason = Some(LabelledValueScanReason::DateBoundary);
            break;
        }
        if assignment_connector_end.is_some_and(|until| next_start < until)
            && ["is", "ist", "lautet"]
                .iter()
                .any(|word| next.eq_ignore_ascii_case(word))
        {
            // Only a connector of an already accepted complete cue may bridge
            // lowercase text. Ordinary action/repeat assignments keep the
            // baseline value-group checks and never join the previous value.
            end = assignment_connector_end.take().expect("checked connector");
            continue;
        }
        if let Some(prefix_end) = (!next.is_ascii())
            .then(|| unicode_licence_prefix_end(&input[next_start..]))
            .flatten()
        {
            let cue_end = next_start + prefix_end;
            if seed_separator {
                uncertain_prefix.get_or_insert((separator_start, cue_end));
            }
            end = cue_end;
            continue;
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
        let value_like = next.chars().any(char::is_numeric)
            || labelled_group_is_uppercase(next)
            || (initial_lowercase
                && next_len <= 2
                && next.bytes().all(|byte| byte.is_ascii_lowercase()));
        if !value_like {
            break;
        }
        if seed_separator {
            if let Some(cue_end) = legacy_complete_seed_cue_end(&input[next_start..]) {
                uncertain_prefix.get_or_insert((separator_start, next_start + cue_end));
            }
        }
        if let Some(prefix_end) = conservative_assignment_prefix_end(&input[next_start..]) {
            assignment_connector_end = Some(next_start + prefix_end);
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
    // Only a complete cue retained inside the final owner licenses extra
    // prefix protection. The original capture and conservative tail stay
    // untouched. Wrapper bytes share the owner's start, preserving the
    // existing same-start translation and snapshot geometry.
    let repeat_seed = uncertain_prefix.and_then(|(prefix_end, cue_end)| {
        (start < prefix_end && prefix_end < end && cue_end < end).then_some(
            gaze_types::LabelledValueRepeatSeed {
                span: start..prefix_end,
                kind: gaze_types::LabelledValueRepeatSeedKind::UncertainPrefix,
            },
        )
    });
    LabelledValueScan {
        quoted_value: false,
        repeat_seed,
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
    fn field_owned_nested_scalar_stops_before_structure_at_every_depth() {
        let value = "ZX731482\"\\É843721";
        for depth in 0..=12 {
            let mut input = format!(
                "{{\"customerId\":{},\"dispatch_reference\":7}}",
                serde_json::to_string(value).unwrap()
            );
            let mut raw = value.to_owned();
            for wrapper in 0..=depth {
                let encoded = serde_json::to_string(&raw).unwrap();
                raw = encoded[1..encoded.len() - 1].to_owned();
                if wrapper < depth {
                    input = serde_json::to_string(&input).unwrap();
                }
            }
            let start = input.find(&raw).unwrap();
            let scan = scan_field_owned_value(&input, start..start + 2);
            assert_eq!(scan.span, start..start + raw.len(), "depth {depth}");
            assert_eq!(scan.repeat_seed.unwrap().span, scan.span);
        }
    }

    #[test]
    fn field_owned_unterminated_quote_keeps_nonempty_ownership_until_record_end() {
        for opening in ["", "\n", "\r\n"] {
            for delimiter in ["; ", ", ", "\n"] {
                for suffix in ["dispatch_reference=7", "dispatch_reference={\"kind\":7}"] {
                    let value = format!("{opening}ZX731482 É843721");
                    let input = format!("customerId: \"{value}{delimiter}{suffix}");
                    let start = input.find('"').unwrap() + 1;
                    let scan = scan_field_owned_value(&input, start..start + 2);
                    let end = input[start..]
                        .find(['\n', '\r'])
                        .map_or(input.len(), |offset| start + offset);
                    let owned_start = if start == end { start - 1 } else { start };
                    assert_eq!(scan.span, owned_start..end);
                    assert!(!scan.rejected);
                    assert_eq!(
                        scan.repeat_seed.map(|seed| seed.span),
                        Some(owned_start..end)
                    );
                }
            }
        }
    }

    #[test]
    fn unterminated_scalar_owns_container_looking_private_content() {
        for (prefix, suffix) in [("{", "}"), ("[{", "}]"), ("[", "]")] {
            let value = "Q731284;É842395";
            let input = format!("{prefix}customerId: \"{value}{suffix}");
            let start = input.find('"').unwrap() + 1;
            let scan = scan_field_owned_value(&input, start..start + 2);
            assert_eq!(&input[scan.span], format!("{value}{suffix}"));
        }
        let value = "Q731284;É842395; \"public_version':7";
        let input = format!("customerId: \"{value}");
        let start = input.find('"').unwrap() + 1;
        let scan = scan_field_owned_value(&input, start..start + 2);
        assert_eq!(&input[scan.span], value);
    }

    #[test]
    fn field_owned_closed_quote_keeps_delimiters_inside_the_scalar() {
        let value = "ZX731482; dispatch_reference=É843721, batch=9";
        let input = format!("customerId: \"{value}\"; dispatch_reference=7");
        let start = input.find('"').unwrap() + 1;
        let scan = scan_field_owned_value(&input, start..start + 2);
        assert_eq!(scan.span, start..start + value.len());
    }

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
    fn unverified_licence_boundary_preserves_the_complete_tail() {
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
    fn unquoted_capture_guard_owns_only_the_value_quote_and_preserves_audit_vetoes() {
        let detector = RegexDetector::with_rulepack_fields(
            r#"(?:"field"|field)[: ]+["']?([A-Za-z0-9]+)"#,
            PiiClass::custom("synthetic").unwrap(),
            "synthetic.labelled",
            vec![LocaleTag::Global],
            0.9,
            100,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap()
        .with_unquoted_capture_rejection(Some(r"^[\p{L}\p{M}]+$"))
        .unwrap();
        for (input, rejected) in [
            (r#""field":null"#, true),
            (r#""field":true"#, true),
            (r#""field":false"#, true),
            (r#""field" placeholder"#, true),
            (r#""field":"opaque""#, false),
            ("field: 'opaque'", false),
            (r#""field":600123"#, false),
        ] {
            let scans = detector.scanned_spans(input, None);
            assert_eq!(scans.len(), 1, "{input}");
            assert_eq!(scans[0].rejected, rejected, "{input}");
            let dictionaries = gaze_types::DictionaryBundle::default();
            let locales = [LocaleTag::Global];
            let ctx = DetectContext::new(&locales, &dictionaries);
            let audit = Recognizer::detect_for_registry(&detector, input, &ctx).unwrap();
            assert_eq!(audit.len(), 1, "{input}");
            assert_eq!(audit[0].regex_guard_rejected, rejected, "{input}");
        }
        assert!(detector.with_unquoted_capture_rejection(Some("[")).is_err());
    }

    #[test]
    fn rejected_unquoted_capture_resumes_before_scanner_extension() {
        let detector = RegexDetector::with_rulepack_fields(
            r"(?i:field)[ :=]+([A-Za-z0-9]+)",
            PiiClass::custom("synthetic").unwrap(),
            "synthetic.labelled",
            vec![LocaleTag::Global],
            0.9,
            100,
            "counter",
            Some(vec![1]),
            Vec::new(),
            None,
            None,
        )
        .unwrap()
        .with_complete_labelled_value(true)
        .with_unquoted_capture_rejection(Some(r"^[\p{L}\p{M}]+$"))
        .unwrap();
        let input = "field token FIELD=801234";
        let dictionaries = gaze_types::DictionaryBundle::default();
        let locales = [LocaleTag::Global];
        let ctx = DetectContext::new(&locales, &dictionaries);
        let scans = detector.scanned_spans(input, None);
        assert_eq!(scans.len(), 2);
        assert_eq!(&input[scans[0].capture.clone()], "token");
        assert!(scans[0].rejected);
        assert_eq!(&input[scans[1].span.clone()], "801234");
        assert!(!scans[1].rejected);
        let audit = Recognizer::detect_for_registry(&detector, input, &ctx).unwrap();
        assert_eq!(audit.len(), 2);
        assert!(audit[0].regex_guard_rejected);
        assert!(!audit[1].regex_guard_rejected);
        assert_eq!(Recognizer::detect(&detector, input, &ctx).unwrap().len(), 1);
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
