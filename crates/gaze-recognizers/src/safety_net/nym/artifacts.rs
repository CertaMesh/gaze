//! Pinned Nym-small int8 bundle.

use std::path::Path;

use gaze_types::nym::NymLabel;
use gaze_types::SafetyNetError;

use crate::bundle::{verify_bundle, BundleSpec};

/// Hugging Face repository of the Nym-small model.
pub const NYM_SMALL_HF_REPO: &str = "Wismut/nym-pii-multilingual-small";

/// Immutable upstream revision the bundle is fetched from.
pub const NYM_SMALL_HF_COMMIT: &str = "4348999cd3c2e20c49615e9af7c6bbb45b64cd85";

/// Checksum file name inside the bundle.
pub const NYM_SMALL_CHECKSUM_FILE: &str = "SHA256SUMS";

/// Model file name inside the bundle.
pub const NYM_SMALL_MODEL_FILE: &str = "model_int8.onnx";

/// Tokenizer file name inside the bundle.
pub const NYM_SMALL_TOKENIZER_FILE: &str = "tokenizer.json";

/// Model config (carries `id2label`) inside the bundle.
pub const NYM_SMALL_CONFIG_FILE: &str = "config.json";

/// Every file an installed bundle contains.
pub const REQUIRED_NYM_SMALL_ARTIFACTS: &[&str] = &[
    NYM_SMALL_CHECKSUM_FILE,
    NYM_SMALL_CONFIG_FILE,
    NYM_SMALL_MODEL_FILE,
    NYM_SMALL_TOKENIZER_FILE,
];

/// Upstream path of each bundle file at [`NYM_SMALL_HF_COMMIT`], as `(upstream, bundle)` pairs.
pub const NYM_SMALL_UPSTREAM_FILES: &[(&str, &str)] = &[
    ("int8/config.json", NYM_SMALL_CONFIG_FILE),
    ("int8/model_int8.onnx", NYM_SMALL_MODEL_FILE),
    ("int8/tokenizer.json", NYM_SMALL_TOKENIZER_FILE),
];

/// Canonical checksum file content. Digests are of the files at [`NYM_SMALL_HF_COMMIT`].
pub const NYM_SMALL_INT8_SHA256SUMS: &str = concat!(
    "3f07065571e22bb73eba28ddb1fae4509c0cf703762e3bfa7a6ab2111cf7cb88  config.json\n",
    "139006aea2cbd8e709d322f056232570de54661f624143be4893aaa387190286  model_int8.onnx\n",
    "c299144e68dfec1dc536204a7ae3712710c5c6cade9269a83f5042250d47d8de  tokenizer.json\n",
);

/// SHA-256 of [`NYM_SMALL_INT8_SHA256SUMS`]. The runtime verifies this digest first, then every
/// artifact the checksum file lists; any mismatch refuses to load.
pub const NYM_SMALL_INT8_BUNDLE_SHA256: &str =
    "71f9023bcf86ead7234434f11a4881c0b0a87622ba4e2e44b74f55d3ede7c767";

pub(crate) const NYM_BUNDLE_SPEC: BundleSpec = BundleSpec {
    backend: "nym",
    checksum_file: NYM_SMALL_CHECKSUM_FILE,
    required: REQUIRED_NYM_SMALL_ARTIFACTS,
};

/// Fail-closed verification of an installed Nym-small bundle against the pinned digests.
pub fn verify_nym_bundle(model_dir: &Path) -> Result<(), SafetyNetError> {
    verify_bundle(model_dir, NYM_BUNDLE_SPEC, NYM_SMALL_INT8_BUNDLE_SHA256)
}

pub(crate) fn verify_nym_bundle_with_digest(
    model_dir: &Path,
    expected_sha256: &str,
) -> Result<(), SafetyNetError> {
    verify_bundle(model_dir, NYM_BUNDLE_SPEC, expected_sha256)
}

/// Checks that `config.json` lists exactly the 81 BIO labels the decoder assumes, in order.
///
/// The config is already covered by the bundle digest; this ties the decoder's label table to
/// the bytes it runs against instead of trusting a comment.
pub(crate) fn verify_id2label(config_json: &[u8]) -> Result<(), SafetyNetError> {
    let mismatch = |actual: &str| SafetyNetError::ModelIntegrityMismatch {
        expected: "nym id2label (O + B/I for 40 labels)".to_string(),
        actual: actual.to_string(),
    };
    let config: serde_json::Value =
        serde_json::from_slice(config_json).map_err(|_| mismatch("<invalid config.json>"))?;
    let id2label = config
        .get("id2label")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| mismatch("<missing id2label>"))?;
    let expected = expected_id2label();
    if id2label.len() != expected.len() {
        return Err(mismatch("<label count>"));
    }
    for (id, label) in expected.iter().enumerate() {
        if id2label
            .get(&id.to_string())
            .and_then(serde_json::Value::as_str)
            != Some(label.as_str())
        {
            return Err(mismatch("<label order>"));
        }
    }
    Ok(())
}

fn expected_id2label() -> Vec<String> {
    std::iter::once("O".to_string())
        .chain(
            NymLabel::ALL
                .iter()
                .flat_map(|label| [format!("B-{label}"), format!("I-{label}")]),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::hex_sha256;

    #[test]
    fn pinned_bundle_digest_is_the_digest_of_the_pinned_checksum_file() {
        assert_eq!(
            hex_sha256(NYM_SMALL_INT8_SHA256SUMS.as_bytes()),
            NYM_SMALL_INT8_BUNDLE_SHA256
        );
        for (_, bundle_name) in NYM_SMALL_UPSTREAM_FILES {
            assert!(NYM_SMALL_INT8_SHA256SUMS.contains(&format!("  {bundle_name}\n")));
        }
    }

    #[cfg(unix)]
    #[test]
    fn bundle_sha_mismatch_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        crate::bundle::test_bundle::write(
            dir.path(),
            NYM_SMALL_CHECKSUM_FILE,
            &[
                (NYM_SMALL_CONFIG_FILE, b"{}"),
                (NYM_SMALL_MODEL_FILE, b"not the pinned model"),
                (NYM_SMALL_TOKENIZER_FILE, b"{}"),
            ],
        );
        // A self-consistent bundle with the wrong bytes is still refused: the checksum file's
        // own digest is pinned.
        assert!(matches!(
            verify_nym_bundle(dir.path()),
            Err(SafetyNetError::ModelIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn id2label_must_match_the_decoder_table() {
        let labels = expected_id2label();
        let config = |labels: &[String]| {
            let map = labels
                .iter()
                .enumerate()
                .map(|(id, label)| (id.to_string(), serde_json::Value::from(label.as_str())))
                .collect::<serde_json::Map<_, _>>();
            serde_json::to_vec(&serde_json::json!({ "id2label": map })).unwrap()
        };
        assert_eq!(labels.len(), 81);
        assert_eq!(labels[7], "B-BUILDING_NUMBER");
        assert_eq!(labels[80], "I-ZIP_CODE");
        verify_id2label(&config(&labels)).unwrap();

        let mut swapped = labels.clone();
        swapped.swap(7, 8);
        assert!(verify_id2label(&config(&swapped)).is_err());
        assert!(verify_id2label(&config(&labels[..79])).is_err());
        assert!(verify_id2label(b"not json").is_err());
    }
}
