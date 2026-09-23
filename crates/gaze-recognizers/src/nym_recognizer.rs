//! Nym-small as a recognizer (single-pass Stage A, opt-in).
//!
//! The Nym safety net runs after the rules and flags what they missed. This module puts the
//! same pinned model into the candidate pool instead: one [`NymLabelRecognizer`] per enabled
//! label of the operating point, each a plain [`Recognizer`] that competes with the rules under
//! the same resolver, action policy and residual coverage.
//!
//! * **Input.** An adapter reads the normalized text every recognizer sees, never the raw
//!   input, and its spans are offsets into that text like every other candidate's.
//! * **One inference per request.** The adapters share one loaded model and one inference
//!   result per detection request, held in the request's [`gaze_types::DetectMemo`] under a key
//!   made of a digest of the input, the model revision and the operating point. Nothing is
//!   cached across requests and no input text is kept.
//! * **Closed label set.** Adapters exist only for the labels the validated
//!   [`NymOperatingPoint`] enables; a label outside it (TAX_ID and ZIP_CODE are off by default)
//!   is rejected when the operating point is built. Model output that names a disabled label or
//!   an impossible span fails the request ([`DetectError`]), never a silent skip.
//! * **Lowest standing.** A candidate carries no canonical form, the lowest rule priority and a
//!   `nym/<label>` source, so the resolver places it in the learned evidence tier: a rule
//!   container swallows it, it never swallows a rule candidate, and a rule wins every other
//!   overlap. Its bytes outside the rule token stay protected by residual coverage.
//! * **Audit.** The recognizer id and source are `nym/<label>` (`nym/license_plate`, lowercase
//!   like every stable source id); the version id names the model
//!   revision, the rule that fired (`LABEL>=THRESHOLD`), the whole operating point and the input
//!   representation.
//!
//! Nothing here is registered by default: a caller loads [`NymRecognizers`] and registers the
//! adapters. The classes the adapters emit need explicit policy actions
//! ([`NymRecognizers::classes`]).

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gaze_types::nym::{
    nym_label_to_pii_class, NymLabel, NymOperatingPoint, NYM_RECOGNIZER_SOURCE_PREFIX,
    NYM_SAFETY_NET_ID,
};
use gaze_types::{
    is_inside_word, Candidate, ConflictTier, DetectContext, DetectError, LocaleBasis, PiiClass,
    Recognizer, SafetyNetError,
};
use sha2::{Digest, Sha256};

use crate::safety_net::nym::{NymConfig, NymOrtBackend, NymSpan, NYM_SMALL_HF_COMMIT};

/// Rule priority of every Nym candidate: below any rule, so a rule of the same class wins.
pub const NYM_RECOGNIZER_PRIORITY: i32 = i32::MIN;

/// The frozen recognizer operating point: the per-label thresholds chosen on the development
/// split (never the evaluation split), with the selection rule and its evidence.
pub const NYM_RECOGNIZER_OPERATING_POINT_JSON: &str =
    include_str!("../nym-recognizer-operating-point.json");

/// Why the frozen operating point could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrozenOperatingPointError {
    /// The committed file is not the expected JSON shape.
    #[error("frozen nym recognizer operating point is malformed: {0}")]
    Malformed(String),
    /// The thresholds fail the closed-label-set validation.
    #[error(transparent)]
    Invalid(#[from] gaze_types::nym::NymConfigError),
}

/// The frozen recognizer operating point ([`NYM_RECOGNIZER_OPERATING_POINT_JSON`]), validated
/// like any policy spelling: unknown, unmapped or threshold-less labels fail.
pub fn recognizer_operating_point() -> Result<NymOperatingPoint, FrozenOperatingPointError> {
    #[derive(serde::Deserialize)]
    struct Frozen {
        labels: Vec<String>,
        thresholds: std::collections::BTreeMap<String, f32>,
    }
    let frozen: Frozen = serde_json::from_str(NYM_RECOGNIZER_OPERATING_POINT_JSON)
        .map_err(|error| FrozenOperatingPointError::Malformed(error.to_string()))?;
    Ok(NymOperatingPoint::from_labels_and_thresholds(
        &frozen.labels,
        &frozen.thresholds,
    )?)
}

/// The text representation the model reads, recorded in every version id.
pub const NYM_RECOGNIZER_INPUT: &str = "normalized";

