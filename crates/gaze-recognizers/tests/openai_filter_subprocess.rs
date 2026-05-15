#![cfg(unix)]

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gaze_recognizers::safety_net::openai_filter::{
    map_openai_label, openai_label_to_safety_net_class, OpenAiFilterBackend, OpenAiFilterSafetyNet,
    SubprocessOpenAiFilterBackend, SubprocessOpenAiFilterConfig,
};
use gaze_recognizers::{LocaleAwareModel, ModelHints, ModelInput, ModelStage};
use gaze_types::{
    DocumentKind, LeakKind, LocaleTag, Manifest, PiiClass, SafetyNet, SafetyNetContext,
    SafetyNetError,
};
use serial_test::serial;

#[test]
#[serial]
fn official_json_private_fields_are_dropped_at_boundary() {
    let clean = "Dr. Schmidt uses alice@example.invalid";
    let opf = script(
        "opf-private-fields",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"schema_version":1,"summary":{"output_mode":"typed","span_count":2,"by_label":{"private_person":1,"private_email":1},"decoded_mismatch":false},"text":"Dr. Schmidt uses alice@example.invalid","detected_spans":[{"label":"private_person","start":0,"end":11,"text":"Dr. Schmidt","placeholder":"<PRIVATE_PERSON>"},{"label":"private_email","start":17,"end":38,"text":"alice@example.invalid","placeholder":"<PRIVATE_EMAIL>"}],"redacted_text":" uses "}'
"#,
    )
    .unwrap();
    let backend = backend(opf);

    let spans = backend.infer(clean).unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].label, "private_person");
    assert_eq!(spans[1].label, "private_email");

    let debug = format!("{spans:?} {backend:?}");
    assert_private_payload_absent(&debug);

    let net = OpenAiFilterSafetyNet::new(SubprocessOpenAiFilterConfig::new(
        backend.config().command().to_path_buf(),
    ));
    let manifest = Manifest::default();
    let context = context(&manifest, Some("$.profile.email"));
    let suspects = net.check(clean, context).unwrap();
    assert_eq!(suspects.len(), 2);
    assert_eq!(suspects[0].field_path.as_deref(), Some("$.profile.email"));

    let suspect_debug = format!("{suspects:?}");
    assert_private_payload_absent(&suspect_debug);
}

#[test]
#[serial]
fn all_official_labels_map_exactly_to_gaze_classes() {
    let cases = [
        ("private_person", PiiClass::Name),
        ("private_address", PiiClass::Location),
        ("private_email", PiiClass::Email),
        ("private_phone", PiiClass::custom("phone")),
        ("private_url", PiiClass::custom("url")),
        ("private_date", PiiClass::custom("date")),
        ("account_number", PiiClass::custom("account_number")),
        ("secret", PiiClass::custom("secret")),
    ];

    for (raw, expected) in cases {
        let label = map_openai_label(raw).unwrap();
        assert_eq!(
            openai_label_to_safety_net_class(label).to_pii_class(),
            expected
        );
    }

    for invented in [
        "person",
        "email",
        "organization",
        "ssn",
        "credit_card",
        "ip_address",
    ] {
        assert!(map_openai_label(invented).is_err(), "{invented}");
    }
}

#[test]
#[serial]
fn openai_filter_is_object_safe_locale_aware_model_with_global_native_locale() {
    let net = OpenAiFilterSafetyNet::new(SubprocessOpenAiFilterConfig::new("opf"));
    let model: &dyn LocaleAwareModel = &net;

    assert_eq!(model.name(), "openai-privacy-filter");
    assert_eq!(model.native_locales(), &[LocaleTag::Global]);
}

