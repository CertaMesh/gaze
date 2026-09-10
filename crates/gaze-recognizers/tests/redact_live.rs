#![cfg(unix)]
use gaze::{
    Action, Context, Detector, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Policy,
    ProtectionContext, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::redact_live::{label_class, BridgeError, RedactDetector};
use std::path::PathBuf;

fn live() -> RedactDetector {
    let bridge = std::env::var_os("GAZE_REDACT_BRIDGE").expect("missing private bridge");
    let model = std::env::var_os("GAZE_REDACT_MODEL_DIR").expect("missing verified bundle");
    RedactDetector::new(PathBuf::from(bridge), PathBuf::from(model)).unwrap()
}

#[test]
fn taxonomy_does_not_claim_narrow_validator_equivalences() {
    assert_eq!(
        label_class("BANK_ACCOUNT"),
        Ok(PiiClass::Custom("bank_account".into()))
    );
    assert_eq!(
        label_class("IP_ADDRESS"),
        Ok(PiiClass::Custom("redact_network_identifier".into()))
    );
    assert_eq!(label_class("ORG"), Ok(PiiClass::Organization));
    assert_eq!(label_class("DATE"), Err(BridgeError::UnknownLabel));
    assert_eq!(label_class("SECRET"), Err(BridgeError::UnknownLabel));
}

#[test]
#[ignore = "requires allocated local CoreML inference slot and private bridge"]
fn real_multiline_multiclass_primary_preserves_floor_and_exact_restore() {
    let detector = live();
    let input = "Dr. Schmidt\nEmail:\talice@example.invalid\nCity: Berlin   Street: Hauptstrasse 12\nOrganization: Example Research GmbH\nPlay Synthetic Artist";
    let detections = detector
        .try_detect(input)
        .expect("real complete hybrid batch");
    assert!(
        detections.iter().any(|d| d.class == PiiClass::Email),
        "email class missing"
    );
    assert!(
        detections.iter().any(|d| d.class != PiiClass::Email),
        "multiclass batch required"
    );
    for detection in &detections {
        assert!(
            input.is_char_boundary(detection.span.start)
                && input.is_char_boundary(detection.span.end)
        );
    }
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let context = Context {
        dictionaries: [(
            "artists".into(),
            gaze::ContextDictionary {
                terms: vec!["Synthetic Artist".into()],
                case_sensitive: true,
            },
        )]
        .into_iter()
        .collect(),
        class_map: [("artists".into(), PiiClass::Custom("artist".into()))]
            .into_iter()
            .collect(),
        fields: Default::default(),
    };
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        gaze_recognizers::embedded("core").unwrap(),
    ))
    .unwrap();
    let locales =
        LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs, LocaleTag::DeDe]), None);
    let baseline = gaze_assembly::build_pipeline(
        &policy,
        &context,
        std::slice::from_ref(&rulepack),
        &locales,
        None,
    )
    .unwrap();
    let dictionaries = gaze::dictionary_bundle_from_context(&context);
    for (bundle, expected_protected) in
        [(&DictionaryBundle::default(), false), (&dictionaries, true)]
    {
        let baseline_session = Session::new(Scope::Ephemeral).unwrap();
        let mut baseline_tx = baseline_session.begin_transaction();
        let baseline_clean = baseline
            .protect_text_transaction(
                &mut baseline_tx,
                input,
                ProtectionContext::strict(&[LocaleTag::EnUs, LocaleTag::DeDe], bundle),
            )
            .unwrap();
        assert_eq!(
            !baseline_clean.contains("Synthetic Artist"),
            expected_protected,
            "baseline dictionary registration requires its runtime bundle"
        );
        assert!(baseline_tx.restore_strict_text(&baseline_clean).unwrap() == input);
    }
    let pipeline = gaze_assembly::build_pipeline_with_detector(
        &policy,
        &context,
        &[rulepack],
        &locales,
        None,
        detector,
    )
    .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let clean = pipeline
        .protect_text_transaction(
            &mut tx,
            input,
            ProtectionContext::strict(&[LocaleTag::EnUs, LocaleTag::DeDe], &dictionaries),
        )
        .unwrap();
    assert!(
        !clean.contains("alice@example.invalid"),
        "floor email escaped"
    );
    assert!(
        !clean.contains("Synthetic Artist"),
        "tenant dictionary escaped"
    );
    assert!(
        tx.restore_strict_text(&clean).unwrap() == input,
        "restore differs"
    );
    assert!(session.tokens().is_empty(), "uncommitted state escaped");
    tx.commit().unwrap();
    assert!(!session.tokens().is_empty());
}

