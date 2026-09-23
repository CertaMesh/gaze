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
use gaze_types::{LeakSuspect, LocaleTag, SafetyNet, SafetyNetContext, SafetyNetError};

pub mod artifacts;
pub(crate) mod decode;
mod ort;

pub use artifacts::{
    verify_nym_bundle, NYM_SMALL_CHECKSUM_FILE, NYM_SMALL_HF_COMMIT, NYM_SMALL_HF_REPO,
    NYM_SMALL_INT8_BUNDLE_SHA256, NYM_SMALL_INT8_SHA256SUMS, NYM_SMALL_UPSTREAM_FILES,
    REQUIRED_NYM_SMALL_ARTIFACTS,
};
pub use ort::{NymConfig, DEFAULT_NYM_INTRA_THREADS};

pub(crate) use decode::NymSpan;
pub(crate) use ort::NymOrtBackend;

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
        let backend = self.backend()?;
        let spans = backend.infer(clean_text)?;
        let mut suspects = Vec::with_capacity(spans.len());
        for span in spans {
            if let Some(suspect) =
                span_to_suspect(span, clean_text, backend.operating_point(), context)?
            {
                suspects.push(suspect);
            }
        }
        Ok(suspects)
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

/// Maps a decoded span to a suspect, or `None` when the manifest already covers it.
fn span_to_suspect(
    span: NymSpan,
    clean_text: &str,
    operating_point: &NymOperatingPoint,
    context: SafetyNetContext<'_>,
) -> Result<Option<LeakSuspect>, SafetyNetError> {
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
    let range = span.start..span.end;
    let Some(kind) = context.manifest.diff_against(&range, &class) else {
        return Ok(None);
    };
    Ok(Some(LeakSuspect::new(
        range,
        class,
        NYM_SAFETY_NET_ID,
        Some(span.score),
        kind,
        raw_label(span.label, threshold),
        context.field_path.map(str::to_string),
    )))
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
        let suspect = span_to_suspect(span, text, &NymOperatingPoint::op_b(), context(&manifest))
            .unwrap()
            .unwrap();
        assert_eq!(suspect.safety_net_id, "nym-small-int8");
        assert_eq!(suspect.raw_label, "LICENSE_PLATE>=0.5");
        assert_eq!(suspect.score, Some(0.97));
        assert_eq!(suspect.class, PiiClass::custom("license_plate").unwrap());
        assert_eq!(suspect.kind, LeakKind::Uncovered);
        assert_eq!(raw_label(NymLabel::DateOfBirth, 0.9), "DATE_OF_BIRTH>=0.9");
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
            span_to_suspect(
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
            span_to_suspect(span, "ü", &NymOperatingPoint::op_b(), context(&manifest)).is_err()
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
