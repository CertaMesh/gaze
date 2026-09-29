//! Nym-small safety net: a pinned multilingual PII token classifier, opt-in.
//!
//! Pass-3 backend that runs `Wismut/nym-pii-multilingual-small` (v3, int8 ONNX) in process through
//! ONNX Runtime. It flags; the pipeline decides. Only allowlisted labels with an explicit
//! threshold can produce a suspect ([`NymOperatingPoint`], op-B by default), every suspect is a
//! whole word or run of words, and every suspect records the label and threshold that fired
//! (`raw_label = "LICENSE_PLATE>=0.5;view=stable"`) next to its score and the stable id
//! `nym-small-int8`.
//!
//! The bundle is SHA-256 pinned ([`NYM_SMALL_INT8_BUNDLE_SHA256`]) and verified before the model
//! loads. Input longer than one model window is scanned in overlapping windows; a piece no
//! window scored, or a character the tokenizer did not cover, is a typed error, never a silent
//! skip.

use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

pub use gaze_types::nym::{
    nym_label_to_pii_class, nym_label_to_safety_net_class, NymConfigError, NymLabel,
    NymOperatingPoint, NYM_SAFETY_NET_ID,
};
use gaze_types::{
    LeakKind, LeakReportTelemetry, LeakSuspect, LocaleTag, Manifest, SafetyNet, SafetyNetContext,
    SafetyNetError, SafetyNetRefusalReason,
};

pub mod artifacts;
pub(crate) mod decode;
mod ort;

pub use artifacts::{
    verify_nym_bundle, NYM_SMALL_CHECKSUM_FILE, NYM_SMALL_HF_COMMIT, NYM_SMALL_HF_REPO,
    NYM_SMALL_INT8_BUNDLE_SHA256, NYM_SMALL_INT8_SHA256SUMS, NYM_SMALL_UPSTREAM_FILES,
    REQUIRED_NYM_SMALL_ARTIFACTS,
};
pub use ort::{NymConfig, DEFAULT_NYM_INTRA_THREADS};

use decode::NymSpan;
use ort::NymOrtBackend;

/// The Nym-small safety net. The model loads on first use; a load failure is cached so a broken
/// bundle fails every check the same way instead of retrying.
pub struct NymSafetyNet {
    locales: Vec<LocaleTag>,
    config: NymConfig,
    backend: OnceLock<Result<Arc<NymOrtBackend>, Arc<SafetyNetError>>>,
}

impl std::fmt::Debug for NymSafetyNet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NymSafetyNet")
            .field("locales", &self.locales)
            .field("operating_point", self.config.operating_point())
            .finish_non_exhaustive()
    }
}

impl NymSafetyNet {
    pub fn new(config: NymConfig) -> Self {
        Self {
            locales: vec![LocaleTag::Global],
            config,
            backend: OnceLock::new(),
        }
    }

    pub fn from_env() -> Result<Self, SafetyNetError> {
        Ok(Self::new(NymConfig::from_env()?))
    }

    pub fn with_locales(mut self, locales: Vec<LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    pub fn config(&self) -> &NymConfig {
        &self.config
    }

    /// Loads the bundle now instead of on the first check.
    pub fn preload(&self) -> Result<(), SafetyNetError> {
        self.backend().map(|_| ())
    }

    fn backend(&self) -> Result<Arc<NymOrtBackend>, SafetyNetError> {
        match self.backend.get_or_init(|| {
            NymOrtBackend::new(self.config.clone())
                .map(Arc::new)
                .map_err(Arc::new)
        }) {
            Ok(backend) => Ok(Arc::clone(backend)),
            Err(error) => Err((**error).clone()),
        }
    }
}

impl SafetyNet for NymSafetyNet {
    fn id(&self) -> &str {
        NYM_SAFETY_NET_ID
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.check_with_telemetry(clean_text, context)
            .map(|(suspects, _)| suspects)
    }

