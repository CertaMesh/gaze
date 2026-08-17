//! Kiji LocaleAwareModel contract tests.

#![cfg(all(unix, feature = "safety-net-kiji"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use gaze_recognizers::safety_net::kiji_distilbert::{
    KijiDistilbertSafetyNet, SubprocessKijiConfig, REQUIRED_KIJI_ARTIFACTS,
};
use gaze_recognizers::{LocaleAwareModel, ModelHints, ModelInput, ModelStage};
use gaze_types::{LocaleTag, PiiClass};
use sha2::{Digest, Sha256};
use tempfile::{tempdir, TempDir};

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

fn write_mock_kiji(body: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mock-kiji");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{}'
"#,
            body
        ),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    (dir, path)
}

fn populate_model_dir() -> (TempDir, String) {
    let dir = tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for required in REQUIRED_KIJI_ARTIFACTS {
        if *required == "SHA256SUMS" {
            continue;
        }
        let path = dir.path().join(required);
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let artifact_sha = hex_sha256(b"fixture");
    let sha256sums = format!(
        "{artifact_sha}  labels.json\n{artifact_sha}  model.onnx\n{artifact_sha}  tokenizer.json\n"
    );
    let path = dir.path().join("SHA256SUMS");
    fs::write(&path, sha256sums.as_bytes()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let expected_sha = hex_sha256(sha256sums.as_bytes());
    (dir, expected_sha)
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn kiji_locale_aware_model_reports_configured_native_locales() {
    let net = KijiDistilbertSafetyNet::new(SubprocessKijiConfig::new("kiji"))
        .with_locales(vec![LocaleTag::DeDe]);
    let model: &dyn LocaleAwareModel = &net;

    assert_eq!(model.name(), "kiji-distilbert");
    assert_eq!(model.native_locales(), &[LocaleTag::DeDe]);
}

#[test]
fn kiji_infer_round_trips_spans_through_locale_aware_trait() {
    let (_kiji_dir, kiji) =
        write_mock_kiji(r#"[{"label":"person","start":0,"end":11,"score":0.97}]"#);
    let (model_dir, expected_sha) = populate_model_dir();
    let net = KijiDistilbertSafetyNet::new(
        SubprocessKijiConfig::new(&kiji)
            .with_model_dir(model_dir.path())
            .with_timeout(test_subprocess_timeout())
            .with_expected_bundle_sha256_for_tests(expected_sha),
    );
    let model: &dyn LocaleAwareModel = &net;

    let spans = model
        .infer(
            ModelInput {
                text: "Dr. Schmidt greets you".to_string(),
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
    assert_eq!(spans[0].model_name, "kiji-distilbert");
}