/// Owner name of the shared inference result in the request memo.
const MEMO_OWNER: &str = "gaze-recognizers/nym-recognizer";

/// One model call over a text, decoded to word-aligned spans of the enabled labels.
pub(crate) trait NymInference: Send + Sync {
    fn infer(&self, text: &str) -> Result<Vec<NymSpan>, SafetyNetError>;
}

impl NymInference for NymOrtBackend {
    fn infer(&self, text: &str) -> Result<Vec<NymSpan>, SafetyNetError> {
        NymOrtBackend::infer(self, text)
    }
}

struct Shared {
    inference: Arc<dyn NymInference>,
    operating_point: NymOperatingPoint,
    /// `nym-small-int8@<commit>`.
    model_revision: String,
    /// `LABEL>=THRESHOLD,...` in label order.
    operating_point_spelling: String,
}

impl Shared {
    /// The request's inference result, computed by the first adapter that asks.
    fn spans(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> Result<Rc<Vec<NymSpan>>, SafetyNetError> {
        ctx.memo()
            .get_or_try_insert_with(MEMO_OWNER, &self.memo_key(input), || self.checked(input))
    }

    /// Everything the inference result depends on: the input, the model and the operating
    /// point. A digest, so the memo never holds input text.
    fn memo_key(&self, input: &str) -> Vec<u8> {
        let mut digest = Sha256::new();
        for part in [
            input.as_bytes(),
            self.model_revision.as_bytes(),
            self.operating_point_spelling.as_bytes(),
        ] {
            digest.update((part.len() as u64).to_le_bytes());
            digest.update(part);
        }
        digest.finalize().to_vec()
    }

    /// Runs the model and refuses any span the decoder can never produce.
    fn checked(&self, input: &str) -> Result<Vec<NymSpan>, SafetyNetError> {
        let spans = self.inference.infer(input)?;
        for span in &spans {
            check_span(input, span, &self.operating_point)?;
        }
        Ok(spans)
    }
}

fn check_span(
    input: &str,
    span: &NymSpan,
    operating_point: &NymOperatingPoint,
) -> Result<(), SafetyNetError> {
    let invalid = |message: &str| SafetyNetError::InvalidOutput {
        message: message.to_string(),
    };
    if span.start >= span.end
        || span.end > input.len()
        || !input.is_char_boundary(span.start)
        || !input.is_char_boundary(span.end)
    {
        return Err(invalid("nym returned out-of-bounds span"));
    }
    let threshold = operating_point
        .threshold(span.label)
        .ok_or_else(|| invalid("nym returned a label that is not enabled"))?;
    if !(span.score.is_finite() && span.score >= threshold) {
        return Err(invalid("nym returned a span below its threshold"));
    }
    if is_inside_word(input, span.start) || is_inside_word(input, span.end) {
        return Err(invalid("nym returned a span that cuts a word"));
    }
    Ok(())
}

/// The loaded Nym model and its operating point; builds the per-label adapters.
pub struct NymRecognizers {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for NymRecognizers {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NymRecognizers")
            .field("model_revision", &self.shared.model_revision)
            .field("operating_point", &self.shared.operating_point_spelling)
            .finish_non_exhaustive()
    }
}

impl NymRecognizers {
    /// Verifies the pinned bundle and loads the model now, so a bad bundle fails the build of
    /// the pipeline rather than a request.
    pub fn load(config: NymConfig) -> Result<Self, SafetyNetError> {
        let operating_point = config.operating_point().clone();
        let backend = NymOrtBackend::new(config)?;
        Ok(Self::from_inference(
            Arc::new(backend),
            operating_point,
            format!("{NYM_SAFETY_NET_ID}@{NYM_SMALL_HF_COMMIT}"),
        ))
    }

    /// [`Self::load`] with [`NymConfig::from_env`] (`GAZE_NYM_MODEL_DIR`).
    pub fn from_env() -> Result<Self, SafetyNetError> {
        Self::load(NymConfig::from_env()?)
    }

