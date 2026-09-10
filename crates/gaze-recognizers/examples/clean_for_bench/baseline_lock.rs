//! Default-off benchmark arm. Assembly rules may be withheld only for an empty Context.
use super::*;
use gaze::experimental_benchmark_baseline_lock::{BenchmarkBaselineLock, BenchmarkLockPolicy, Disposition};
use gaze_recognizers::redact_live::RedactDetector;
use std::cell::RefCell;
use std::fs::OpenOptions;

pub(super) struct Producer {
    baseline: BenchmarkBaselineLock,
    detector: RedactDetector,
    audit: RefCell<Box<dyn Write>>,
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
        Ok(Self { baseline, detector, audit: RefCell::new(Box::new(audit)) })
    }

    fn evidence(&self, ordinal: u64, status: &'static str, dispositions: &[Disposition]) -> Result<(), Box<dyn std::error::Error>> {
        if dispositions.len() > 4096 { return Err("LOCK_AUDIT_LIMIT".into()); }
        let counts = dispositions.iter().fold([0usize; 3], |mut counts, disposition| {
            counts[match disposition {
                Disposition::Admitted => 0,
                Disposition::BaselineOverlap => 1,
                Disposition::SupplementalOverlap => 2,
            }] += 1;
            counts
        });
        let record = serde_json::json!({
            "policy": "baseline-lock-candidate-v1", "request": ordinal, "status": status,
            "admitted": counts[0], "baseline_overlap": counts[1], "supplemental_overlap": counts[2],
        });
        let mut bytes = serde_json::to_vec(&record)?;
        if bytes.len() > 1024 { return Err("LOCK_AUDIT_LIMIT".into()); }
        bytes.push(b'\n');
        let mut audit = self.audit.try_borrow_mut().map_err(|_| "LOCK_AUDIT_BUSY")?;
        audit.write_all(&bytes).and_then(|_| audit.flush()).map_err(|_| "LOCK_AUDIT_WRITE")?;
        Ok(())
    }

    pub(super) fn handle(&self, ordinal: u64, request: Request) -> Result<Outcome, Box<dyn std::error::Error>> {
        self.evidence(ordinal, "request_begin", &[])?;
        let result = self.clean(ordinal, request);
        let status = match &result {
            Ok(Outcome::Success(_)) => "request_success",
            Ok(Outcome::PipelineError { .. }) => "request_refusal",
            Err(_) => "request_error",
        };
        // No successful response reaches stdout until final audit write and flush succeed.
        self.evidence(ordinal, status, &[])?;
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
        self.evidence(ordinal, "batch_complete", &output.dispositions)?;
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
        Producer { baseline: assemble(None, false).unwrap(), detector, audit: RefCell::new(audit) }
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
            assert!(matches!(&runtime, super::super::Producer::Ordinary(_)));
            let actual = semantic_fields(runtime.handle(config, 1, request(raw)).unwrap());
            assert_eq!(actual["clean_text"], text);
            assert_eq!(actual["manifest_spans"], serde_json::to_value(serialize_manifest(manifest)).unwrap());
            assert_eq!(actual["final_protection_trace"], serde_json::to_value(serialize_final_protection_trace(trace)).unwrap());
            assert_eq!(actual["initial_safety_net_stats"], serde_json::to_value(SafetyNetStats::from(&report.stats)).unwrap());
            assert_eq!(restored.0.text, raw);
            assert_eq!(actual["restore"]["exact"], true);
        }
    }

    struct AuditProbe { writes: Rc<RefCell<Vec<Vec<u8>>>>, fail_at: usize }
    impl Write for AuditProbe {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut writes = self.writes.borrow_mut();
            writes.push(bytes.to_vec());
            if writes.len() == self.fail_at { return Err(std::io::Error::other("synthetic failure")); }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    #[test]
    fn audit_begin_batch_and_finalization_failures_never_release_success() {
        for fail_at in [1, 2, 3, usize::MAX] {
            let (_dir, detector) = bridge(json!([]));
            let writes = Rc::new(RefCell::new(Vec::new()));
            let producer = synthetic_producer(detector, Box::new(AuditProbe { writes: writes.clone(), fail_at }));
            let result = producer.handle(1, request("alice@example.invalid"));
            if fail_at == usize::MAX {
                assert!(matches!(result, Ok(Outcome::Success(_))));
                let records = writes.borrow().iter().map(|bytes| serde_json::from_slice::<Value>(bytes).unwrap()).collect::<Vec<_>>();
                assert_eq!(records.iter().map(|record| record["status"].as_str().unwrap()).collect::<Vec<_>>(), ["request_begin", "batch_complete", "request_success"]);
                assert!(!serde_json::to_string(&records).unwrap().contains("alice@example.invalid"));
            } else { assert!(result.is_err()); }
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
