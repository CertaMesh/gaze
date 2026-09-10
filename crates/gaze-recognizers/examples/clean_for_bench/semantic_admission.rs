//! Private experiment. Coordinates refer to Gaze-normalized detector input.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use gaze_types::{
    Candidate, ConflictTier, DetectContext, DetectError, Detection, Detector, LocaleBasis,
    LocaleTag, PhoneCandidateAdmission, PiiClass, Recognizer, Region, ValidatorKind,
};
use serde::Serialize;

const ID: &str = "redact-semantic-candidate-v1";
const PREFIX: &str = "redact-patched-coreml-v1:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Admission {
    Accept,
    NotApplicable,
    SemanticInvalid,
    Error,
}

// Pure: no counters, logs, detector calls, canonical replacement, or thresholds.
fn admit(label: &str, text: &str, locales: &[LocaleTag]) -> Admission {
    match label {
        "credit_card" => {
            if !text
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_whitespace() || b == b'-')
            {
                Admission::NotApplicable
            } else if ValidatorKind::Luhn.validates(text) {
                Admission::Accept
            } else {
                Admission::SemanticInvalid
            }
        }
        "phone" => {
            // Multiple/unknown document regions are ambiguous, not a guessed country.
            let mut explicit = locales
                .iter()
                .filter(|locale| **locale != LocaleTag::Global);
            let region = match (explicit.next(), explicit.next()) {
                (Some(LocaleTag::DeDe), None) => Region::De,
                (Some(LocaleTag::EnUs), None) => Region::Us,
                _ => return Admission::NotApplicable,
            };
            match gaze_types::phone_candidate_admission(region, text) {
                PhoneCandidateAdmission::Accept => Admission::Accept,
                PhoneCandidateAdmission::NotApplicable => Admission::NotApplicable,
                PhoneCandidateAdmission::SemanticInvalid => Admission::SemanticInvalid,
                PhoneCandidateAdmission::Error => Admission::Error,
            }
        }
        _ => Admission::NotApplicable,
    }
}

#[derive(Serialize)]
struct SpanEvidence<'a> {
    start: usize,
    end: usize,
    label: &'a str,
    disposition: Admission,
}

#[derive(Serialize)]
struct BatchEvidence<'a> {
    policy: &'static str,
    coordinates: &'static str,
    batch: u64,
    status: &'static str,
    semantic_invalid_count: usize,
    semantic_invalid_bytes: usize,
    spans: Vec<SpanEvidence<'a>>,
}

#[derive(Clone)]
pub(super) struct AuditSink(Arc<Mutex<Audit>>);

impl AuditSink {
    pub(super) fn new(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self(Arc::new(Mutex::new(Audit {
            file,
            next_batch: 1,
        }))))
    }

    pub(super) fn request<T>(
        &self,
        ordinal: u64,
        run: impl FnOnce() -> Result<T, Box<dyn std::error::Error>>,
        success_status: impl FnOnce(&T) -> &'static str,
    ) -> Result<T, Box<dyn std::error::Error>> {
        self.request_boundary(ordinal, "request_begin")?;
        let result = run();
        let status = match &result {
            Ok(value) => success_status(value),
            Err(_) => "request_error",
        };
        // The caller cannot emit a success until this write has succeeded.
        self.request_boundary(ordinal, status)?;
        result
    }

    // Output-only request framing. Locale and admission never read this state.
    pub(super) fn request_boundary(
        &self,
        request: u64,
        status: &'static str,
    ) -> Result<(), DetectError> {
        #[derive(Serialize)]
        struct Boundary {
            policy: &'static str,
            request: u64,
            status: &'static str,
        }
        let mut audit = self.0.try_lock().map_err(|_| failure("audit_busy"))?;
        write_evidence(
            &mut audit.file,
            &Boundary {
                policy: ID,
                request,
                status,
            },
        )
    }
}

struct Audit {
    file: File,
    next_batch: u64,
}

pub(super) struct SemanticRecognizer<D> {
    detector: D,
    audit: AuditSink,
    class: PiiClass,
}

impl<D> SemanticRecognizer<D> {
    #[cfg(test)]
    pub(super) fn new(detector: D, audit_path: &Path) -> std::io::Result<Self> {
        Ok(Self::with_audit(detector, AuditSink::new(audit_path)?))
    }

