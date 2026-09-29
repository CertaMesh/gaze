//! Nym-small safety net: a pinned multilingual PII token classifier, opt-in.
//!
//! Pass-3 backend that runs `Wismut/nym-pii-multilingual-small` (v3, int8 ONNX) in process through
//! ONNX Runtime. It flags; the pipeline decides. Only allowlisted labels with an explicit
//! threshold can produce a suspect ([`NymOperatingPoint`], op-B by default), every suspect is a
//! whole word or run of words, and every suspect records the label and threshold that fired
//! (`raw_label = "LICENSE_PLATE>=0.5"`) next to its score and the stable id `nym-small-int8`.
//!
//! The bundle is SHA-256 pinned ([`NYM_SMALL_INT8_BUNDLE_SHA256`]) and verified before the model
//! loads. Input longer than one model window is scanned in overlapping windows; a piece no
//! window scored, or a character the tokenizer did not cover, is a typed error, never a silent
//! skip.

use std::sync::{Arc, OnceLock};

pub use gaze_types::nym::{
    nym_label_to_pii_class, nym_label_to_safety_net_class, NymConfigError, NymLabel,
    NymOperatingPoint, NYM_SAFETY_NET_ID,
};
use gaze_types::{
    LeakReportTelemetry, LeakSuspect, LocaleTag, SafetyNet, SafetyNetContext, SafetyNetError,
    SafetyNetRefusalReason,
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
            ("x\"page\":2}", "2"),
            ("{\"page\":2x}", "2"),
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