    fn check_with_telemetry(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<(Vec<LeakSuspect>, Vec<LeakReportTelemetry>), SafetyNetError> {
        let backend = self.backend()?;
        let spans = backend.infer(clean_text)?;
        let mut suspects = Vec::with_capacity(spans.len());
        let mut telemetry = Vec::new();
        for span in spans {
            let range = span.start..span.end;
            match span_to_disposition(span, clean_text, backend.operating_point(), context)? {
                SpanDisposition::Suspect(suspect) => suspects.push(suspect),
                SpanDisposition::Refused(reason) => {
                    telemetry.push(LeakReportTelemetry::ModelSpanRefused {
                        safety_net_id: NYM_SAFETY_NET_ID.to_string(),
                        reason,
                        span: range,
                        document_kind: context.document_kind,
                        field_path: context.field_path.map(str::to_string),
                    });
                }
                SpanDisposition::Covered => {}
            }
        }
        Ok((suspects, telemetry))
    }

    fn check_with_neutral(
        &self,
        stable_text: &str,
        neutral_text: Option<&str>,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.check_with_neutral_and_telemetry(stable_text, neutral_text, context)
            .map(|(suspects, _)| suspects)
    }

    fn check_with_neutral_and_telemetry(
        &self,
        stable_text: &str,
        neutral_text: Option<&str>,
        context: SafetyNetContext<'_>,
    ) -> Result<(Vec<LeakSuspect>, Vec<LeakReportTelemetry>), SafetyNetError> {
        let Some(neutral_text) = neutral_text else {
            let (stable, telemetry) = self.check_with_telemetry(stable_text, context)?;
            return Ok((
                union_view_suspects(stable_text, context.manifest, stable, Vec::new()),
                telemetry,
            ));
        };
        if stable_text.len() != neutral_text.len()
            || !stable_text
                .char_indices()
                .map(|(index, _)| index)
                .eq(neutral_text.char_indices().map(|(index, _)| index))
        {
            return Err(SafetyNetError::InvalidOutput {
                message: "nym neutral view changed byte offsets".to_string(),
            });
        }
        let (stable, mut telemetry) = self.check_with_telemetry(stable_text, context)?;
        let (neutral, neutral_telemetry) = self.check_with_telemetry(neutral_text, context)?;
        for event in neutral_telemetry {
            if !telemetry.contains(&event) {
                telemetry.push(event);
            }
        }
        Ok((
            union_view_suspects(stable_text, context.manifest, stable, neutral),
            telemetry,
        ))
    }
}

fn union_view_suspects(
    stable_text: &str,
    manifest: &Manifest,
    mut stable: Vec<LeakSuspect>,
    neutral: Vec<LeakSuspect>,
) -> Vec<LeakSuspect> {
    for suspect in stable.iter_mut() {
        suspect.raw_label.push_str(";view=stable");
    }
    let tokens = manifest
        .spans
        .iter()
        .map(|emitted| emitted.clean_span.clone())
        .collect::<Vec<_>>();
    // The stable scan owns every byte it already flagged. A different parent span from the
    // neutral scan may name the very same exposed gap; passing both makes Resolve reject the
    // batch as an overlap and use the one-way fallback.
    let mut covered = tokens.clone();
    for suspect in &stable {
        covered.extend(exposed_gaps(suspect.span.clone(), &tokens));
    }
    for suspect in neutral {
        if let Some(existing) = stable.iter_mut().find(|prior| {
            prior.class == suspect.class && actionable_span(prior) == actionable_span(&suspect)
        }) {
            if existing.raw_label.ends_with(";view=stable") {
                existing.raw_label.push_str("+neutral");
            }
        }
        for gap in exposed_gaps(suspect.span.clone(), &covered) {
            let mut projected = suspect.clone();
            projected.span = gap.clone();
            projected.kind = LeakKind::Uncovered;
            if !plausible_neutral_finding(stable_text, &projected) {
                continue;
            }
            projected.raw_label.push_str(";view=neutral");
            covered.push(gap);
            stable.push(projected);
        }
    }
    stable
}

fn actionable_span(suspect: &LeakSuspect) -> Range<usize> {
    match &suspect.kind {
        LeakKind::PartialBleed { uncovered } => uncovered.clone(),
        _ => suspect.span.clone(),
    }
}

fn exposed_gaps(span: Range<usize>, covered: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut gaps = vec![span];
    for block in covered {
        gaps = gaps
            .into_iter()
            .flat_map(|gap| {
                if gap.end <= block.start || block.end <= gap.start {
                    return vec![gap];
                }
                let mut remainder = Vec::with_capacity(2);
                if gap.start < block.start {
                    remainder.push(gap.start..block.start);
                }
                if block.end < gap.end {
                    remainder.push(block.end..gap.end);
                }
                remainder
            })
            .collect();
    }
    gaps
}

fn plausible_neutral_finding(text: &str, suspect: &LeakSuspect) -> bool {
    let actionable_span = match &suspect.kind {
        LeakKind::PartialBleed { uncovered } => uncovered,
        _ => &suspect.span,
    };
    let Some(value) = text.get(actionable_span.clone()) else {
        // An invalid span still reaches the pipeline's fail-closed validation.
        return true;
    };
    if value.trim().is_empty() {
        return false;
    }
    match &suspect.class {
        // A single character cannot be a complete date, even when it touches a token.
        gaze_types::PiiClass::Custom(class) if class == "date" => value.chars().count() != 1,
        // A room or other subunit number is not a building's street number.
        gaze_types::PiiClass::Custom(class) if class == "building_number" => {
            !preceding_subunit_cue(text, actionable_span.start)
        }
        _ => true,
    }
}

fn preceding_subunit_cue(text: &str, start: usize) -> bool {
    let Some(prefix) = text.get(..start) else {
        return false;
    };
    let prefix = prefix.trim_end_matches(|ch: char| ch.is_whitespace() || "#:=,;\"'".contains(ch));
    let key = prefix
        .rsplit(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let cue = key
        .strip_suffix("_number")
        .or_else(|| key.strip_suffix("number"))
        .or_else(|| key.strip_suffix("_no"))
        .unwrap_or(&key);
    [
        "room",
        "suite",
        "unit",
        "apartment",
        "apt",
        "office",
        "floor",
        "desk",
    ]
    .contains(&cue)
}

/// Hooks for tests that replay captured model output through the production decoder.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support {
    use std::ops::Range;

    pub use super::decode::{PieceScore, ScoredPieces};
    use super::*;

    /// A decoded span: byte range, label, score.
    pub type DecodedSpan = (Range<usize>, NymLabel, f32);

    /// Runs the production decoder over captured tokenizer char offsets and piece scores.
    pub fn decode_captured(
        text: &str,
        char_offsets: &[(usize, usize)],
        scores: &[PieceScore],
        operating_point: &NymOperatingPoint,
    ) -> Result<Vec<DecodedSpan>, SafetyNetError> {
        Ok(
            decode::decode_pieces(text, char_offsets, scores, operating_point)?
                .into_iter()
                .map(|span| (span.start..span.end, span.label, span.score))
                .collect(),
        )
    }

    /// Tokenizes and scores `text` with the real model: `(char offsets, piece scores)`.
    pub fn capture(net: &NymSafetyNet, text: &str) -> Result<ScoredPieces, SafetyNetError> {
        net.backend()?.score_pieces(text)
    }
}

enum SpanDisposition {
    Suspect(LeakSuspect),
    Refused(SafetyNetRefusalReason),
    Covered,
}

/// Maps a validated span to a suspect, a typed refusal, or manifest coverage.
fn span_to_disposition(
    span: NymSpan,
    clean_text: &str,
    operating_point: &NymOperatingPoint,
    context: SafetyNetContext<'_>,
) -> Result<SpanDisposition, SafetyNetError> {
    let invalid = |message: &str| SafetyNetError::InvalidOutput {
        message: message.to_string(),
    };
    if span.start >= span.end
        || !clean_text.is_char_boundary(span.start)
        || !clean_text.is_char_boundary(span.end)
        || span.end > clean_text.len()
    {
        return Err(invalid("nym returned out-of-bounds span"));
    }
    let class = nym_label_to_pii_class(span.label)
        .map_err(|_| invalid("nym returned unsupported label"))?;
    let threshold = operating_point
        .threshold(span.label)
        .ok_or_else(|| invalid("nym returned a label that is not enabled"))?;
    if is_pagination_number(clean_text, &span, context.field_path) {
        return Ok(SpanDisposition::Refused(
            SafetyNetRefusalReason::NymPaginationKeyV1,
        ));
    }
    let range = span.start..span.end;
    let Some(kind) = context.manifest.diff_against(&range, &class) else {
        return Ok(SpanDisposition::Covered);
    };
    Ok(SpanDisposition::Suspect(LeakSuspect::new(
        range,
        class,
        NYM_SAFETY_NET_ID,
        Some(span.score),
        kind,
        raw_label(span.label, threshold),
        context.field_path.map(str::to_string),
    )))
}

// V1 is deliberately finite: every member is metadata, even when its value is a single digit.
// Compare after ASCII case-folding and removing snake/kebab separators, so camelCase also works.
const PAGINATION_KEYS_V1: &[&str] = &[
    "page",
    "pagenumber",
    "perpage",
    "pagesize",
    "limit",
    "offset",
    "total",
    "count",
    "index",
    "cursor",
    "currentpage",
    "totalpages",
    "totalcount",
    "pagecount",
];

fn is_pagination_key(key: &str) -> bool {
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return false;
    }
    let normalized: String = key
        .bytes()
        .filter(|byte| !matches!(byte, b'_' | b'-'))
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect();
    PAGINATION_KEYS_V1.contains(&normalized.as_str())
}

/// Numeric pagination/count metadata is not an address, regardless of its wrapper syntax.
fn is_pagination_number(text: &str, span: &NymSpan, field_path: Option<&str>) -> bool {
    if span.label != NymLabel::BuildingNumber {
        return false;
    }
    let value = &text[span.start..span.end];
    if !value.bytes().all(|byte| byte.is_ascii_digit())
        || gaze_types::is_inside_word(text, span.start)
        || gaze_types::is_inside_word(text, span.end)
    {
        return false;
    }
    if field_path.is_some_and(|path| {
        let key = path.rsplit(['.', '[']).next().unwrap_or(path);
        let key = key.trim_end_matches(']').trim_matches(['"', '\'']);
        is_pagination_key(key)
            && text[..span.start].trim().is_empty()
            && text[span.end..].trim().is_empty()
    }) {
        return true;
    }

    let before = text[..span.start].trim_end();
    let tail = &text[span.end..];
    let after = tail.trim_start();
    if let Some(prefix) = before.strip_suffix('=') {
        let key_start = prefix.rfind(['?', '&']);
        if let Some(index) = key_start {
            return is_pagination_key(&prefix[index + 1..])
                && matches!(after.chars().next(), None | Some('&' | '#'));
        }
    }
    let Some(prefix) = before.strip_suffix(':') else {
        return false;
    };
    let prefix = prefix.trim_end();
    if let Some(quoted) = prefix.strip_suffix('"') {
        if let Some(open) = quoted.rfind('"') {
            let key = &quoted[open + 1..];
            return is_pagination_key(key)
                && matches!(
                    quoted[..open].trim_end().chars().next_back(),
                    Some('{' | ',')
                )
                && matches!(after.chars().next(), Some(',' | '}'));
        }
    }
    let line = prefix.rsplit('\n').next().unwrap_or(prefix).trim_start();
    let line = line.strip_prefix("- ").unwrap_or(line);
    let yaml_tail = tail.trim_start_matches([' ', '\t']);
    is_pagination_key(line) && matches!(yaml_tail.chars().next(), None | Some('\n' | '\r' | '#'))
}

/// `LABEL>=THRESHOLD`, the audit spelling of which rule fired.
fn raw_label(label: NymLabel, threshold: f32) -> String {
    format!("{label}>={threshold}")
}

#[cfg(test)]
mod tests {
    use gaze::{
        Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, Pipeline, RawDocument,
        SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
    };
    use gaze_types::{DocumentKind, LeakKind, Manifest, PiiClass};