    pub(super) fn with_audit(detector: D, audit: AuditSink) -> Self {
        Self {
            detector,
            audit,
            class: PiiClass::Custom("__legacy_detector__".into()),
        }
    }
}

impl<D: Detector> Recognizer for SemanticRecognizer<D> {
    fn id(&self) -> &str {
        ID
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn locale_basis(&self) -> LocaleBasis {
        LocaleBasis::Format
    }

    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let mut audit = self.audit.0.try_lock().map_err(|_| failure("audit_busy"))?;
        let batch = audit.next_batch;
        audit.next_batch = batch.checked_add(1).ok_or_else(|| failure("audit_limit"))?;
        #[derive(Serialize)]
        struct BatchBoundary {
            policy: &'static str,
            batch: u64,
            status: &'static str,
        }
        write_evidence(
            &mut audit.file,
            &BatchBoundary {
                policy: ID,
                batch,
                status: "batch_begin",
            },
        )?;
        // Redact's fallible entrypoint validates the ENTIRE raw 22-label reply,
        // including scores, ordering, UTF-8 bounds and window completion, first.
        let detections = match self.detector.try_detect(input) {
            Ok(detections) => detections,
            Err(error) => {
                write_evidence(
                    &mut audit.file,
                    &BatchBoundary {
                        policy: ID,
                        batch,
                        status: "detector_error",
                    },
                )?;
                return Err(DetectError::backend(error.recognizer_id, error.message));
            }
        };
        // Defend the adapter boundary too; no admission is evaluated during this pass.
        let structure = (|| {
            if detections.len() > 4096 {
                return Err(failure("invalid_batch"));
            }
            let mut end = 0;
            for detection in &detections {
                let label = detection
                    .source
                    .strip_prefix(PREFIX)
                    .ok_or_else(|| failure("invalid_batch"))?;
                let class = gaze_recognizers::redact_live::label_class(&label.to_ascii_uppercase())
                    .map_err(|_| failure("invalid_batch"))?;
                if detection.class != class
                    || detection.span.start < end
                    || detection.span.start >= detection.span.end
                    || input.get(detection.span.clone()).is_none()
                {
                    return Err(failure("invalid_batch"));
                }
                end = detection.span.end;
            }
            Ok(())
        })();
        if let Err(error) = structure {
            write_evidence(
                &mut audit.file,
                &BatchBoundary {
                    policy: ID,
                    batch,
                    status: "adapter_invalid_batch",
                },
            )?;
            return Err(error);
        }
        let mut evidence = BatchEvidence {
            policy: ID,
            coordinates: "detector_input_utf8",
            batch,
            status: "complete",
            semantic_invalid_count: 0,
            semantic_invalid_bytes: 0,
            spans: Vec::new(),
        };
        let mut kept = Vec::new();
        for detection in &detections {
            let label = &detection.source[PREFIX.len()..];
            let disposition = admit(label, &input[detection.span.clone()], ctx.locale_chain);
            evidence.spans.push(SpanEvidence {
                start: detection.span.start,
                end: detection.span.end,
                label,
                disposition,
            });
            match disposition {
                Admission::Accept | Admission::NotApplicable => kept.push(as_candidate(detection)),
                Admission::SemanticInvalid => {
                    evidence.semantic_invalid_count += 1;
                    evidence.semantic_invalid_bytes += detection.span.len();
                }
                Admission::Error => evidence.status = "validator_error",
            }
        }
        // Evidence must be written before any exclusion can affect emitted output.
        write_evidence(&mut audit.file, &evidence)?;
        if evidence.status == "validator_error" {
            return Err(failure("validator_error"));
        }
        Ok(kept)
    }
}

fn failure(code: &'static str) -> DetectError {
    DetectError::backend(ID, code)
}

fn write_evidence(file: &mut File, evidence: &impl Serialize) -> Result<(), DetectError> {
    let mut bytes = serde_json::to_vec(evidence).map_err(|_| failure("audit_encoding"))?;
    if bytes.len() > 1_048_576 {
        return Err(failure("audit_limit"));
    }
    bytes.push(b'\n');
    let written = file.metadata().map_err(|_| failure("audit_write"))?.len();
    if written.saturating_add(bytes.len() as u64) > 64 * 1024 * 1024 {
        return Err(failure("audit_limit"));
    }
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .map_err(|_| failure("audit_write"))
}

