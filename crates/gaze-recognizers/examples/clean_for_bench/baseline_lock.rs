//! Default-off benchmark arm. Assembly rules may be withheld only for an empty Context.
use super::*;
use gaze::experimental_benchmark_baseline_lock::{BenchmarkBaselineLock, BenchmarkLockPolicy, Disposition};
use gaze_recognizers::redact_live::RedactDetector;
use std::cell::RefCell;
use std::fs::OpenOptions;

pub(super) struct Producer {
    baseline: BenchmarkBaselineLock,
    detector: RedactDetector,
    audit: RefCell<Audit>,
}

const MAX_AUDIT_BYTES: usize = 8 * 1024 * 1024;

struct Audit {
    writer: Box<dyn Write>,
    // Reserve before writing, then poison on any failure; partial writes cannot reopen capacity.
    bytes_reserved: usize,
    failure: Option<AuditError>,
}
impl Audit {
    fn new(writer: Box<dyn Write>) -> Self {
        Self { writer, bytes_reserved: 0, failure: None }
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
enum AuditError {
    #[error("LOCK_AUDIT_LIMIT")]
    Limit,
    #[error("LOCK_AUDIT_BUSY")]
    Busy,
    #[error("LOCK_AUDIT_WRITE")]
    Write,
}

enum Lifecycle { Begin, Success, Refusal, Error }
enum AuditEvent<'a> {
    Lifecycle(Lifecycle),
    BatchComplete(&'a [Disposition]),
}

// Same Pass2Ner recipe; the test-only NER-free variant proves assembly plumbing, not model parity.
fn assemble(ner: Option<NerSettings>, require_ner: bool) -> Result<BenchmarkBaselineLock, Box<dyn std::error::Error>> {
    let rulepack = load_bundled_rulepack("core-extended")?;
    let mut policy = benchmark_policy(&rulepack, true);
    let threshold = match ner {
        Some(ner) => {
            let mut settings = NerPolicy::default();
            settings.model_dir = Some(ner.model_dir);
            settings.locale = ner.locale;
            settings.threshold = ner.threshold;
            policy.ner = Some(settings);
            Some(ner.threshold)
        }
        None if require_ner => return Err(BenchmarkBuildError::MissingNerModelDir.into()),
        None => None,
    };
    let locales = benchmark_locale_chain(&policy, &rulepack);
    bind_assembled(policy, empty_context(), rulepack, locales, threshold)
}

fn bind_assembled(
    policy: gaze::Policy, context: Context, rulepack: Rulepack,
    locales: LocaleChain, threshold: Option<f32>,
) -> Result<BenchmarkBaselineLock, Box<dyn std::error::Error>> {
    if !context.dictionaries.is_empty() || !context.class_map.is_empty() || !context.fields.is_empty() {
        return Err("LOCK_UNSUPPORTED_CONTEXT".into());
    }
    let closed = BenchmarkLockPolicy::try_from(policy.rules.clone())?;
    let mut ruleless = policy.clone();
    ruleless.rules.clear();
    let pipeline = gaze_assembly::build_pipeline(&ruleless, &context, &[rulepack], &locales, threshold)?;
    Ok(closed.bind(pipeline)?)
}

impl Producer {
    pub(super) fn build() -> Result<Self, Box<dyn std::error::Error>> {
        // The supplement cannot satisfy the independent assembly floor or rescue failed NER loading.
        let baseline = assemble(Some(ner_settings_from_env()?), true)?;
        let bridge = std::env::var_os("GAZE_REDACT_BRIDGE").ok_or(BenchmarkBuildError::Redact("missing_bridge"))?;
        let model = std::env::var_os("GAZE_REDACT_MODEL_DIR").ok_or(BenchmarkBuildError::Redact("missing_model"))?;
        let detector = RedactDetector::new(PathBuf::from(bridge), PathBuf::from(model))
            .map_err(|e| BenchmarkBuildError::Redact(e.code()))?;
        let path = std::env::var_os("GAZE_REDACT_ADMISSION_AUDIT_FILE").ok_or(BenchmarkBuildError::Redact("missing_admission_audit"))?;
        let audit = OpenOptions::new().write(true).create_new(true).open(path)
            .map_err(|_| BenchmarkBuildError::Redact("admission_audit_open"))?;
        Ok(Self { baseline, detector, audit: RefCell::new(Audit::new(Box::new(audit))) })
    }

    fn evidence(&self, ordinal: u64, event: AuditEvent<'_>) -> Result<(), Box<dyn std::error::Error>> {
        let mut audit = self.audit.try_borrow_mut().map_err(|_| AuditError::Busy)?;
        if let Some(error) = audit.failure { return Err(error.into()); }
        let mut record = serde_json::json!({
            "policy": "baseline-lock-candidate-v1", "request": ordinal,
        });
        let status = match event {
            AuditEvent::Lifecycle(lifecycle) => match lifecycle {
                Lifecycle::Begin => "request_begin",
                Lifecycle::Success => "request_success",
                Lifecycle::Refusal => "request_refusal",
                Lifecycle::Error => "request_error",
            },
            AuditEvent::BatchComplete(dispositions) => {
                if dispositions.len() > 4096 {
                    audit.failure = Some(AuditError::Limit);
                    return Err(AuditError::Limit.into());
                }
                let counts = dispositions.iter().fold([0usize; 3], |mut counts, disposition| {
                    counts[match disposition {
                        Disposition::Admitted => 0,
                        Disposition::BaselineOverlap => 1,
                        Disposition::SupplementalOverlap => 2,
                    }] += 1;
                    counts
                });
                record["admitted"] = counts[0].into();
                record["baseline_overlap"] = counts[1].into();
                record["supplemental_overlap"] = counts[2].into();
                "batch_complete"
            }
        };
        record["status"] = status.into();
        let mut bytes = serde_json::to_vec(&record)?;
        bytes.push(b'\n');
        let next = audit.bytes_reserved.checked_add(bytes.len());
        if bytes.len() > 1024 || next.is_none_or(|next| next > MAX_AUDIT_BYTES) {
            audit.failure = Some(AuditError::Limit);
            return Err(AuditError::Limit.into());
        }
        audit.bytes_reserved = next.ok_or(AuditError::Limit)?;
        if audit.writer.write_all(&bytes).and_then(|_| audit.writer.flush()).is_err() {
            audit.failure = Some(AuditError::Write);
            return Err(AuditError::Write.into());
        }
        Ok(())
    }

    pub(super) fn handle(&self, ordinal: u64, request: Request) -> Result<Outcome, Box<dyn std::error::Error>> {
        self.evidence(ordinal, AuditEvent::Lifecycle(Lifecycle::Begin))?;
        let result = self.clean(ordinal, request);
        let status = match &result {
            Ok(Outcome::Success(_)) => Lifecycle::Success,
            Ok(Outcome::PipelineError { .. }) => Lifecycle::Refusal,
            Err(_) => Lifecycle::Error,
        };
        // No successful response reaches stdout until final audit write and flush succeed.
        self.evidence(ordinal, AuditEvent::Lifecycle(status))?;
        result
    }

    fn clean(&self, ordinal: u64, request: Request) -> Result<Outcome, Box<dyn std::error::Error>> {
        let locales = request.locale_chain.iter().map(|s| LocaleTag::parse(s)).collect::<Result<Vec<_>, _>>()?;
        let prefix = session_hex_for_fixture(&request.fixture_id);
        let start = Instant::now();
        let result = self.baseline.clean_redact_text(&request.text, prefix, &locales, &self.detector);
        let clean_ms = start.elapsed().as_secs_f64() * 1000.0;
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                emit_lock_diagnostic(&error);
                let reason = pipeline_failure_reason(&error).ok_or("unclassified clean-stage pipeline error variant")?;
                return Ok(Outcome::PipelineError { fixture_id: request.fixture_id, stage: "clean", reason, total_ms: clean_ms });
            }
        };
        self.evidence(ordinal, AuditEvent::BatchComplete(&output.dispositions))?;
        observe_clean(
            BenchConfig::Pass2NerRedactBaselineLock, RestoreSource::Locked(&self.baseline),
            request.fixture_id, request.text, output.session, output.text, output.manifest,
            gaze::LeakReport::default(), output.trace, locales, clean_ms,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::os::unix::fs::PermissionsExt;
    use std::rc::Rc;

    fn bridge(spans: Value) -> (tempfile::TempDir, RedactDetector) {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("synthetic-bridge");
        let fixture = json!({"version":1,"kind":"primary","complete":true,"threshold":0.6,"org":true,
            "content_tokens":1,"planned_windows":1,"completed_windows":1,"spans":spans,
            "dispositions":{"threshold":0,"arbitration":0,"cleanup":0,"special_tokens":0},"error":null}).to_string();
        let program = format!(r##"#!/usr/bin/python3
import json, sys
reply = json.loads({fixture:?})
for line in sys.stdin:
    request = json.loads(line)
    reply['id'] = request['id']
    print(json.dumps(reply), flush=True)
"##);
        std::fs::write(&executable, program).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let detector = RedactDetector::new(executable, dir.path().to_path_buf()).unwrap();
        (dir, detector)
    }
    fn request(text: &str) -> Request {
        Request { fixture_id: "synthetic-lock-proof".into(), locale_chain: vec!["de-CH".into(), "en-US".into(), "de-CH".into()], text: text.into() }
    }
    fn semantic_fields(outcome: Outcome) -> Value {
        let Outcome::Success(response) = outcome else { panic!("synthetic request refused"); };
        let mut value = serde_json::to_value(response).unwrap();
        value.as_object_mut().unwrap().remove("timing");
        value
    }
    fn synthetic_producer(detector: RedactDetector, audit: Box<dyn Write>) -> Producer {
        Producer { baseline: assemble(None, false).unwrap(), detector, audit: RefCell::new(Audit::new(audit)) }
    }
    #[test]
    fn embedded_assembly_empty_supplement_matches_all_observed_fields() {
        let ordinary = assemble_rule_floor(BenchConfig::RuleFloorExtended, None).unwrap();
        let (_dir, detector) = bridge(json!([]));
        let producer = synthetic_producer(detector, Box::new(Vec::<u8>::new()));
        for (index, raw) in ["", "\u{200d}\u{200c}", "Email: alice@example.invalid", "Email: ａｌｉｃｅ＠ｅｘａｍｐｌｅ．ｉｎｖａｌｉｄ", "Dr. Schmidt visits Example Research GmbH"].iter().enumerate() {
            let reference = semantic_fields(handle_request(BenchConfig::Pass2Ner, &ordinary, request(raw)).unwrap());
            let candidate = semantic_fields(producer.handle(index as u64 + 1, request(raw)).unwrap());
            assert_eq!(reference, candidate);
            assert_eq!(candidate["restore"]["exact"], true);
        }
    }

    #[test]
    fn original_three_arms_keep_ordinary_dispatch_and_direct_pipeline_results() {
        for config in [BenchConfig::Pass2Ner, BenchConfig::Pass2NerRedact, BenchConfig::Pass2NerRedactSemantic] {
            assert!(!config.uses_baseline_lock(), "production build_producer must select the ordinary recipe");
            let rulepack = load_bundled_rulepack("core-extended").unwrap();
            let policy = benchmark_policy(&rulepack, true);
            let locales = benchmark_locale_chain(&policy, &rulepack);
            let context = empty_context();
            let (_dir, detector) = bridge(json!([{"start":0,"end":2,"label":"GIVEN_NAME","score":0.1}]));
            // Frozen original construction routes, with NER intentionally absent in this plumbing proof.
            let pipeline = match config {
                BenchConfig::Pass2Ner => gaze_assembly::build_pipeline(&policy, &context, &[rulepack], &locales, None).unwrap(),
                BenchConfig::Pass2NerRedact => gaze_assembly::build_pipeline_with_detector(&policy, &context, &[rulepack], &locales, None, detector).unwrap(),
                #[cfg(feature = "phone-parser")]
                BenchConfig::Pass2NerRedactSemantic => {
                    let audit = semantic_admission::AuditSink::new(&_dir.path().join("semantic.jsonl")).unwrap();
                    let recognizer = semantic_admission::SemanticRecognizer::with_audit(detector, audit);
                    gaze_assembly::build_pipeline_with_recognizer(&policy, &context, &[rulepack], &locales, None, recognizer).unwrap()
                }
                #[cfg(not(feature = "phone-parser"))]
                BenchConfig::Pass2NerRedactSemantic => continue,
                _ => unreachable!(),
            };
            let raw = "xy alice@example.invalid";
            let session = Session::new_with_session_hex_for_tests(Scope::Ephemeral, session_hex_for_fixture("synthetic-lock-proof")).unwrap();
            let request_locales = [LocaleTag::DeCh, LocaleTag::EnUs, LocaleTag::DeCh];
            let (clean, manifest, report, trace) = pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session, raw, &request_locales, &Default::default(), safety_net_policy(config)).unwrap();
            let CleanDocument::Text(text) = clean else { panic!("text required"); };
            let restored = pipeline.restore_with_telemetry(&session, &text).unwrap();
            let runtime = super::super::Producer::Ordinary(pipeline);
            let actual = semantic_fields(runtime.handle(config, 1, request(raw)).unwrap());
            assert_eq!(actual["clean_text"], text);
            assert_eq!(actual["manifest_spans"], serde_json::to_value(serialize_manifest(manifest)).unwrap());
            assert_eq!(actual["final_protection_trace"], serde_json::to_value(serialize_final_protection_trace(trace)).unwrap());
            assert_eq!(actual["initial_safety_net_stats"], serde_json::to_value(SafetyNetStats::from(&report.stats)).unwrap());
            assert_eq!(restored.0.text, raw);
            assert_eq!(actual["restore"]["exact"], true);
        }
    }

    #[test]
    fn production_selection_enables_only_the_separate_candidate_arm() {
        for config in [BenchConfig::RuleFloorCore, BenchConfig::RuleFloorExtended,
            BenchConfig::Pass2Ner, BenchConfig::Pass2NerRedact, BenchConfig::RuleFloorRedact,
            BenchConfig::Pass2NerRedactSemantic, BenchConfig::RuleFloorRedactSemantic,
            BenchConfig::FullStackKijiResolve, BenchConfig::FullStackOpfResolve,
            BenchConfig::Pass3Kiji, BenchConfig::Pass3Opf, BenchConfig::Pass3LocaleAware] {
            assert!(!config.uses_baseline_lock());
        }
        assert!(BenchConfig::Pass2NerRedactBaselineLock.uses_baseline_lock());
    }

    struct AuditProbe { writes: Rc<RefCell<Vec<Vec<u8>>>>, fail_at: usize, fail_flush_at: usize }
    impl Write for AuditProbe {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut writes = self.writes.borrow_mut();
            writes.push(bytes.to_vec());
            if writes.len() == self.fail_at { return Err(std::io::Error::other("synthetic failure")); }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.writes.borrow().len() == self.fail_flush_at {
                Err(std::io::Error::other("synthetic flush failure"))
            } else { Ok(()) }
        }
    }
    #[test]
    fn audit_begin_batch_and_finalization_failures_never_release_success() {
        for (fail_at, fail_flush_at) in [(1, usize::MAX), (2, usize::MAX), (3, usize::MAX),
            (usize::MAX, 1), (usize::MAX, 2), (usize::MAX, 3), (usize::MAX, usize::MAX)] {
            let (_dir, detector) = bridge(json!([]));
            let writes = Rc::new(RefCell::new(Vec::new()));
            let producer = synthetic_producer(detector, Box::new(AuditProbe { writes: writes.clone(), fail_at, fail_flush_at }));
            let result = producer.handle(1, request("alice@example.invalid"));
            if fail_at == usize::MAX && fail_flush_at == usize::MAX {
                assert!(matches!(result, Ok(Outcome::Success(_))));
                let records = writes.borrow().iter().map(|bytes| serde_json::from_slice::<Value>(bytes).unwrap()).collect::<Vec<_>>();
                assert_eq!(records.iter().map(|record| record["status"].as_str().unwrap()).collect::<Vec<_>>(), ["request_begin", "batch_complete", "request_success"]);
                assert!(!serde_json::to_string(&records).unwrap().contains("alice@example.invalid"));
            } else {
                assert!(result.is_err());
                let count = writes.borrow().len();
                assert!(producer.handle(2, request("alice@example.invalid")).is_err());
                assert_eq!(writes.borrow().len(), count, "audit failure must remain closed");
            }
        }
    }

    #[test]
    fn lifecycle_records_omit_counts_and_only_complete_batches_report_zero() {
        let (_dir, detector) = bridge(json!([]));
        let writes = Rc::new(RefCell::new(Vec::new()));
        let producer = synthetic_producer(detector, Box::new(AuditProbe {
            writes: writes.clone(), fail_at: usize::MAX, fail_flush_at: usize::MAX,
        }));
        for lifecycle in [Lifecycle::Begin, Lifecycle::Refusal, Lifecycle::Error, Lifecycle::Success] {
            producer.evidence(1, AuditEvent::Lifecycle(lifecycle)).unwrap();
        }
        producer.evidence(1, AuditEvent::BatchComplete(&[])).unwrap();
        producer.evidence(2, AuditEvent::BatchComplete(&[Disposition::Admitted, Disposition::BaselineOverlap])).unwrap();
        let records = writes.borrow().iter().map(|bytes| serde_json::from_slice::<Value>(bytes).unwrap()).collect::<Vec<_>>();
        for lifecycle in &records[..4] {
            for key in ["admitted", "baseline_overlap", "supplemental_overlap"] {
                assert!(lifecycle.get(key).is_none(), "unknown batch must not report zero");
            }
        }
        for key in ["admitted", "baseline_overlap", "supplemental_overlap"] { assert_eq!(records[4][key], 0); }
        assert_eq!(records[5]["admitted"], 1);
        assert_eq!(records[5]["baseline_overlap"], 1);
    }

    #[test]
    fn total_audit_cap_including_final_record_refuses_without_more_writes() {
        let (_dir, detector) = bridge(json!([]));
        let writes = Rc::new(RefCell::new(Vec::new()));
        let producer = synthetic_producer(detector, Box::new(AuditProbe {
            writes: writes.clone(), fail_at: usize::MAX, fail_flush_at: usize::MAX,
        }));
        assert!(matches!(producer.handle(1, request("alice@example.invalid")), Ok(Outcome::Success(_))));
        let lengths = writes.borrow().iter().map(Vec::len).collect::<Vec<_>>();
        let total: usize = lengths.iter().sum();
        assert_eq!(lengths.len(), 3);
        // Same ordinal gives identical record sizes. Each boundary fails one byte before it fits.
        for allowed in [lengths[0] - 1, lengths[0] + lengths[1] - 1, total - 1] {
            let (_dir, detector) = bridge(json!([]));
            let writes = Rc::new(RefCell::new(Vec::new()));
            let producer = synthetic_producer(detector, Box::new(AuditProbe {
                writes: writes.clone(), fail_at: usize::MAX, fail_flush_at: usize::MAX,
            }));
            producer.audit.borrow_mut().bytes_reserved = MAX_AUDIT_BYTES - allowed;
            assert!(producer.handle(1, request("alice@example.invalid")).is_err());
            assert!(producer.audit.borrow().bytes_reserved <= MAX_AUDIT_BYTES);
            assert!(writes.borrow().iter().map(Vec::len).sum::<usize>() <= allowed);
            let count = writes.borrow().len();
            assert!(producer.handle(2, request("alice@example.invalid")).is_err());
            assert_eq!(writes.borrow().len(), count);
        }
    }
    #[test]
    fn candidate_preflights_context_policy_floor_and_missing_ner() {
        assert!(assemble(None, true).is_err());
        assert!(build_pipeline_internal(BenchConfig::Pass2NerRedactBaselineLock,
            #[cfg(all(feature = "redact-live", feature = "phone-parser", unix))]
            None,
        ).is_err());
        for context_field in 0..3 {
            let rulepack = load_bundled_rulepack("core-extended").unwrap();
            let policy = benchmark_policy(&rulepack, true);
            let locales = benchmark_locale_chain(&policy, &rulepack);
            let mut context = empty_context();
            match context_field {
                0 => { context.fields.insert("synthetic".into(), json!(true)); }
                1 => { context.class_map.insert("synthetic".into(), PiiClass::Name); }
                _ => { context.dictionaries.insert("synthetic".into(), gaze::ContextDictionary { terms: vec!["synthetic".into()], case_sensitive: true }); }
            }
            assert!(bind_assembled(policy, context, rulepack, locales, None).is_err());
        }
    }
}

// Emit only our closed internal codes, never a backend's arbitrary message.
fn emit_lock_diagnostic(error: &gaze::Error) {
    let reason = match error {
        gaze::Error::RecognizerDetect(gaze_types::DetectError::Backend { recognizer_id, message })
            if recognizer_id == "benchmark.baseline_lock" => match message.as_str() {
                "LOCK_UNSUPPORTED_SCOPE" => "unsupported_scope",
                "LOCK_MISSING_BASELINE" => "missing_baseline",
                "LOCK_INVALID_BATCH" => "invalid_batch",
                "LOCK_INVALID_MAP" => "invalid_map",
                "LOCK_UNSUPPORTED_ACTION" => "unsupported_action",
                "LOCK_INCOMPLETE" => "incomplete",
                "LOCK_PROVIDER_FAILED" => "provider_failed",
                "LOCK_UNKNOWN_LABEL" => "unknown_label",
                _ => "other_failure",
            },
        _ => "baseline_or_emission_failure",
    };
    eprintln!("gaze_bench_baseline_lock stage=clean reason={reason}");
}