    use super::*;

    fn context(manifest: &Manifest) -> SafetyNetContext<'_> {
        SafetyNetContext::new(
            manifest,
            &[LocaleTag::Global],
            DocumentKind::Text,
            None,
            None,
        )
    }

    fn disposition_for(
        text: &str,
        value: &str,
        label: NymLabel,
        field_path: Option<&str>,
    ) -> SpanDisposition {
        let manifest = Manifest::default();
        let start = text.rfind(value).unwrap();
        let mut ctx = context(&manifest);
        ctx.field_path = field_path;
        span_to_disposition(
            NymSpan {
                start,
                end: start + value.len(),
                label,
                score: 0.99,
            },
            text,
            &NymOperatingPoint::op_b(),
            ctx,
        )
        .unwrap()
    }

    #[test]
    fn view_union_keeps_stable_score_and_audits_both_views() {
        let make = |span, class: &str, score| {
            LeakSuspect::new(
                span,
                PiiClass::custom(class).unwrap(),
                NYM_SAFETY_NET_ID,
                Some(score),
                LeakKind::Uncovered,
                "USERNAME>=0.5",
                None,
            )
        };
        let stable = vec![make(4..10, "username", 0.7)];
        let neutral = vec![
            make(4..10, "username", 0.9),
            make(14..20, "username", 0.8),
            make(14..20, "username", 0.8),
        ];
        let combined = union_view_suspects(&"x".repeat(24), &Manifest::default(), stable, neutral);
        assert_eq!(combined.len(), 2);
        assert_eq!(combined[0].score, Some(0.7));
        assert_eq!(combined[0].raw_label, "USERNAME>=0.5;view=stable+neutral");
        assert_eq!(combined[1].raw_label, "USERNAME>=0.5;view=neutral");
        let stable_only = union_view_suspects(
            &"x".repeat(24),
            &Manifest::default(),
            vec![make(4..10, "username", 0.7)],
            Vec::new(),
        );
        assert_eq!(stable_only[0].raw_label, "USERNAME>=0.5;view=stable");
    }