    fn from_inference(
        inference: Arc<dyn NymInference>,
        operating_point: NymOperatingPoint,
        model_revision: String,
    ) -> Self {
        let operating_point_spelling = operating_point
            .iter()
            .map(|(label, threshold)| rule_spelling(label, threshold))
            .collect::<Vec<_>>()
            .join(",");
        Self {
            shared: Arc::new(Shared {
                inference,
                operating_point,
                model_revision,
                operating_point_spelling,
            }),
        }
    }

    /// The operating point the adapters apply.
    pub fn operating_point(&self) -> &NymOperatingPoint {
        &self.shared.operating_point
    }

    /// The classes the adapters emit, in label order. Declare an explicit policy action for
    /// each: a learned class that falls through to a `preserve` default ships its bytes raw.
    pub fn classes(&self) -> Vec<PiiClass> {
        self.adapters()
            .into_iter()
            .map(|adapter| adapter.class)
            .collect()
    }

    /// One adapter per enabled label, in label order, all sharing the loaded model.
    pub fn adapters(&self) -> Vec<NymLabelRecognizer> {
        self.shared
            .operating_point
            .iter()
            .map(|(label, threshold)| {
                let class = nym_label_to_pii_class(label)
                    .expect("a validated operating point enables only mapped labels");
                let id = format!(
                    "{NYM_RECOGNIZER_SOURCE_PREFIX}{}",
                    label.as_str().to_ascii_lowercase()
                );
                let version_id = format!(
                    "{}/{}/op={}/input={NYM_RECOGNIZER_INPUT}",
                    self.shared.model_revision,
                    rule_spelling(label, threshold),
                    self.shared.operating_point_spelling,
                );
                NymLabelRecognizer {
                    label,
                    class,
                    id,
                    version_id,
                    shared: Arc::clone(&self.shared),
                }
            })
            .collect()
    }
}

/// `LABEL>=THRESHOLD`, the audit spelling shared with the Nym safety net.
fn rule_spelling(label: NymLabel, threshold: f32) -> String {
    format!("{label}>={threshold}")
}

/// One enabled Nym label as a recognizer.
pub struct NymLabelRecognizer {
    label: NymLabel,
    class: PiiClass,
    id: String,
    version_id: String,
    shared: Arc<Shared>,
}

impl std::fmt::Debug for NymLabelRecognizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NymLabelRecognizer")
            .field("id", &self.id)
            .field("version_id", &self.version_id)
            .finish_non_exhaustive()
    }
}

impl NymLabelRecognizer {
    /// The Nym label this adapter reports.
    pub fn label(&self) -> NymLabel {
        self.label
    }

    fn candidate(&self, span: Range<usize>, score: f32) -> Candidate {
        Candidate::new(
            span,
            self.class.clone(),
            self.id.clone(),
            score,
            NYM_RECOGNIZER_PRIORITY,
            None,
            self.token_family(),
            self.id.clone(),
            ConflictTier::None,
            Vec::new(),
        )
        .with_recognizer_version_id(self.version_id.clone())
    }
}

impl Recognizer for NymLabelRecognizer {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_class(&self) -> &PiiClass {
        &self.class
    }

    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let spans = self.shared.spans(input, ctx).map_err(|error| {
            tracing::error!(recognizer = %self.id, error = %error, "nym recognizer failed closed");
            DetectError::backend(self.id.clone(), error.to_string())
        })?;
        Ok(spans
            .iter()
            .filter(|span| span.label == self.label)
            .map(|span| self.candidate(span.start..span.end, span.score))
            .collect())
    }

    fn token_family(&self) -> &str {
        "counter"
    }

    /// The model reads every language it was trained on; admission does not depend on the
    /// document locale.
    fn locale_basis(&self) -> LocaleBasis {
        LocaleBasis::Format
    }
}