fn as_candidate(detection: &Detection) -> Candidate {
    // Match DetectorRecognizer routing and preserve original source/span identity.
    Candidate::new(
        detection.span.clone(),
        detection.class.clone(),
        detection.source.clone(),
        1.0,
        0,
        None,
        "counter",
        detection.source.clone(),
        ConflictTier::None,
        Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::RecognizerRuntimeError;

    struct Fixed(Vec<Detection>);
    impl Detector for Fixed {
        fn detect(&self, _: &str) -> Vec<Detection> {
            self.0.clone()
        }
    }
    fn detection(start: usize, end: usize, label: &str) -> Detection {
        Detection::new(
            start..end,
            gaze_recognizers::redact_live::label_class(&label.to_ascii_uppercase()).unwrap(),
            format!("{PREFIX}{label}"),
        )
    }
    fn recognizer(detections: Vec<Detection>) -> (tempfile::TempDir, SemanticRecognizer<Fixed>) {
        let dir = tempfile::tempdir().unwrap();
        let r =
            SemanticRecognizer::new(Fixed(detections), &dir.path().join("audit.jsonl")).unwrap();
        (dir, r)
    }
    fn evidence(dir: &tempfile::TempDir) -> serde_json::Value {
        serde_json::from_str(
            std::fs::read_to_string(dir.path().join("audit.jsonl"))
                .unwrap()
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn luhn_and_separators_are_bounded_and_pure() {
        for text in ["4111111111111111", "4111-1111 1111\t1111"] {
            assert_eq!(admit("credit_card", text, &[]), Admission::Accept);
        }
        for text in ["4111111111111112", "123"] {
            assert_eq!(admit("credit_card", text, &[]), Admission::SemanticInvalid);
        }
        for text in [
            "４１１１１１１１１１１１１１１２",
            "4111\u{a0}1111 1111 1112",
            "4111/1111/1111/1112",
        ] {
            assert_eq!(admit("credit_card", text, &[]), Admission::NotApplicable);
        }
    }

    #[test]
    fn phone_requires_explicit_unambiguous_supported_document_locale() {
        assert_eq!(
            admit("phone", "01555 0112233", &[LocaleTag::DeDe]),
            Admission::Accept
        );
        assert_eq!(
            admit("phone", "+1-555-0100", &[LocaleTag::EnUs]),
            Admission::Accept
        );
        for locales in [
            vec![],
            vec![LocaleTag::Global],
            vec![LocaleTag::DeAt],
            vec![LocaleTag::EnUs, LocaleTag::DeDe],
        ] {
            assert_eq!(admit("phone", "123", &locales), Admission::NotApplicable);
        }
        for input in ["+999 123", "00 999 123", "１２３", "123\u{a0}456"] {
            assert_eq!(
                admit("phone", input, &[LocaleTag::DeDe]),
                Admission::NotApplicable
            );
        }
        assert_eq!(
            admit("phone", "011 999 123", &[LocaleTag::EnUs]),
            Admission::NotApplicable
        );
        assert_eq!(
            admit("phone", "1", &[LocaleTag::DeDe]),
            Admission::SemanticInvalid
        );
        for locale in [LocaleTag::DeDe, LocaleTag::EnUs] {
            assert_eq!(
                admit("phone", "1", &[locale.clone(), LocaleTag::Global]),
                Admission::SemanticInvalid
            );
        }
        assert_eq!(
            admit(
                "phone",
                "01555 0112233",
                &[LocaleTag::DeDe, LocaleTag::Global]
            ),
            Admission::Accept
        );
        assert_eq!(
            admit(
                "phone",
                "+1-555-0100",
                &[LocaleTag::EnUs, LocaleTag::Global]
            ),
            Admission::Accept
        );
        assert_eq!(
            admit(
                "phone",
                "1",
                &[LocaleTag::DeDe, LocaleTag::EnUs, LocaleTag::Global]
            ),
            Admission::NotApplicable
        );
        for locale in ["en", "de"] {
            assert_eq!(
                admit(
                    "phone",
                    "1",
                    &[LocaleTag::parse(locale).unwrap(), LocaleTag::Global]
                ),
                Admission::NotApplicable
            );
        }
    }

    #[test]
    fn other_twenty_vendor_labels_retain_exact_legacy_candidate_contract() {
        let labels = [
            "given_name",
            "surname",
            "org",
            "street_name",
            "building_number",
            "secondary_address",
            "city",
            "state",
            "email",
            "zip_code",
            "url",
            "ssn",
            "passport",
            "drivers_license",
            "tax_id",
            "bank_account",
            "routing_number",
            "government_id",
            "imei",
            "ip_address",
        ];
        let input = "x".repeat(labels.len());
        let detections = labels
            .iter()
            .enumerate()
            .map(|(i, l)| detection(i, i + 1, l))
            .collect::<Vec<_>>();
        let expected = detections.iter().map(as_candidate).collect::<Vec<_>>();
        let (dir, r) = recognizer(detections);
        let dictionaries = Default::default();
        let ctx = DetectContext::new(&[LocaleTag::DeDe], &dictionaries);
        assert_eq!(r.detect(&input, &ctx).unwrap(), expected);
        assert_eq!(evidence(&dir)["semantic_invalid_count"], 0);
    }

    #[test]
    fn exclusion_has_explicit_text_free_span_and_byte_counters() {
        let (dir, r) = recognizer(vec![
            detection(0, 16, "credit_card"),
            detection(17, 18, "phone"),
        ]);
        let dictionaries = Default::default();
        let ctx = DetectContext::new(&[LocaleTag::DeDe], &dictionaries);
        assert!(r.detect("4111111111111112 1", &ctx).unwrap().is_empty());
        let log = evidence(&dir);
        assert_eq!(log["semantic_invalid_count"], 2);
        assert_eq!(log["semantic_invalid_bytes"], 17);
        assert_eq!(log["coordinates"], "detector_input_utf8");
        assert_eq!(log["spans"][0]["start"], 0);
        assert_eq!(log["spans"][0]["end"], 16);
        assert_eq!(log["spans"][0]["disposition"], "semantic_invalid");
        assert!(!log.to_string().contains("4111111111111112"));
    }

    #[test]
    fn later_malformed_span_prevents_all_semantic_decisions() {
        let (dir, r) = recognizer(vec![
            detection(0, 16, "credit_card"),
            detection(18, 20, "email"),
        ]);
        let dictionaries = Default::default();
        assert!(r
            .detect(
                "4111111111111112 x",
                &DetectContext::new(&[], &dictionaries)
            )
            .is_err());
        assert_eq!(evidence(&dir)["status"], "adapter_invalid_batch");
        assert!(evidence(&dir).get("semantic_invalid_count").is_none());
    }

    #[test]
    fn failed_detector_is_a_whole_request_error_with_no_exclusions() {
        struct Failed;
        impl Detector for Failed {
            fn detect(&self, _: &str) -> Vec<Detection> {
                panic!("fallible entrypoint required")
            }
            fn try_detect(&self, _: &str) -> Result<Vec<Detection>, RecognizerRuntimeError> {
                Err(RecognizerRuntimeError::new(
                    "synthetic",
                    "incomplete_window",
                ))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let r = SemanticRecognizer::new(Failed, &dir.path().join("audit.jsonl")).unwrap();
        let dictionaries = Default::default();
        assert!(r
            .detect("x", &DetectContext::new(&[], &dictionaries))
            .is_err());
        assert_eq!(evidence(&dir)["status"], "detector_error");
        assert!(evidence(&dir).get("semantic_invalid_count").is_none());
    }

    #[test]
    fn actual_floor_and_fullwidth_normalization_preserve_original_restore() {
        use gaze::{Action, LocaleChain, ProtectionContext, RuleSpec, Scope, Session};
        for (raw_card, admitted) in [
            ("４１１１１１１１１１１１１１１１", true),
            ("４１１１１１１１１１１１１１１２", false),
        ] {
            let input = format!("{raw_card} alice@example.invalid");
            let (dir, r) = recognizer(vec![detection(0, 16, "credit_card")]);
            let rulepack = super::super::load_bundled_rulepack("core-extended").unwrap();
            let mut policy = super::super::benchmark_policy(&rulepack, true);
            policy.rules = vec![RuleSpec::Default {
                action: Action::Tokenize,
            }];
            let pipeline = gaze_assembly::build_pipeline_with_recognizer(
                &policy,
                &super::super::empty_context(),
                &[rulepack],
                &LocaleChain::from_tags(vec![LocaleTag::DeDe]),
                None,
                r,
            )
            .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let mut tx = session.begin_transaction();
            let dictionaries = Default::default();
            let clean = pipeline
                .protect_text_transaction(
                    &mut tx,
                    &input,
                    ProtectionContext::strict(&[LocaleTag::DeDe], &dictionaries),
                )
                .unwrap();
            assert!(
                !clean.contains("alice@example.invalid"),
                "floor email must survive admission policy"
            );
            assert_eq!(tx.restore_strict_text(&clean).unwrap(), input);
            let log = evidence(&dir);
            assert_eq!(log["semantic_invalid_count"], usize::from(!admitted));
            assert_eq!(
                log["spans"][0]["end"], 16,
                "normalized, not original fullwidth bytes"
            );
            if admitted {
                assert!(!clean.contains(raw_card));
            }
        }
    }

    #[test]
    fn independent_ner_shaped_detector_is_not_filtered() {
        use gaze::{Action, ProtectionContext, Scope, Session};
        let (_dir, r) = recognizer(vec![detection(0, 16, "credit_card")]);
        let input = "4111111111111112 Dr. Schmidt";
        let ner = Fixed(vec![Detection::new(
            17..input.len(),
            PiiClass::Name,
            "synthetic-ner",
        )]);
        let pipeline = gaze::Pipeline::builder()
            .recognizer(r)
            .detector(ner)
            .rule(gaze::DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let dictionaries = Default::default();
        let clean = pipeline
            .protect_text_transaction(
                &mut tx,
                input,
                ProtectionContext::strict(&[LocaleTag::DeDe], &dictionaries),
            )
            .unwrap();
        assert!(!clean.contains("Dr. Schmidt"));
        assert!(clean.contains("4111111111111112"));
        assert_eq!(tx.restore_strict_text(&clean).unwrap(), input);
    }
    #[test]
    fn audit_request_order_distinguishes_no_batch_from_valid_empty() {
        let (dir, r) = recognizer(Vec::new());
        let dictionaries = Default::default();
        let audit = r.audit.clone();
        let failed: Result<(), Box<dyn std::error::Error>> = audit.request(
            1,
            || Err(Box::new(failure("synthetic_earlier_floor_failure"))),
            |_| "request_success",
        );
        assert!(failed.is_err());
        audit
            .request(
                2,
                || {
                    assert!(r
                        .detect("plain", &DetectContext::new(&[], &dictionaries))?
                        .is_empty());
                    Ok(())
                },
                |_| "request_success",
            )
            .unwrap();
        let log = std::fs::read_to_string(dir.path().join("audit.jsonl")).unwrap();
        let rows = log
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|r| r["status"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "request_begin",
                "request_error",
                "request_begin",
                "batch_begin",
                "complete",
                "request_success"
            ]
        );
        assert_eq!(rows[0]["request"], 1);
        assert_eq!(rows[2]["request"], 2);
        assert_eq!(rows[3]["batch"], 1);
        assert_eq!(rows[4]["spans"], serde_json::json!([]));
    }

    #[test]
    fn later_audit_failure_cannot_release_success() {
        let (dir, r) = recognizer(vec![detection(0, 16, "credit_card")]);
        let dictionaries = Default::default();
        let audit = r.audit.clone();
        let result = audit.request(
            1,
            || {
                assert!(r
                    .detect("4111111111111112", &DetectContext::new(&[], &dictionaries))?
                    .is_empty());
                // A sparse file reaches the cap without allocating/printing text.
                audit.0.lock().unwrap().file.set_len(64 * 1024 * 1024)?;
                Ok(())
            },
            |_| "request_success",
        );
        assert!(
            result.is_err(),
            "audit end failure must prevent a success result"
        );
        let bytes = std::fs::read(dir.path().join("audit.jsonl")).unwrap();
        let prefix =
            std::str::from_utf8(&bytes[..bytes.iter().position(|b| *b == 0).unwrap()]).unwrap();
        assert!(prefix.contains("request_begin"));
        assert!(prefix.contains("semantic_invalid"));
        assert!(!prefix.contains("request_success"));
    }

    #[test]
    fn candidate_configs_preserve_ner_and_floor_choices() {
        use super::super::BenchConfig::*;
        for (candidate, reference) in [
            (Pass2NerRedactSemantic, Pass2NerRedact),
            (RuleFloorRedactSemantic, RuleFloorRedact),
        ] {
            assert_eq!(candidate.uses_ner(), reference.uses_ner());
            assert_eq!(
                candidate.uses_extended_rule_floor(),
                reference.uses_extended_rule_floor()
            );
            assert!(!reference.uses_semantic_admission());
        }
    }
}