    #[test]
    fn neutral_overlap_yields_only_novel_exposed_bytes() {
        let make = |span, class: &str, kind| {
            LeakSuspect::new(
                span,
                PiiClass::custom(class).unwrap(),
                NYM_SAFETY_NET_ID,
                Some(0.9),
                kind,
                "synthetic>=0.5",
                None,
            )
        };
        let stable = vec![make(4..12, "license_plate", LeakKind::Uncovered)];
        let neutral = vec![
            make(2..10, "license_plate", LeakKind::Uncovered),
            make(4..12, "username", LeakKind::Uncovered),
        ];
        let combined =
            union_view_suspects("abcdefghijklmnop", &Manifest::default(), stable, neutral);
        assert_eq!(combined.len(), 2);
        assert_eq!(combined[0].span, 4..12);
        assert_eq!(combined[1].span, 2..4);
        assert_eq!(combined[1].raw_label, "synthetic>=0.5;view=neutral");
    }

    #[test]
    fn neutral_partial_parent_does_not_duplicate_a_stable_gap() {
        let token = 8..18;
        let manifest = Manifest::from_spans(vec![gaze_types::EmittedTokenSpan::new(
            token.clone(),
            0..10,
            PiiClass::custom("postal_code").unwrap(),
        )]);
        let make = |span, uncovered| {
            LeakSuspect::new(
                span,
                PiiClass::custom("license_plate").unwrap(),
                NYM_SAFETY_NET_ID,
                Some(0.9),
                LeakKind::PartialBleed { uncovered },
                "LICENSE_PLATE>=0.5",
                None,
            )
        };
        let combined = union_view_suspects(
            "abcdefghijklmnopqrstuvwx",
            &manifest,
            vec![make(4..12, 4..8)],
            vec![make(4..10, 4..8)],
        );
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].span, 4..12);
        assert_eq!(
            combined[0].raw_label,
            "LICENSE_PLATE>=0.5;view=stable+neutral"
        );
    }

    #[test]
    fn neutral_token_overlap_projects_only_exposed_gaps() {
        let manifest = Manifest::from_spans(vec![gaze_types::EmittedTokenSpan::new(
            4..12,
            0..8,
            PiiClass::Name,
        )]);
        let neutral = vec![LeakSuspect::new(
            2..15,
            PiiClass::custom("username").unwrap(),
            NYM_SAFETY_NET_ID,
            Some(0.9),
            LeakKind::PartialBleed { uncovered: 2..4 },
            "USERNAME>=0.5",
            None,
        )];
        let combined = union_view_suspects("abcdefghijklmnop", &manifest, Vec::new(), neutral);
        assert_eq!(
            combined.iter().map(|s| s.span.clone()).collect::<Vec<_>>(),
            vec![2..4, 12..15]
        );
        assert!(combined
            .iter()
            .all(|s| matches!(s.kind, LeakKind::Uncovered)));
    }

    #[test]
    fn exposed_gap_partition_preserves_exact_byte_coverage() {
        for start in 0..8 {
            for end in start + 1..=8 {
                for left in 0..8 {
                    for right in left + 1..=8 {
                        let blocks = [left..right, 3..5];
                        let gaps = exposed_gaps(start..end, &blocks);
                        assert!(gaps.windows(2).all(|pair| pair[0].end <= pair[1].start));
                        for byte in 0..8 {
                            let expected = (start..end).contains(&byte)
                                && !blocks.iter().any(|block| block.contains(&byte));
                            let actual = gaps.iter().any(|gap| gap.contains(&byte));
                            assert_eq!(actual, expected, "{start}..{end}, {blocks:?}, {byte}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn neutral_projection_adds_coverage_without_action_overlap() {
        let make = |span| {
            LeakSuspect::new(
                span,
                PiiClass::custom("username").unwrap(),
                NYM_SAFETY_NET_ID,
                Some(0.9),
                LeakKind::Uncovered,
                "USERNAME>=0.5",
                None,
            )
        };
        for stable_start in 0..8 {
            for stable_end in stable_start + 1..=8 {
                for neutral_start in 0..8 {
                    for neutral_end in neutral_start + 1..=8 {
                        let stable_span = stable_start..stable_end;
                        let neutral_span = neutral_start..neutral_end;
                        let combined = union_view_suspects(
                            "abcdefgh",
                            &Manifest::default(),
                            vec![make(stable_span.clone())],
                            vec![make(neutral_span.clone())],
                        );
                        assert_eq!(combined[0].span, stable_span);
                        for added in &combined[1..] {
                            assert!(
                                added.span.end <= stable_start || stable_end <= added.span.start
                            );
                        }
                        for byte in 0..8 {
                            let expected =
                                stable_span.contains(&byte) || neutral_span.contains(&byte);
                            let actual = combined.iter().any(|s| s.span.contains(&byte));
                            assert_eq!(actual, expected);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn neutral_projection_does_not_tokenize_a_separator() {
        let make = |span| {
            LeakSuspect::new(
                span,
                PiiClass::custom("username").unwrap(),
                NYM_SAFETY_NET_ID,
                Some(0.9),
                LeakKind::Uncovered,
                "USERNAME>=0.5",
                None,
            )
        };
        let combined = union_view_suspects(
            "alpha beta",
            &Manifest::default(),
            vec![make(0..5), make(6..10)],
            vec![make(0..10)],
        );
        assert_eq!(combined.len(), 2);
    }

    struct SyntheticEmailDetector;

    impl Detector for SyntheticEmailDetector {
        fn detect(&self, _input: &str) -> Vec<Detection> {
            vec![Detection::new(
                0.."alice@example.invalid".len(),
                PiiClass::Email,
                "fixture",
            )]
        }
    }

    struct SyntheticOverlappingViews {
        project_neutral: bool,
    }

    impl SafetyNet for SyntheticOverlappingViews {
        fn id(&self) -> &str {
            "synthetic-overlap"
        }

        fn supported_locales(&self) -> &[LocaleTag] {
            &[LocaleTag::Global]
        }

        fn check(
            &self,
            clean_text: &str,
            context: SafetyNetContext<'_>,
        ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
            let Some(start) = clean_text.find("Dr. Schmidt") else {
                return Ok(Vec::new());
            };
            let whole = start..start + "Dr. Schmidt".len();
            let surname = start + "Dr. ".len()..whole.end;
            let make = |span: Range<usize>, class: PiiClass| {
                let kind = context.manifest.diff_against(&span, &class).unwrap();
                LeakSuspect::new(
                    span,
                    class,
                    self.id(),
                    Some(0.9),
                    kind,
                    "synthetic>=0.5",
                    None,
                )
            };
            Ok(vec![
                make(whole.clone(), PiiClass::Name),
                make(surname, PiiClass::Name),
                make(whole, PiiClass::Organization),
            ])
        }

        fn check_with_neutral(
            &self,
            stable_text: &str,
            neutral_text: Option<&str>,
            context: SafetyNetContext<'_>,
        ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
            let mut findings = self.check(stable_text, context)?;
            if !self.project_neutral || neutral_text.is_none() || findings.is_empty() {
                return Ok(findings);
            }
            let stable = vec![findings.remove(0)];
            Ok(union_view_suspects(
                stable_text,
                context.manifest,
                stable,
                findings,
            ))
        }

        fn check_with_neutral_and_telemetry(
            &self,
            stable_text: &str,
            neutral_text: Option<&str>,
            context: SafetyNetContext<'_>,
        ) -> Result<(Vec<LeakSuspect>, Vec<LeakReportTelemetry>), SafetyNetError> {
            self.check_with_neutral(stable_text, neutral_text, context)
                .map(|suspects| (suspects, Vec::new()))
        }
    }

    #[test]
    fn synthetic_unmerged_overlap_falls_back_but_projected_views_restore() {
        let raw = "alice@example.invalid met Dr. Schmidt";
        for projected in [false, true] {
            let pipeline = Pipeline::builder()
                .detector(SyntheticEmailDetector)
                .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(SyntheticOverlappingViews {
                    project_neutral: projected,
                })
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let (clean, manifest, _) = pipeline
                .clean_with_safety_net_policy_detect_context(
                    &session,
                    RawDocument::Text(raw.to_string()),
                    &[LocaleTag::Global],
                    &gaze::DictionaryBundle::default(),
                    SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Redact),
                )
                .unwrap();
            let CleanDocument::Text(clean) = clean else {
                panic!("text expected");
            };
            if projected {
                assert_eq!(manifest.len(), 2);
                assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
                assert!(!clean.contains("[REDACTED:"));
            } else {
                assert!(clean.contains("[REDACTED:name]"));
                assert_ne!(session.restore_strict_text(&clean).unwrap(), raw);
            }
        }
    }

    #[test]
    fn neutral_only_class_guards_keep_street_numbers_and_full_dates() {
        let text = "Room 812 | Suite #22 | house_number: 12 | date: 2001-02-03 | date: 7";
        let suspect = |value: &str, class: &str| {
            let start = text.find(value).unwrap();
            LeakSuspect::new(
                start..start + value.len(),
                PiiClass::custom(class).unwrap(),
                NYM_SAFETY_NET_ID,
                Some(0.9),
                LeakKind::Uncovered,
                "synthetic>=0.5",
                None,
            )
        };
        assert!(!plausible_neutral_finding(
            text,
            &suspect("812", "building_number")
        ));
        assert!(!plausible_neutral_finding(
            text,
            &suspect("22", "building_number")
        ));
        let house = text.find("house_number: 12").unwrap() + "house_number: ".len();
        let house_number = LeakSuspect::new(
            house..house + 2,
            PiiClass::custom("building_number").unwrap(),
            NYM_SAFETY_NET_ID,
            Some(0.9),
            LeakKind::Uncovered,
            "BUILDING_NUMBER>=0.5",
            None,
        );
        assert!(plausible_neutral_finding(text, &house_number));
        assert!(plausible_neutral_finding(
            text,
            &suspect("2001-02-03", "date")
        ));
        let last = text.rfind('7').unwrap();
        let lone_day = LeakSuspect::new(
            last..last + 1,
            PiiClass::custom("date").unwrap(),
            NYM_SAFETY_NET_ID,
            Some(0.9),
            LeakKind::Uncovered,
            "DATE>=0.5",
            None,
        );
        assert!(!plausible_neutral_finding(text, &lone_day));
    }

    #[test]
    fn subunit_cue_accepts_plain_and_structured_separators() {
        for prefix in [
            "Room\u{a0}",
            "suite #",
            "unit=",
            "room_number: ",
            "\"roomNumber\": ",
            "apt_no,",
            "office;",
        ] {
            let text = format!("{prefix}812");
            assert!(preceding_subunit_cue(&text, text.len() - 3), "{prefix:?}");
        }
        for prefix in ["house_number: ", "street number ", "building: "] {
            let text = format!("{prefix}12");
            assert!(!preceding_subunit_cue(&text, text.len() - 2), "{prefix:?}");
        }
    }

    #[test]
    fn partial_bleed_guard_checks_only_uncovered_date_bytes() {
        let text = "<deadbeef:Email_1> X";
        let fragment = text.len() - 1;
        let suspect = LeakSuspect::new(
            0..text.len(),
            PiiClass::custom("date").unwrap(),
            NYM_SAFETY_NET_ID,
            Some(0.9),
            LeakKind::PartialBleed {
                uncovered: fragment..fragment + 1,
            },
            "DATE>=0.5",
            None,
        );
        assert!(!plausible_neutral_finding(text, &suspect));
    }

    #[test]
    fn neutral_view_rejects_changed_utf8_boundaries_before_loading_model() {
        let net = NymSafetyNet::new(NymConfig::new("/missing-synthetic-bundle"));
        let manifest = Manifest::default();
        let error = net
            .check_with_neutral("é", Some("ab"), context(&manifest))
            .unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn suspect_carries_id_label_score_and_threshold() {
        let text = "Kennzeichen M-AB 1234 bitte";
        let start = text.find("M-AB").unwrap();
        let manifest = Manifest::default();
        let span = NymSpan {
            start,
            end: start + "M-AB 1234".len(),
            label: NymLabel::LicensePlate,
            score: 0.97,
        };
        let SpanDisposition::Suspect(suspect) =
            span_to_disposition(span, text, &NymOperatingPoint::op_b(), context(&manifest))
                .unwrap()
        else {
            panic!("expected a suspect");
        };
        assert_eq!(suspect.safety_net_id, "nym-small-int8");
        assert_eq!(suspect.raw_label, "LICENSE_PLATE>=0.5");
        assert_eq!(suspect.score, Some(0.97));
        assert_eq!(suspect.class, PiiClass::custom("license_plate").unwrap());
        assert_eq!(suspect.kind, LeakKind::Uncovered);
        assert_eq!(raw_label(NymLabel::DateOfBirth, 0.9), "DATE_OF_BIRTH>=0.9");
    }

    #[test]
    fn nym_building_number_refuses_numeric_pagination_fields() {
        // Keep this expectation independent of the production table: deleting a key must fail.
        for key in [
            "page",
            "pagenumber",
            "perpage",
            "pagesize",
            "limit",
            "offset",
            "total",
            "count",
            "index",
            "cursor",
            "currentpage",
            "totalpages",
            "totalcount",
            "pagecount",
        ] {
            for text in [
                format!("{{\"{key}\":2}}"),
                format!("?{key}=2&house_number=7"),
                format!("{key}: 2\n"),
            ] {
                assert!(
                    matches!(
                        disposition_for(&text, "2", NymLabel::BuildingNumber, None),
                        SpanDisposition::Refused(SafetyNetRefusalReason::NymPaginationKeyV1)
                    ),
                    "{text}"
                );
            }
        }
        for (text, value) in [
            (
                "{\"operation\":\"fetch\",\"caseId\":\"1234567890\",\"page\":1}",
                "1",
            ),
            ("{\"page\" : 12, \"house_number\": 1}", "12"),
            ("{\"page\":\n  123}", "123"),
            ("{\"Page_Number\":2}", "2"),
            ("{\"page-size\":2}", "2"),
            ("{\"pageSize\":2}", "2"),
            ("?perPage=2&limit=20", "20"),
            ("  total_count: 2\n", "2"),
            ("  page_size: 2  # synthetic pagination metadata\n", "2"),
        ] {
            assert!(
                matches!(
                    disposition_for(text, value, NymLabel::BuildingNumber, None),
                    SpanDisposition::Refused(_)
                ),
                "{text}"
            );
        }
        assert!(matches!(
            disposition_for("2", "2", NymLabel::BuildingNumber, Some("$.meta.pageSize")),
            SpanDisposition::Refused(_)
        ));
    }

    #[test]
    fn nym_building_number_keeps_address_digits_and_non_json_values() {
        let manifest = Manifest::default();
        for (text, value) in [
            ("{\"page\": 1, \"house_number\": 7}", "7"),
            ("{\"address\": \"Main Street 1\"}", "1"),
            ("{\"caseId\": \"1234567890\"}", "1234567890"),
            ("{\"orderRef\": \"AB12-34\"}", "AB12-34"),
            ("{\"trackingNumber\": \"TRK-92A4-8B12\"}", "TRK-92A4-8B12"),
            ("page=1", "1"),
            ("{\"page\": \"1\"}", "1"),
            ("{\"house_number\": 12}", "12"),
            ("{\"building\": \"5\"}", "5"),
            ("Hausnummer: 12\n", "12"),
            ("Main Street 12", "12"),
            ("{\"page\":2,\"house_number\":7}", "7"),
            ("{\"page\":2,\"building\":5}", "5"),
            ("?house_number=7&page=2", "7"),
            ("page: 2 nearby house 7", "7"),
            ("{\"page\":2,\"name\":7}", "7"),
            ("{\"homepage_phone\":2}", "2"),
            ("{\"pagex\":2}", "2"),
            ("{\"cursorlike_id\":2}", "2"),
            ("{\"p!age\":2}", "2"),
            ("x\"page\":2}", "2"),
            ("{\"page\":2x}", "2"),
            ("{\"page\":2 Main}", "2"),
            ("?page=2 Main Street 5", "2"),
            ("?page=x2&limit=3", "2"),
            ("?page=2x&limit=3", "2"),
            ("page: 2 Main Street\n", "2"),
            ("page: ٢\n", "٢"),
        ] {
            let start = text.rfind(value).unwrap();
            let span = NymSpan {
                start,
                end: start + value.len(),
                label: NymLabel::BuildingNumber,
                score: 0.99,
            };
            assert!(
                matches!(
                    span_to_disposition(span, text, &NymOperatingPoint::op_b(), context(&manifest))
                        .unwrap(),
                    SpanDisposition::Suspect(_)
                ),
                "{text}"
            );
        }
        assert!(matches!(
            disposition_for(
                "2",
                "2",
                NymLabel::BuildingNumber,
                Some("$.address.house_number")
            ),
            SpanDisposition::Suspect(_)
        ));
        assert!(matches!(
            disposition_for("{\"page\":2}", "2", NymLabel::DateOfBirth, None),
            SpanDisposition::Suspect(_)
        ));
        for text in ["2 Main Street", "Main Street 2"] {
            assert!(matches!(
                disposition_for(text, "2", NymLabel::BuildingNumber, Some("$.page")),
                SpanDisposition::Suspect(_)
            ));
        }
    }

    #[test]
    fn a_span_outside_the_operating_point_is_invalid_output() {
        let manifest = Manifest::default();
        let span = NymSpan {
            start: 0,
            end: 4,
            label: NymLabel::ZipCode,
            score: 0.99,
        };
        assert!(matches!(
            span_to_disposition(
                span,
                "12345",
                &NymOperatingPoint::op_b(),
                context(&manifest)
            ),
            Err(SafetyNetError::InvalidOutput { .. })
        ));
        let span = NymSpan {
            start: 1,
            end: 2,
            label: NymLabel::Username,
            score: 0.99,
        };
        assert!(
            span_to_disposition(span, "ü", &NymOperatingPoint::op_b(), context(&manifest)).is_err()
        );
    }

    #[test]
    fn missing_bundle_fails_closed_and_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let net = NymSafetyNet::new(NymConfig::new(dir.path().join("absent")));
        let manifest = Manifest::default();
        let first = net.check("text", context(&manifest)).unwrap_err();
        let second = net.check("text", context(&manifest)).unwrap_err();
        assert!(matches!(first, SafetyNetError::WeightsMissing { .. }));
        assert_eq!(first, second);
    }
}
