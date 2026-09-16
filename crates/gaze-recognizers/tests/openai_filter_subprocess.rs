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
use serial_test::file_serial;

fn test_subprocess_timeout() -> Duration {
    let seconds = std::env::var("GAZE_TEST_SUBPROCESS_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .expect("test subprocess timeout must be an integer")
        })
        .unwrap_or(60);
    assert!(seconds > 0, "test subprocess timeout must be positive");
    Duration::from_secs(seconds)
}

#[test]
#[file_serial(gaze_subprocess)]
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

    let net = OpenAiFilterSafetyNet::new(
        SubprocessOpenAiFilterConfig::new(backend.config().command().to_path_buf())
            .with_timeout(test_subprocess_timeout()),
    );
    let manifest = Manifest::default();
    let context = context(&manifest, Some("$.profile.email"));
    let suspects = net.check(clean, context).unwrap();
    assert_eq!(suspects.len(), 2);
    assert_eq!(suspects[0].field_path.as_deref(), Some("$.profile.email"));

    let suspect_debug = format!("{suspects:?}");
    assert_private_payload_absent(&suspect_debug);
}

#[test]
#[file_serial(gaze_subprocess)]
fn all_official_labels_map_exactly_to_gaze_classes() {
    let cases = [
        ("private_person", PiiClass::Name),
        ("private_address", PiiClass::Location),
        ("private_email", PiiClass::Email),
        (
            "private_phone",
            PiiClass::custom("phone").expect("valid custom class"),
        ),
        (
            "private_url",
            PiiClass::custom("url").expect("valid custom class"),
        ),
        (
            "private_date",
            PiiClass::custom("date").expect("valid custom class"),
        ),
        (
            "account_number",
            PiiClass::custom("account_number").expect("valid custom class"),
        ),
        (
            "secret",
            PiiClass::custom("secret").expect("valid custom class"),
        ),
    ];

    for (raw, expected) in cases {
        let label = map_openai_label(raw).unwrap();
        assert_eq!(
            openai_label_to_safety_net_class(label)
                .expect("official label maps to safety-net class")
                .to_pii_class(),
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
#[file_serial(gaze_subprocess)]
fn openai_filter_locale_aware_model_reports_configured_native_locales() {
    let net = OpenAiFilterSafetyNet::new(SubprocessOpenAiFilterConfig::new("opf"))
        .with_locales(vec![LocaleTag::EnUs]);
    let model: &dyn LocaleAwareModel = &net;

    assert_eq!(model.name(), "openai-privacy-filter");
    assert_eq!(model.native_locales(), &[LocaleTag::EnUs]);
}

#[test]
#[file_serial(gaze_subprocess)]
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
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(test_subprocess_timeout()),
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

// OPF reports offsets as Python `str` indices (Unicode scalar values). The fixture puts
// multibyte text before, inside, and after the spans: umlauts and ß, an NFD combining acute
// accent, an emoji, and an NBSP. Read as bytes, every one of these spans lands on the wrong text.
const MULTIBYTE_CLEAN: &str =
    "Grüße an Jürgen Müller: cafe\u{301} \u{1F600}\u{a0}bob@example.invalid, gezeichnet Zoë";

#[test]
#[file_serial(gaze_subprocess)]
fn character_offsets_are_converted_to_clean_text_bytes() {
    let opf = script(
        "opf-char-offsets",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"schema_version":1,"detected_spans":[{"label":"private_person","start":9,"end":22},{"label":"private_email","start":32,"end":51},{"label":"private_person","start":64,"end":67}],"redacted_text":""}'
"#,
    )
    .unwrap();

    let spans = backend(opf).infer(MULTIBYTE_CLEAN).unwrap();

    let texts = spans
        .iter()
        .map(|span| &MULTIBYTE_CLEAN[span.start..span.end])
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Jürgen Müller", "bob@example.invalid", "Zoë"]);
    assert_eq!(
        spans
            .iter()
            .map(|span| (span.start, span.end))
            .collect::<Vec<_>>(),
        [(11, 26), (41, 60), (73, 77)]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn character_offset_past_the_last_character_fails_closed() {
    // 70 is inside the 77 UTF-8 bytes but past the 67 characters: only a byte reading accepts it.
    let opf = script(
        "opf-char-offsets-oob",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '[{"label":"private_person","start":64,"end":70}]'
"#,
    )
    .unwrap();

    let error = backend(opf).infer(MULTIBYTE_CLEAN).unwrap_err();

    assert!(matches!(
        error,
        SafetyNetError::InvalidOutput { ref message } if message == "opf returned out-of-bounds span"
    ));
}

#[test]
#[file_serial(gaze_subprocess)]
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
#[file_serial(gaze_subprocess)]
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
#[file_serial(gaze_subprocess)]
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
#[file_serial(gaze_subprocess)]
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
    // The failure mode this bounds is "the write blocked instead of timing
    // out", which returns only when the child's own `sleep 30` exits - so any
    // bound comfortably under 30s still discriminates it. The old 17s left the
    // configured 15s timeout just 2s of slack, which is the same
    // fixed-budget-loses-its-race shape as solo #2981; sit halfway between the
    // two instead so a loaded runner cannot turn this into a false red.
    assert!(
        elapsed < Duration::from_secs(25),
        "blocked stdin timeout took {elapsed:?}"
    );

    let child_pid = wait_for_pidfile(&pidfile, Duration::from_secs(10))
        .expect("child failed to create pidfile within budget - child startup broken");
    assert!(!process_is_present(&child_pid.to_string()));
}

#[test]
#[file_serial(gaze_subprocess)]
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
#[file_serial(gaze_subprocess)]
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
            .with_timeout(test_subprocess_timeout())
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
#[file_serial(gaze_subprocess)]
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
#[file_serial(gaze_subprocess)]
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
        SubprocessOpenAiFilterConfig::new(opf).with_timeout(test_subprocess_timeout()),
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
        SubprocessOpenAiFilterConfig::new(command).with_timeout(test_subprocess_timeout()),
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

#[test]
#[file_serial(gaze_subprocess)]
fn verbose_stderr_preserves_successful_spans() {
    for bytes in [257, 300, 2 * 1024 * 1024] {
        let body = format!(
            r#"#!/bin/sh
cat >/dev/null
head -c {bytes} /dev/zero | tr '\000' w >&2
printf '%s\n' '[{{"label":"private_person","start":0,"end":11,"score":0.97}}]'
"#
        );
        let command = script("opf-diagnostics", &body).unwrap();
        let config = SubprocessOpenAiFilterConfig::new(command);
        let quiet = SubprocessOpenAiFilterBackend::new(
            config.clone().with_timeout(test_subprocess_timeout()),
        )
        .unwrap()
        .infer("Dr. Schmidt greets you")
        .unwrap();
        let verbose = SubprocessOpenAiFilterBackend::new(
            config
                .with_timeout(test_subprocess_timeout())
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer("Dr. Schmidt greets you")
        .unwrap();
        assert_eq!(verbose, quiet, "stderr bytes: {bytes}");
        assert_eq!(verbose.len(), 1);
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn overflowing_stderr_failure_is_sanitized_and_marked() {
    let body = r#"#!/bin/sh
cat >/dev/null
printf 'failed alice@example.invalid +1-555-0101 \033\377é ' >&2
head -c 2097152 /dev/zero | tr '\000' w >&2
exit 7
"#
    .to_string();
    let command = script("opf-diagnostics", &body).unwrap();
    let config = SubprocessOpenAiFilterConfig::new(command);
    let error = SubprocessOpenAiFilterBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true),
    )
    .unwrap()
    .infer("clean")
    .unwrap_err();
    let SafetyNetError::Runtime { message } = error else {
        panic!("expected runtime error");
    };
    assert!(message.contains("exited with status"));
    let diagnostic = message.split_once(": ").unwrap().1;
    assert!(diagnostic.len() <= 256);
    assert!(diagnostic.ends_with("[truncated]"));
    assert!(diagnostic.contains("<redacted>"));
    assert!(!diagnostic.contains("alice"));
    assert!(!diagnostic.contains("555"));
    assert!(diagnostic.is_ascii());
    assert!(!diagnostic.chars().any(char::is_control));
}

#[test]
#[file_serial(gaze_subprocess)]
fn stdout_overflow_still_fails_closed_with_verbose_stderr() {
    let body = r#"#!/bin/sh
cat >/dev/null
head -c 2097152 /dev/zero | tr '\000' w >&2
printf '%s\n' '[{"label":"private_person","start":0,"end":11,"score":0.97}]'
"#
    .to_string();
    let command = script("opf-diagnostics", &body).unwrap();
    let config = SubprocessOpenAiFilterConfig::new(command);
    let error = SubprocessOpenAiFilterBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true)
            .with_max_stdout_bytes(8),
    )
    .unwrap()
    .infer("Dr. Schmidt")
    .unwrap_err();
    let SafetyNetError::Runtime { message } = error else {
        panic!("expected runtime error");
    };
    assert!(message.contains("stdout capture failed"));
    assert!(message.contains("stream exceeded configured byte cap"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn overflowing_pipes_still_kill_and_reap_child() {
    for stdout_overflow in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        // Keep the shell itself writing: no descendant can retain a pipe after kill.
        let output = if stdout_overflow {
            "printf '%8192s' w; printf '%8192s' w >&2"
        } else {
            "printf '%8192s' w >&2"
        };
        let body = format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' "$$" > '{}'
while :; do {output}; done
"#,
            pidfile.display()
        );
        let command = script("opf-lifecycle", &body).unwrap();
        let config = SubprocessOpenAiFilterConfig::new(command);
        let started = std::time::Instant::now();
        let error = SubprocessOpenAiFilterBackend::new(
            config
                .with_timeout(Duration::from_secs(5))
                .with_max_stdout_bytes(128 * 1024)
                .with_stderr_diagnostics(true),
        )
        .unwrap()
        .infer("clean")
        .unwrap_err();
        let SafetyNetError::Runtime { message } = error else {
            panic!("expected runtime error");
        };
        if stdout_overflow {
            assert!(message.contains("stdout capture failed"), "{message}");
        } else {
            assert!(message.contains("timed out"), "{message}");
        }
        assert!(started.elapsed() < Duration::from_secs(20));
        let pid = fs::read_to_string(pidfile).unwrap();
        assert!(
            !std::process::Command::new("ps")
                .args(["-p", pid.trim()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success(),
            "child must be killed and reaped"
        );
    }
}

#[test]
#[file_serial(gaze_subprocess)]
fn invalid_stdout_remains_invalid_output_with_verbose_stderr() {
    let body = r#"#!/bin/sh
cat >/dev/null
head -c 2097152 /dev/zero | tr '\000' w >&2
printf '%s\n' 'alice@example.invalid'
"#
    .to_string();
    let command = script("opf-lifecycle", &body).unwrap();
    let config = SubprocessOpenAiFilterConfig::new(command);
    let error = SubprocessOpenAiFilterBackend::new(
        config
            .with_timeout(test_subprocess_timeout())
            .with_stderr_diagnostics(true),
    )
    .unwrap()
    .infer("clean")
    .unwrap_err();
    assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    assert!(!error.to_string().contains("alice@example.invalid"));
}