/// Scripted model output for tests that exercise the adapters without the model bundle.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;

    /// A decoded span the script returns: byte range in the text it was given, label, score.
    pub type ScriptedSpan = (Range<usize>, NymLabel, f32);

    /// What the scripted model saw.
    #[derive(Debug, Default)]
    pub struct ScriptLog {
        calls: AtomicUsize,
        inputs: Mutex<Vec<String>>,
    }

    impl ScriptLog {
        /// Number of inferences run.
        pub fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        /// Every text the model was given, in call order.
        pub fn inputs(&self) -> Vec<String> {
            self.inputs.lock().expect("script log").clone()
        }
    }

    struct Scripted<F> {
        script: F,
        log: Arc<ScriptLog>,
    }

    impl<F> NymInference for Scripted<F>
    where
        F: Fn(&str) -> Result<Vec<ScriptedSpan>, SafetyNetError> + Send + Sync,
    {
        fn infer(&self, text: &str) -> Result<Vec<NymSpan>, SafetyNetError> {
            self.log.calls.fetch_add(1, Ordering::SeqCst);
            self.log
                .inputs
                .lock()
                .expect("script log")
                .push(text.to_string());
            Ok((self.script)(text)?
                .into_iter()
                .map(|(range, label, score)| NymSpan {
                    start: range.start,
                    end: range.end,
                    label,
                    score,
                })
                .collect())
        }
    }

    impl NymRecognizers {
        /// Adapters over a scripted model instead of the pinned bundle.
        pub fn scripted<F>(operating_point: NymOperatingPoint, script: F) -> (Self, Arc<ScriptLog>)
        where
            F: Fn(&str) -> Result<Vec<ScriptedSpan>, SafetyNetError> + Send + Sync + 'static,
        {
            let log = Arc::new(ScriptLog::default());
            let recognizers = Self::from_inference(
                Arc::new(Scripted {
                    script,
                    log: Arc::clone(&log),
                }),
                operating_point,
                "nym-scripted".to_string(),
            );
            (recognizers, log)
        }
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use gaze_types::{DictionaryBundle, LocaleTag};

    use super::test_support::ScriptedSpan;
    use super::*;

    fn plate_at(text: &str, plate: &str) -> ScriptedSpan {
        let start = text.find(plate).expect("plate in text");
        (start..start + plate.len(), NymLabel::LicensePlate, 0.97)
    }

    fn detect_all(
        adapters: &[NymLabelRecognizer],
        text: &str,
        ctx: &DetectContext<'_>,
    ) -> Vec<Candidate> {
        adapters
            .iter()
            .flat_map(|adapter| adapter.detect(text, ctx).expect("detect"))
            .collect()
    }

    #[test]
    fn one_adapter_per_enabled_label_with_audit_identity() {
        let (set, _) = NymRecognizers::scripted(NymOperatingPoint::op_b(), |_| Ok(Vec::new()));
        let adapters = set.adapters();
        assert_eq!(
            adapters
                .iter()
                .map(|a| a.id().to_string())
                .collect::<Vec<_>>(),
            [
                "nym/building_number",
                "nym/date_of_birth",
                "nym/license_plate",
                "nym/username"
            ]
        );
        assert_eq!(
            set.classes()
                .iter()
                .map(PiiClass::to_canonical_str)
                .collect::<Vec<_>>(),
            [
                "custom:building_number",
                "custom:date",
                "custom:license_plate",
                "custom:username"
            ]
        );
        let plate = &adapters[2];
        assert_eq!(
            plate.version_id,
            "nym-scripted/LICENSE_PLATE>=0.5/op=BUILDING_NUMBER>=0.5,DATE_OF_BIRTH>=0.9,\
             LICENSE_PLATE>=0.5,USERNAME>=0.5/input=normalized"
        );
        assert_eq!(plate.locale_basis(), LocaleBasis::Format);
        assert_eq!(
            plate.supported_class(),
            &PiiClass::custom("license_plate").unwrap()
        );
    }

    #[test]
    fn candidates_carry_the_lowest_priority_no_canonical_form_and_the_nym_source() {
        let text = "Kennzeichen M-AB 1234 bitte";
        let (set, _) = NymRecognizers::scripted(NymOperatingPoint::op_b(), move |t| {
            Ok(vec![plate_at(t, "M-AB 1234")])
        });
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        let candidates = detect_all(&set.adapters(), text, &ctx);
        assert_eq!(
            candidates.len(),
            1,
            "only the plate adapter reports the plate"
        );
        let candidate = &candidates[0];
        assert_eq!(&text[candidate.span.clone()], "M-AB 1234");
        assert_eq!(candidate.priority, i32::MIN);
        assert_eq!(candidate.canonical_form, None);
        assert_eq!(candidate.source, "nym/license_plate");
        assert_eq!(candidate.recognizer_id, "nym/license_plate");
        assert!(candidate
            .recognizer_version_id
            .as_deref()
            .is_some_and(|id| id.contains("/LICENSE_PLATE>=0.5/")));
    }

    #[test]
    fn one_inference_per_request_across_every_adapter() {
        let (set, log) = NymRecognizers::scripted(NymOperatingPoint::op_b(), |_| Ok(Vec::new()));
        let adapters = set.adapters();
        assert_eq!(adapters.len(), 4);
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        detect_all(&adapters, "some text", &ctx);
        let narrowed = ctx.narrowed(&[LocaleTag::Global]);
        detect_all(&adapters, "some text", &narrowed);
        assert_eq!(
            log.calls(),
            1,
            "four adapters, two contexts, one request: one inference"
        );
    }

    #[test]
    fn a_new_request_never_sees_the_previous_result() {
        let (set, log) = NymRecognizers::scripted(NymOperatingPoint::op_b(), move |t| {
            Ok(t.find("M-AB 1234")
                .map(|_| plate_at(t, "M-AB 1234"))
                .into_iter()
                .collect())
        });
        let adapters = set.adapters();
        let dictionaries = DictionaryBundle::default();
        let first = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        assert_eq!(
            detect_all(&adapters, "Kennzeichen M-AB 1234", &first).len(),
            1
        );
        let second = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        assert!(detect_all(&adapters, "Kennzeichen unbekannt", &second).is_empty());
        // A context reused for other text re-infers: the key is the input digest.
        assert!(detect_all(&adapters, "Kennzeichen unbekannt", &first).is_empty());
        assert_eq!(log.calls(), 3);
    }

    #[test]
    fn impossible_model_output_fails_closed() {
        let text = "Kennzeichen M-AB 1234 bitte";
        let cases: Vec<(&str, ScriptedSpan)> = vec![
            ("out of bounds", (20..99, NymLabel::LicensePlate, 0.9)),
            ("empty", (5..5, NymLabel::LicensePlate, 0.9)),
            ("disabled label", (12..21, NymLabel::ZipCode, 0.9)),
            ("unmapped label", (12..21, NymLabel::GivenName, 0.9)),
            ("below threshold", (12..21, NymLabel::LicensePlate, 0.2)),
            ("cuts a word", (15..21, NymLabel::LicensePlate, 0.9)),
        ];
        for (case, span) in cases {
            let (set, _) = NymRecognizers::scripted(NymOperatingPoint::op_b(), move |_| {
                Ok(vec![span.clone()])
            });
            let dictionaries = DictionaryBundle::default();
            let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
            let error = set.adapters()[2].detect(text, &ctx).expect_err(case);
            assert!(
                matches!(&error, DetectError::Backend { recognizer_id, .. } if recognizer_id == "nym/license_plate"),
                "{case}: {error}"
            );
            assert!(
                ctx.memo().is_empty(),
                "{case}: a refused result is never memoized"
            );
        }
    }

    #[test]
    fn a_model_error_fails_closed() {
        let (set, _) = NymRecognizers::scripted(NymOperatingPoint::op_b(), |_| {
            Err(SafetyNetError::Runtime {
                message: "nym ort inference failed".to_string(),
            })
        });
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
        assert!(set.adapters()[0].detect("text", &ctx).is_err());
    }

    /// The committed development-split choice (scripts/bench/nym_recognizer_dev_sweep.py);
    /// changing it is a new tuning run, never an edit.
    #[test]
    fn the_frozen_operating_point_is_the_development_split_choice() {
        let frozen = recognizer_operating_point().expect("frozen operating point");
        assert_eq!(
            frozen.iter().collect::<Vec<_>>(),
            [
                (NymLabel::BuildingNumber, 0.4),
                (NymLabel::DateOfBirth, 0.95),
                (NymLabel::LicensePlate, 0.3),
                (NymLabel::Username, 0.3),
            ]
        );
        assert_eq!(frozen.threshold(NymLabel::TaxId), None);
        assert_eq!(frozen.threshold(NymLabel::ZipCode), None);
    }

    #[test]
    fn a_missing_bundle_fails_at_load() {
        let dir = tempfile::tempdir().unwrap();
        let error = NymRecognizers::load(NymConfig::new(dir.path().join("absent"))).unwrap_err();
        assert!(
            matches!(error, SafetyNetError::WeightsMissing { .. }),
            "{error}"
        );
    }
}
