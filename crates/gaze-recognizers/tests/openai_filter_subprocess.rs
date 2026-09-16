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
use serde_json::{json, Value};
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
printf '%s\n' '{"text":"Dr. Schmidt uses alice@example.invalid","detected_spans":[{"label":"private_person","start":0,"end":11,"score":0.97},{"label":"private_email","start":17,"end":38,"score":0.96}]}'
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
    let opf = emulated_opf(
        "opf-char-offsets",
        json!({"spans": [["private_person", 9, 22], ["private_email", 32, 51], ["private_person", 64, 67]]}),
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
    let opf = emulated_opf(
        "opf-char-offsets-oob",
        json!({"spans": [["private_person", 64, 70]]}),
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
fn crlf_before_a_span_at_the_very_end_maps_back_to_clean_text_bytes() {
    // OPF reads the text as a file in Python text mode, so each CRLF is one `\n` character in
    // the offsets it returns: 21..27 and 34..37 here. The NBSP still shifts the byte offsets; the
    // last span ends exactly at the text end.
    let clean = "Hallo\u{a0}Team,\r\nbitte an Jürgen\r\nGrüße Zoë";
    let opf = emulated_opf(
        "opf-char-offsets-crlf",
        json!({"spans": [["private_person", 21, 27], ["private_person", 34, 37]]}),
    )
    .unwrap();

    let spans = backend(opf).infer(clean).unwrap();

    let texts = spans
        .iter()
        .map(|span| &clean[span.start..span.end])
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Jürgen", "Zoë"]);
    assert_eq!(spans[1].end, clean.len());
}

#[test]
#[file_serial(gaze_subprocess)]
fn ascii_character_offsets_are_unchanged_bytes() {
    let clean = "Dr. Schmidt uses alice@example.invalid";
    let opf = emulated_opf(
        "opf-char-offsets-ascii",
        json!({"spans": [["private_person", 0, 11], ["private_email", 17, 38]]}),
    )
    .unwrap();

    let spans = backend(opf).infer(clean).unwrap();

    assert_eq!(
        spans
            .iter()
            .map(|span| (span.start, span.end))
            .collect::<Vec<_>>(),
        [(0, 11), (17, 38)]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn empty_clean_text_returns_no_spans_without_spawning() {
    // `opf` skips an empty input file and prints nothing, which would fail as invalid JSON.
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("spawned");
    let opf = script(
        "opf-empty",
        &format!(
            "#!/bin/sh\ntouch '{}'\ncat >/dev/null\nprintf '%s\\n' '{{\"text\":\"\",\"detected_spans\":[{{\"label\":\"private_person\",\"start\":0,\"end\":1}}]}}'\n",
            marker.display()
        ),
    )
    .unwrap();

    assert!(backend(opf).infer("").unwrap().is_empty());
    assert!(!marker.exists());
}

#[test]
#[file_serial(gaze_subprocess)]
fn descending_or_zero_width_character_spans_fail_closed() {
    for (name, start, end) in [
        ("opf-char-offsets-descending", 14, 9),
        ("opf-char-offsets-zero-width", 9, 9),
    ] {
        let opf = emulated_opf(name, json!({"spans": [["private_person", start, end]]})).unwrap();

        let error = backend(opf).infer(MULTIBYTE_CLEAN).unwrap_err();

        assert!(
            matches!(
                error,
                SafetyNetError::InvalidOutput { ref message } if message == "opf returned out-of-bounds span"
            ),
            "{name}: {error:?}"
        );
    }
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

/// A fake `opf` that follows the pinned CLI's input selection and output framing (see
/// `fixtures/opf_cli_emulator.py`), so a fixture fails exactly where the real CLI would.
fn emulated_opf(name: &str, config: Value) -> io::Result<PathBuf> {
    emulated_opf_with_prelude(name, config, "")
}

fn emulated_opf_with_prelude(name: &str, config: Value, prelude: &str) -> io::Result<PathBuf> {
    let command = script(
        name,
        &format!("#!/bin/sh\n{prelude}\nexec python3 \"$(dirname \"$0\")/emulator.py\" \"$@\"\n"),
    )?;
    let dir = command.parent().expect("script has a directory");
    fs::write(
        dir.join("emulator.py"),
        include_str!("fixtures/opf_cli_emulator.py"),
    )?;
    fs::write(dir.join("emulator.json"), config.to_string())?;
    Ok(command)
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
        let command = emulated_opf_with_prelude(
            "opf-diagnostics",
            json!({"spans": [["private_person", 0, 11]]}),
            &format!("head -c {bytes} /dev/zero | tr '\\000' w >&2"),
        )
        .unwrap();
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

// Whole-text input contract (todo 3677). Piped stdin makes the pinned `opf` analyse every
// non-blank line as its own input. These fixtures pass `--no-print-color-coded-text` in the
// configured args, as an adopter or the bench would, so the colour section cannot mask the
// silent case: without the fix, leading blank lines come back as ONE valid output whose offsets
// are relative to the first non-blank line.

const PERSON_AND_EMAIL: &[(&str, &str)] = &[
    ("private_person", "John Smith"),
    ("private_person", "Zoë Müller"),
    ("private_person", "Jürgen Müller"),
    ("private_email", "jane.doe@example.invalid"),
];

fn colour_off_backend(command: PathBuf) -> SubprocessOpenAiFilterBackend {
    SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(command)
            .with_args([
                "--format",
                "json",
                "--output-mode",
                "typed",
                "--no-print-color-coded-text",
            ])
            .with_timeout(test_subprocess_timeout()),
    )
    .unwrap()
}

fn emulated_needles(name: &str) -> PathBuf {
    let needles = PERSON_AND_EMAIL
        .iter()
        .map(|(label, needle)| json!([label, needle]))
        .collect::<Vec<_>>();
    emulated_opf(name, json!({ "needles": needles })).unwrap()
}

fn span_texts(clean: &str, name: &str) -> Vec<String> {
    colour_off_backend(emulated_needles(name))
        .infer(clean)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"))
        .iter()
        .map(|span| clean[span.start..span.end].to_string())
        .collect()
}

#[test]
#[file_serial(gaze_subprocess)]
fn two_non_blank_lines_are_analysed_as_one_input() {
    let clean = "Contact John Smith today.\nEmail jane.doe@example.invalid please.";
    assert_eq!(
        span_texts(clean, "opf-whole-two-lines"),
        ["John Smith", "jane.doe@example.invalid"]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn leading_blank_lines_do_not_shift_offsets() {
    let clean = "\n\nJohn Smith called.";
    assert_eq!(span_texts(clean, "opf-whole-leading-blank"), ["John Smith"]);
}

#[test]
#[file_serial(gaze_subprocess)]
fn whitespace_only_first_line_does_not_shift_offsets() {
    let clean = "   \nGrüße an Jürgen Müller, jane.doe@example.invalid";
    assert_eq!(
        span_texts(clean, "opf-whole-whitespace-line"),
        ["Jürgen Müller", "jane.doe@example.invalid"]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn crlf_and_lone_cr_map_back_to_clean_text_bytes() {
    let clean = "Hi team,\r\nplease call John Smith.\rThanks, Zoë Müller\r\n";
    assert_eq!(
        span_texts(clean, "opf-whole-crlf"),
        ["John Smith", "Zoë Müller"]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn trailing_newline_keeps_the_span_at_the_end() {
    let clean = "Signed John Smith\n";
    assert_eq!(
        span_texts(clean, "opf-whole-trailing-newline"),
        ["John Smith"]
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn default_args_parse_the_real_cli_output_framing() {
    // No configured colour flag: the adapter must suppress the ANSI section itself.
    let clean = "Contact John Smith today.";
    let spans = backend(emulated_needles("opf-whole-default-args"))
        .infer(clean)
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(&clean[spans[0].start..spans[0].end], "John Smith");
}

#[test]
#[file_serial(gaze_subprocess)]
fn more_than_one_json_document_fails_closed() {
    let clean = "John Smith";
    let document =
        r#"{"text":"John Smith","detected_spans":[{"label":"private_person","start":0,"end":10}]}"#;
    let opf = script(
        "opf-whole-two-documents",
        &format!("#!/bin/sh\ncat >/dev/null\nprintf '%s\\n%s\\n' '{document}' '{document}'\n"),
    )
    .unwrap();

    assert!(matches!(
        backend(opf).infer(clean).unwrap_err(),
        SafetyNetError::InvalidOutput { ref message } if message == "opf stdout was not valid JSON"
    ));
}

#[test]
#[file_serial(gaze_subprocess)]
fn echoed_text_other_than_the_sent_text_fails_closed() {
    // Exactly what piped stdin returns for "\n\nJohn Smith": one output for the third line.
    let opf = script(
        "opf-whole-echo-mismatch",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"text":"John Smith","detected_spans":[{"label":"private_person","start":0,"end":10}]}'
"#,
    )
    .unwrap();

    let error = backend(opf).infer("\n\nJohn Smith").unwrap_err();

    assert!(matches!(
        error,
        SafetyNetError::InvalidOutput { ref message }
            if message == "opf analysed a different text than the one sent"
    ));
    assert!(!error.to_string().contains("John Smith"));
}

#[test]
#[file_serial(gaze_subprocess)]
fn output_without_the_echoed_text_fails_closed() {
    let opf = script(
        "opf-whole-bare-array",
        r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '[{"label":"private_person","start":0,"end":10}]'
"#,
    )
    .unwrap();

    assert!(matches!(
        backend(opf).infer("John Smith").unwrap_err(),
        SafetyNetError::InvalidOutput { ref message } if message == "opf stdout was not valid JSON"
    ));
}

#[test]
#[file_serial(gaze_subprocess)]
fn whole_text_input_arguments_follow_the_configured_args() {
    let dir = tempfile::tempdir().unwrap();
    let arg_log = dir.path().join("argv");
    let opf = script(
        "opf-whole-argv",
        &format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
cat >/dev/null
printf '%s\n' '{{"text":"clean","detected_spans":[]}}'
"#,
            arg_log.display()
        ),
    )
    .unwrap();

    backend(opf).infer("clean").unwrap();

    assert_eq!(
        fs::read_to_string(arg_log).unwrap(),
        "--format\njson\n--output-mode\ntyped\n--no-print-color-coded-text\n--text-file\n/dev/stdin\n"
    );
}

#[test]
#[file_serial(gaze_subprocess)]
fn a_text_file_in_the_configured_args_fails_closed() {
    // `--text-file` appends in the pinned CLI: an adopter-configured file is analysed next to the
    // piped text, so two results come back and neither may be applied.
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other.txt");
    fs::write(&other, "Contact John Smith today.").unwrap();
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(emulated_needles("opf-whole-configured-text-file"))
            .with_args([
                "--format",
                "json",
                "--output-mode",
                "typed",
                "--text-file",
                other.to_str().unwrap(),
            ])
            .with_timeout(test_subprocess_timeout()),
    )
    .unwrap();

    assert!(matches!(
        backend.infer("Email jane.doe@example.invalid please.").unwrap_err(),
        SafetyNetError::InvalidOutput { ref message } if message == "opf stdout was not valid JSON"
    ));
}

/// Runs the real pinned CLI. `GAZE_TEST_REAL_OPF=<opf path>` and a verified checkpoint at
/// `GAZE_TEST_REAL_OPF_CHECKPOINT` are required; run with `--ignored`.
#[test]
#[ignore = "needs the real opf runtime: GAZE_TEST_REAL_OPF and GAZE_TEST_REAL_OPF_CHECKPOINT"]
#[file_serial(gaze_subprocess)]
fn real_opf_analyses_multi_line_text_as_one_input() {
    let command = std::env::var_os("GAZE_TEST_REAL_OPF").expect("GAZE_TEST_REAL_OPF");
    let checkpoint =
        std::env::var_os("GAZE_TEST_REAL_OPF_CHECKPOINT").expect("GAZE_TEST_REAL_OPF_CHECKPOINT");
    let backend = SubprocessOpenAiFilterBackend::new(
        SubprocessOpenAiFilterConfig::new(PathBuf::from(command))
            .with_args([
                "--format",
                "json",
                "--output-mode",
                "typed",
                "--device",
                "cpu",
            ])
            .with_checkpoint_path(PathBuf::from(checkpoint))
            .with_checkpoint_bundle_sha256_verification(true)
            .with_timeout(Duration::from_secs(120)),
    )
    .unwrap();

    for (clean, expected) in [
        (
            "Contact John Smith today.\nEmail jane.doe@example.com please.\n",
            "John Smith",
        ),
        ("\n\nPlease call John Smith tomorrow.", "John Smith"),
        (
            "Hi team,\r\nplease call John Smith.\rThanks, Zoë Müller\r\n",
            "Zoë Müller",
        ),
        (
            "   \nGrüße an Jürgen Müller, jane.doe@example.com",
            "Jürgen Müller",
        ),
    ] {
        let spans = backend
            .infer(clean)
            .unwrap_or_else(|error| panic!("{clean:?}: {error:?}"));
        let texts = spans
            .iter()
            .map(|span| &clean[span.start..span.end])
            .collect::<Vec<_>>();
        eprintln!("{clean:?} -> {texts:?}");
        assert!(texts.contains(&expected), "{clean:?}: {texts:?}");
    }
}
