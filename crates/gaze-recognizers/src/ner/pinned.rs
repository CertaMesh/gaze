//! Pinned Davlan mBERT NER bundle: the primary NER model `gaze setup` installs.

use std::path::Path;

use gaze_types::SafetyNetError;

use crate::bundle::{verify_bundle, BundleSpec};

/// Hugging Face repository of the int8 ONNX mirror of `Davlan/bert-base-multilingual-cased-ner-hrl`.
pub const DAVLAN_NER_HF_REPO: &str = "onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX";

/// Immutable upstream revision the bundle is fetched from.
pub const DAVLAN_NER_HF_COMMIT: &str = "cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8";

/// Default directory name under `$XDG_DATA_HOME/gaze/models`.
pub const DAVLAN_NER_MODEL_DIR_NAME: &str = "davlan-mbert-ner-hrl";

/// Gaze-authored label contract installed as `labels.json`.
pub const DAVLAN_NER_LABELS_JSON: &str = include_str!("../../assets/ner/labels.davlan-mbert.json");

/// Every file an installed bundle contains.
pub const REQUIRED_DAVLAN_NER_ARTIFACTS: &[&str] = &[
    "SHA256SUMS",
    "model.onnx",
    "tokenizer.json",
    "tokenizer_config.json",
    "config.json",
    "special_tokens_map.json",
    "vocab.txt",
    "labels.json",
];

/// Upstream path of each downloaded bundle file at [`DAVLAN_NER_HF_COMMIT`], as
/// `(upstream, bundle)` pairs. `labels.json` is not downloaded; it is [`DAVLAN_NER_LABELS_JSON`].
pub const DAVLAN_NER_UPSTREAM_FILES: &[(&str, &str)] = &[
    ("onnx/model_int8.onnx", "model.onnx"),
    ("tokenizer.json", "tokenizer.json"),
    ("tokenizer_config.json", "tokenizer_config.json"),
    ("config.json", "config.json"),
    ("special_tokens_map.json", "special_tokens_map.json"),
    ("vocab.txt", "vocab.txt"),
];

/// Canonical checksum file content, byte-identical to the `SHA256SUMS.ner` release asset.
pub const DAVLAN_NER_SHA256SUMS: &str = concat!(
    "1213fdd405d295768b0d41d8214062f2f278f0e3acff6af67d8fd47360d2be0f  model.onnx\n",
    "bf1b59b7b11c95f194f51708d918eea378e09d05f84c0e1656dc5180e8117088  tokenizer.json\n",
    "470cff6e0353b08e2a6e9b4f61729ecdc47ccb3ced335fa5520e9ce334572d59  tokenizer_config.json\n",
    "8e5caefadaf9923a9e7d3de42ca97780c68fc4d83519d333f141b299e40af638  config.json\n",
    "b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3  special_tokens_map.json\n",
    "fe0fda7c425b48c516fc8f160d594c8022a0808447475c1a7c6d6479763f310c  vocab.txt\n",
    "8498e2bafc017a793571c3c2f7092390a93a757f5ca45004f21db2560a8c6fdb  labels.json\n",
);

/// SHA-256 of [`DAVLAN_NER_SHA256SUMS`], the same digest the benchmark scorecards record for
/// `davlan-mbert-ner-hrl-onnx`. Verified first; then every listed artifact.
pub const DAVLAN_NER_BUNDLE_SHA256: &str =
    "7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16";

const DAVLAN_NER_BUNDLE_SPEC: BundleSpec = BundleSpec {
    backend: "davlan-ner",
    checksum_file: "SHA256SUMS",
    required: REQUIRED_DAVLAN_NER_ARTIFACTS,
};

/// Fail-closed verification of an installed Davlan NER bundle against the pinned digests:
/// missing files, digest mismatches, symlinks, foreign owners and loose modes all refuse.
pub fn verify_davlan_ner_bundle(model_dir: &Path) -> Result<(), SafetyNetError> {
    verify_bundle(model_dir, DAVLAN_NER_BUNDLE_SPEC, DAVLAN_NER_BUNDLE_SHA256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::hex_sha256;

    #[test]
    fn pinned_digest_is_the_digest_of_the_pinned_checksum_file() {
        assert_eq!(
            hex_sha256(DAVLAN_NER_SHA256SUMS.as_bytes()),
            DAVLAN_NER_BUNDLE_SHA256
        );
    }

    #[test]
    fn embedded_labels_match_their_checksum_entry() {
        let expected = DAVLAN_NER_SHA256SUMS
            .lines()
            .find_map(|line| line.strip_suffix("  labels.json"))
            .expect("labels.json entry");
        assert_eq!(hex_sha256(DAVLAN_NER_LABELS_JSON.as_bytes()), expected);
    }

    #[test]
    fn every_required_artifact_is_listed_or_embedded() {
        for required in REQUIRED_DAVLAN_NER_ARTIFACTS {
            if *required == "SHA256SUMS" {
                continue;
            }
            assert!(
                DAVLAN_NER_SHA256SUMS.contains(&format!("  {required}\n")),
                "{required} has no checksum entry"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_self_consistent_bundle_with_other_bytes_is_refused() {
        use crate::bundle::test_bundle::write;

        let dir = tempfile::tempdir().unwrap();
        let files: Vec<(&str, &[u8])> = REQUIRED_DAVLAN_NER_ARTIFACTS
            .iter()
            .filter(|name| **name != "SHA256SUMS")
            .map(|name| (*name, b"not the pinned bytes".as_slice()))
            .collect();
        write(dir.path(), "SHA256SUMS", &files);
        assert!(matches!(
            verify_davlan_ner_bundle(dir.path()),
            Err(SafetyNetError::ModelIntegrityMismatch { .. })
        ));
    }

    struct CurrentDirGuard(std::path::PathBuf);

    impl CurrentDirGuard {
        fn enter(dir: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            Self(previous)
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    /// Release gate (`scripts/gate/model-setup-ownership.sh`): trust follows the process owner,
    /// never the owner of the current directory.
    #[cfg(unix)]
    #[test]
    #[ignore = "requires real pinned bundles and a cross-owner cwd fixture"]
    #[serial_test::serial]
    fn verify_davlan_ner_bundle_is_cwd_independent() {
        let env_dir = |name: &str| {
            std::path::PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
        };
        let model_dir = env_dir("GAZE_MODEL_SETUP_OWNED_MODEL_DIR");
        let foreign_cwd = env_dir("GAZE_MODEL_SETUP_FOREIGN_CWD");
        let foreign_model_dir = env_dir("GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR");

        verify_davlan_ner_bundle(&model_dir).unwrap();
        assert!(verify_davlan_ner_bundle(&foreign_model_dir).is_err());

        let _guard = CurrentDirGuard::enter(&foreign_cwd);
        verify_davlan_ner_bundle(&model_dir).unwrap();
        assert!(verify_davlan_ner_bundle(&foreign_model_dir).is_err());
    }
}