#[test]
#[ignore = "requires allocated local CoreML inference slot and private bridge"]
fn real_whitespace_unicode_tail_and_typed_limits() {
    let detector = live();
    for input in [
        "Dr. Schmidt\nalice@example.invalid",
        "Dr. Schmidt\talice@example.invalid",
        "Dr.  Schmidt   alice@example.invalid",
        "😀 Dr. Schmidt\n\talice@example.invalid",
    ] {
        assert!(
            detector.try_detect(input).is_ok(),
            "ordinary whitespace/Unicode rejected"
        );
    }
    let tail = format!(
        "{}\nDr. Schmidt alice@example.invalid",
        "plain words ".repeat(300)
    );
    let detections = detector
        .try_detect(&tail)
        .expect("complete multiple windows");
    assert!(
        detections
            .iter()
            .any(|d| d.class == PiiClass::Email && d.span.end == tail.len()),
        "window tail missing"
    );
    let error = detector.try_detect(&"x".repeat(65_537)).unwrap_err();
    assert_eq!(error.message, "limit");
    let error = detector.try_detect("ﬁ").unwrap_err();
    assert_eq!(error.message, "alignment");
}

fn peer(body: &str) -> (tempfile::TempDir, RedactDetector) {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("peer");
    std::fs::write(
        &script,
        format!("#!/usr/bin/python3\nimport json,sys,time\n{body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let detector = RedactDetector::new(script, temp.path().to_path_buf()).unwrap();
    (temp, detector)
}
const REPLY: &str = r#"
for line in sys.stdin:
 r=json.loads(line)
 out={'version':1,'id':r['id'],'kind':'primary','complete':True,'threshold':0.6,'org':True,'content_tokens':1,'planned_windows':1,'completed_windows':1,'spans':[{'start':0,'end':1,'label':'GIVEN_NAME','score':0.8},{'start':2,'end':3,'label':'ORG','score':0.9}], 'dispositions':{'threshold':0,'arbitration':0,'cleanup':0,'special_tokens':2},'error':None}
 print(json.dumps(out),flush=True)
"#;
#[test]
fn actual_ipc_returns_multiclass_batches_and_reuses_one_process() {
    let body = REPLY
        .replace("for line in sys.stdin:", "count=0\nfor line in sys.stdin:")
        .replace(
            " r=json.loads(line)",
            " count+=1\n r=json.loads(line)\n assert r['id']==count",
        );
    let (_temp, detector) = peer(&body);
    for _ in 0..2 {
        let batch = detector.try_detect("a b").unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].class, PiiClass::Name);
        assert_eq!(batch[1].class, PiiClass::Organization);
    }
}
#[test]
fn actual_ipc_rejects_whole_batch_and_sanitizes_foreign_errors() {
    for (old, new, expected) in [
        ("'GIVEN_NAME'", "'UNKNOWN'", "unknown_label"),
        ("'score':0.8", "'score':float('nan')", "protocol"),
        (
            "'completed_windows':1",
            "'completed_windows':0",
            "incomplete_window",
        ),
        ("'id':r['id']", "'id':r['id']+1", "protocol"),
        ("'start':2", "'start':0", "invalid_span"),
        ("'end':1", "'end':99", "invalid_span"),
    ] {
        let (_temp, detector) = peer(&REPLY.replace(old, new));
        let error = detector.try_detect("a b").unwrap_err();
        assert_eq!(error.message, expected);
    }
    let (_temp, detector) = peer("print('foreign diagnostic',flush=True)");
    assert!(matches!(
        detector.try_detect("a b").unwrap_err().message.as_str(),
        "protocol" | "backend"
    ));
}
#[test]
fn actual_ipc_error_preserves_transaction_and_typed_backend_failure() {
    let (_temp, detector) = peer(&REPLY.replace("'completed_windows':1", "'completed_windows':0"));
    let pipeline = gaze::Pipeline::builder()
        .detector(detector)
        .rule(gaze::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let error = pipeline
        .redact(&session, gaze::RawDocument::Text("a b".into()))
        .unwrap_err();
    assert!(matches!(
        error,
        gaze::Error::RecognizerDetect(gaze_types::DetectError::Backend { .. })
    ));
    assert!(session.tokens().is_empty());
}
#[test]
fn actual_ipc_deadline_kills_and_reaps_worker() {
    let (_temp, detector) = peer("time.sleep(120)");
    let start = std::time::Instant::now();
    assert_eq!(detector.try_detect("a b").unwrap_err().message, "timeout");
    assert!(start.elapsed() < std::time::Duration::from_secs(35));
}

#[test]
#[ignore = "requires allocated local CoreML inference slot and private bridge"]
fn real_primary_literal_collision_and_session_isolation() {
    let pipeline = gaze::Pipeline::builder()
        .detector(live())
        .rule(gaze::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut prediction = session.begin_transaction();
    let predicted = prediction
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    drop(prediction);
    let mut tx = session.begin_transaction();
    let result = pipeline.protect_text_transaction(
        &mut tx,
        &format!("{predicted} alice@example.invalid"),
        ProtectionContext::strict(&[LocaleTag::EnUs], &DictionaryBundle::default()),
    );
    assert!(
        matches!(result, Err(gaze::ProtectionError::Provenance)),
        "literal collision was accepted"
    );
    drop(tx);
    assert!(session.tokens().is_empty());
    let mut tx = session.begin_transaction();
    let clean = pipeline
        .protect_text_transaction(
            &mut tx,
            "alice@example.invalid",
            ProtectionContext::strict(&[LocaleTag::EnUs], &DictionaryBundle::default()),
        )
        .unwrap();
    assert!(tx.restore_strict_text(&clean).unwrap() == "alice@example.invalid");
    tx.commit().unwrap();
    let foreign = Session::new(Scope::Ephemeral).unwrap();
    assert!(
        foreign.restore_strict_text(&clean).is_err(),
        "foreign session restored token"
    );
}

#[test]
fn legacy_multiclass_arbitration_preserves_structured_floor_coverage() {
    // This peer emits one partial Name inside the email; the full deterministic
    // email must still be protected despite legacy Detector's routing score 1.0.
    let body = REPLY.replace("{'start':0,'end':1,'label':'GIVEN_NAME','score':0.8},{'start':2,'end':3,'label':'ORG','score':0.9}",
        "{'start':0,'end':5,'label':'GIVEN_NAME','score':0.8}");
    let (_temp, detector) = peer(&body);
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let context = Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
    };
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        gaze_recognizers::embedded("core").unwrap(),
    ))
    .unwrap();
    let locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);
    let pipeline = gaze_assembly::build_pipeline_with_detector(
        &policy,
        &context,
        &[rulepack],
        &locales,
        None,
        detector,
    )
    .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let clean = pipeline
        .protect_text_transaction(
            &mut tx,
            "alice@example.invalid",
            ProtectionContext::strict(&[LocaleTag::EnUs], &DictionaryBundle::default()),
        )
        .unwrap();
    assert!(
        !clean.contains("example.invalid"),
        "structured floor lost its suffix"
    );
    assert!(tx.restore_strict_text(&clean).unwrap() == "alice@example.invalid");
}