#[test]
#[serial]
fn openai_filter_infer_round_trips_spans_through_locale_aware_trait() {
    let opf = script(
        "opf-locale-aware",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '[{"label":"private_person","start":0,"end":11,"score":0.97},{"label":"private_email","start":17,"end":38,"score":0.96}]'
"#,
    )
    .unwrap();
    let net = OpenAiFilterSafetyNet::new(
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(Duration::from_secs(120)),
    );
    let model: &dyn LocaleAwareModel = &net;

    let spans = model
        .infer(
            ModelInput {
                text: "Dr. Schmidt uses alice@example.invalid".to_string(),
                locale: LocaleTag::Global,
            },
            ModelHints {
                stage: ModelStage::Pass3SafetyNet,
                max_spans: Some(1),
            },
        )
        .unwrap();

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].text, "Dr. Schmidt");
    assert_eq!(spans[0].byte_range, 0..11);
    assert_eq!(spans[0].class, PiiClass::Name);
    assert_eq!(spans[0].confidence, Some(0.97));
    assert_eq!(spans[0].model_name, "openai-privacy-filter");
}

#[test]
#[serial]
fn unknown_valid_label_fails_closed() {
    let opf = script(
        "opf-unknown-label",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"schema_version":1,"detected_spans":[{"label":"private_bank","start":0,"end":4,"text":"ABCD","placeholder":""}],"text":"ABCD","redacted_text":""}'
"#,
    )
    .unwrap();
    let error = backend(opf).infer("ABCD").unwrap_err();

    assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    assert!(!error.to_string().contains("ABCD"));
}

#[test]
#[serial]
fn invalid_label_characters_fail_invalid_output() {
    let opf = script(
        "opf-invalid-label",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"schema_version":1,"detected_spans":[{"label":"private-email","start":0,"end":4,"text":"ABCD","placeholder":""}],"text":"ABCD","redacted_text":""}'
"#,
    )
    .unwrap();
    let error = backend(opf).infer("ABCD").unwrap_err();

    assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
}

#[test]
#[serial]
fn sleeping_child_times_out_and_returns_sanitized_runtime_error() {
    let opf = script(
        "opf-sleep",
        r#"#!/bin/sh
cat >/dev/null
sleep 5
printf '%s\n' '{"schema_version":1,"detected_spans":[],"text":"","redacted_text":""}'
"#,
    )
    .unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(Duration::from_millis(100)),
    )
    .unwrap();

    let error = backend.infer("clean text only").unwrap_err();
    assert!(matches!(error, SafetyNetError::Runtime { .. }));
    assert!(error.to_string().contains("timed out"));
}

#[test]
#[serial]
fn stdin_blocked_child_times_out_and_kills_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("opf.pid");
    let opf = script(
        "opf-stdin-block",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$$" > '{}'
IFS= read -r _ || true
exec sleep 30
"#,
            pidfile.display()
        ),
    )
    .unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(Duration::from_secs(15)),
    )
    .unwrap();
    let clean = format!("x\n{}", "x".repeat(128 * 1024));

    let started = Instant::now();
    let error = backend.infer(&clean).unwrap_err();
    let elapsed = started.elapsed();

    assert!(matches!(error, SafetyNetError::Runtime { .. }));
    assert!(error.to_string().contains("timed out"));
    assert!(
        elapsed < Duration::from_secs(17),
        "blocked stdin timeout took {elapsed:?}"
    );

    let child_pid = wait_for_pidfile(&pidfile, Duration::from_secs(10))
        .expect("child failed to create pidfile within budget - child startup broken");
    assert!(!process_is_present(&child_pid.to_string()));
}

#[test]
#[serial]
fn oversized_input_returns_before_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("spawned");
    let opf = script(
        "opf-marker",
        &format!(
            r#"#!/bin/sh
touch '{}'
cat >/dev/null
printf '%s\n' '{{"schema_version":1,"detected_spans":[],"text":"","redacted_text":""}}'
"#,
            marker.display()
        ),
    )
    .unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(opf).with_max_input_bytes(4),
    )
    .unwrap();

    let error = backend.infer("too large").unwrap_err();
    assert!(matches!(error, SafetyNetError::InputTooLarge { .. }));
    assert!(!marker.exists());
}

#[test]
#[serial]
fn verbose_stderr_is_stripped_and_capped() {
    let opf = script(
        "opf-stderr",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' 'failed for alice@example.invalid and +1-555-0101 with a very long diagnostic that should be capped before it can become an audit payload or leak channel' >&2
exit 7
"#,
    )
    .unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(opf)
            .with_timeout(Duration::from_secs(120))
            .with_stderr_diagnostics(true),
    )
    .unwrap();

    let error = backend.infer("clean").unwrap_err();
    let message = error.to_string();
    assert!(!message.contains("alice@example.invalid"));
    assert!(!message.contains("+1-555-0101"));
    assert!(message.contains("<redacted>"));
    assert!(message.len() < 360);
}

#[test]
#[serial]
fn missing_checkpoint_fails_closed_without_spawn_or_download() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("spawned");
    let opf = script(
        "opf-missing-checkpoint",
        &format!(
            r#"#!/bin/sh
touch '{}'
printf '%s\n' '{{"schema_version":1,"detected_spans":[],"text":"","redacted_text":""}}'
"#,
            marker.display()
        ),
    )
    .unwrap();
    let config = SubprocessOpenAiFilterConfig::new(opf)
        .with_checkpoint_path(dir.path().join("missing-checkpoint"));

    let error = SubprocessOpenAiFilterBackend::new(config).unwrap_err();
    assert!(matches!(error, SafetyNetError::WeightsMissing { .. }));
    assert!(!marker.exists());
}

#[test]
#[serial]
fn safety_net_correlates_raw_spans_with_manifest_without_source_text() {
    let clean = "<Email_1>";
    let opf = script(
        "opf-covered",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"schema_version":1,"detected_spans":[{"label":"private_email","start":0,"end":9,"text":"alice@example.invalid","placeholder":"<PRIVATE_EMAIL>"}],"text":"<Email_1>","redacted_text":""}'
"#,
    )
    .unwrap();
    let net = OpenAiFilterSafetyNet::new(
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(Duration::from_secs(120)),
    );
    let manifest = Manifest::from_spans(vec![gaze_types::EmittedTokenSpan::new(
        0..9,
        0..21,
        PiiClass::Email,
    )]);

    let suspects = net.check(clean, context(&manifest, None)).unwrap();
    assert!(suspects.is_empty());

    let empty_manifest = Manifest::default();
    let suspects = net.check(clean, context(&empty_manifest, None)).unwrap();
    assert_eq!(suspects.len(), 1);
    assert_eq!(suspects[0].kind, LeakKind::Uncovered);
    assert_eq!(suspects[0].raw_label, "private_email");
}

fn backend(command: PathBuf) -> SubprocessOpenAiFilterBackend {
    SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(command).with_timeout(Duration::from_secs(120)),
    )
    .unwrap()
}

fn context<'a>(manifest: &'a Manifest, field_path: Option<&'a str>) -> SafetyNetContext<'a> {
    SafetyNetContext::new(
        manifest,
        &[LocaleTag::Global],
        DocumentKind::Text,
        None,
        field_path,
    )
}

fn script(name: &str, body: &str) -> io::Result<PathBuf> {
    let dir = tempfile::Builder::new().prefix(name).tempdir()?.keep();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    let path = dir.join("opf");
    fs::write(&path, body)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

fn wait_for_pidfile(path: &Path, deadline: Duration) -> io::Result<u32> {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                return Ok(pid);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("pidfile {path:?} not created within {deadline:?}"),
    ))
}

fn process_is_present(pid: &str) -> bool {
    Command::new("ps")
        .args(["-p", pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn assert_private_payload_absent(value: &str) {
    for private in [
        "alice@example.invalid",
        "Dr. Schmidt",
        "<PRIVATE_EMAIL>",
        "<PRIVATE_PERSON>",
        "placeholder",
    ] {
        assert!(
            !value.contains(private),
            "private payload crossed boundary: {private} in {value}"
        );
    }
}
