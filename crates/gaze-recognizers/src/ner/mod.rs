//! Transformer-based NER detector with pluggable backends.

mod backend;
pub(crate) mod decode;
mod detector;
mod error;
mod loader;
mod recognizer;
mod types;

pub use detector::NerDetector;
pub use error::NerLoadError;
pub use recognizer::NerRecognizer;
pub use types::{LabelMap, NerBackendKind, NerOptions, VerifiedArtifacts};

#[cfg(test)]
pub(crate) use types::NerSpanResult;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ner::backend::NerBackend;
    use crate::ner::error::NerRuntimeError;
    use crate::ner::types::{CHECKSUMS_FILE, CONFIG_FILE, LABELS_FILE, MODEL_FILE, TOKENIZER_FILE};
    use gaze_types::{DetectContext, DictionaryBundle, LocaleTag, PiiClass, Recognizer};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn good_labels() -> &'static [u8] {
        br#"{"PER":"Name","LOC":"Location","ORG":"Organization"}"#
    }

    fn good_config() -> &'static [u8] {
        br#"{"id2label":{"0":"O","1":"B-PER","2":"I-PER","3":"B-LOC","4":"I-LOC","5":"B-ORG","6":"I-ORG","7":"B-MISC","8":"I-MISC"}}"#
    }

    fn setup_good_dir() -> tempfile::TempDir {
        setup_dir_with_config(good_config())
    }

    fn setup_dir_with_config(config: &[u8]) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let path = dir.path();
        let model_bytes = b"fake-onnx";
        let tokenizer_bytes = b"fake-tokenizer";
        write(&path.join(MODEL_FILE), model_bytes);
        write(&path.join(TOKENIZER_FILE), tokenizer_bytes);
        write(&path.join(CONFIG_FILE), config);
        write(&path.join(LABELS_FILE), good_labels());
        let sums = format!(
            "{}  {}\n{}  {}\n{}  {}\n{}  {}\n",
            sha256_hex(model_bytes),
            MODEL_FILE,
            sha256_hex(tokenizer_bytes),
            TOKENIZER_FILE,
            sha256_hex(config),
            CONFIG_FILE,
            sha256_hex(good_labels()),
            LABELS_FILE,
        );
        write(&path.join(CHECKSUMS_FILE), sums.as_bytes());
        dir
    }

    #[test]
    fn verify_artifacts_succeeds_on_matching_checksums() {
        let dir = setup_good_dir();
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");
        assert_eq!(verified.backend_kind, NerBackendKind::Ort);
        assert_eq!(verified.recognizer_model_id, "unknown");
        assert_eq!(verified.recognizer_model_version, "v0");
        assert!(verified.labels.get("PER").is_some());
        assert_eq!(verified.id2label[1], "B-PER");
    }

    #[test]
    fn verify_artifacts_reads_versioned_ner_identity_from_config() {
        let dir = setup_dir_with_config(
            br#"{"model_id":"Davlan/mBERT NER HRL","model_version":"1","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");

        assert_eq!(verified.recognizer_model_id, "davlan-mbert-ner-hrl");
        assert_eq!(verified.recognizer_model_version, "v1");
    }

    #[test]
    fn verify_artifacts_honors_explicit_backend_selection() {
        let dir = setup_dir_with_config(
            br#"{"backend":"gliner","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let verified = NerDetector::verify_artifacts(dir.path()).expect("verify");
        assert_eq!(verified.backend_kind, NerBackendKind::Gliner);
    }

    #[test]
    fn load_fails_closed_for_gliner_backend_until_impl_lands() {
        let dir = setup_dir_with_config(
            br#"{"backend":"gliner","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let err = NerDetector::load(dir.path()).unwrap_err();
        assert!(
            matches!(&err, NerLoadError::UnsupportedBackend { backend } if backend == "gliner"),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn load_fails_closed_for_unknown_backend() {
        let dir = setup_dir_with_config(
            br#"{"backend":"nonesuch","id2label":{"0":"O","1":"B-PER","2":"I-PER"}}"#,
        );
        let err = NerDetector::load(dir.path()).unwrap_err();
        assert!(
            matches!(&err, NerLoadError::UnsupportedBackend { backend } if backend == "nonesuch"),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn checksum_mismatch_fails_closed() {
        let dir = setup_good_dir();
        fs::write(dir.path().join(MODEL_FILE), b"tampered").unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumMismatch { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_artifact_fails_closed() {
        let dir = setup_good_dir();
        fs::remove_file(dir.path().join(TOKENIZER_FILE)).unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::MissingArtifact { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_sums_fails_closed() {
        let dir = tempdir().unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumsMissing { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn missing_model_dir_fails_closed() {
        let path = PathBuf::from("/definitely/not/a/path/gaze-ner-xyz");
        let err = NerDetector::verify_artifacts(&path).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ModelDirMissing { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn label_map_parse_error_fails_closed() {
        let dir = setup_good_dir();
        fs::write(dir.path().join(LABELS_FILE), b"{not-json").unwrap();
        let labels_bytes = fs::read(dir.path().join(LABELS_FILE)).unwrap();
        let model_bytes = fs::read(dir.path().join(MODEL_FILE)).unwrap();
        let tokenizer_bytes = fs::read(dir.path().join(TOKENIZER_FILE)).unwrap();
        let config_bytes = fs::read(dir.path().join(CONFIG_FILE)).unwrap();
        let sums = format!(
            "{}  {}\n{}  {}\n{}  {}\n{}  {}\n",
            sha256_hex(&model_bytes),
            MODEL_FILE,
            sha256_hex(&tokenizer_bytes),
            TOKENIZER_FILE,
            sha256_hex(&config_bytes),
            CONFIG_FILE,
            sha256_hex(&labels_bytes),
            LABELS_FILE,
        );
        fs::write(dir.path().join(CHECKSUMS_FILE), sums.as_bytes()).unwrap();
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::LabelsParse(_)),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn malformed_checksums_fail_closed() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join(CHECKSUMS_FILE),
            b"not-a-hash  model.onnx\n",
        );
        let err = NerDetector::verify_artifacts(dir.path()).unwrap_err();
        assert!(
            matches!(err, NerLoadError::ChecksumsMalformed { .. }),
            "unexpected: {err:?}"
        );
    }

    #[test]
    fn merge_bio_merges_adjacent_i_tags() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 6), (7, 13)];
        let tags = vec!["B-PER", "I-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..13);
        assert_eq!(out[0].class, PiiClass::Name);
    }

    #[test]
    fn merge_bio_splits_on_new_b_tag() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 3), (4, 7)];
        let tags = vec!["B-PER", "B-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].span, 0..3);
        assert_eq!(out[1].span, 4..7);
    }

    #[test]
    fn merge_bio_drops_unmapped_labels() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 4)];
        let tags = vec!["B-MISC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert!(out.is_empty());
    }

    /// Regression: adopters who follow `labels.example.json` ship labels.json
    /// keyed by full BIO tags (`B-PER`, `I-PER`, …). Until v0.3.1 the
    /// post-process only looked up the stripped entity (`PER`), so every
    /// detection silently dropped — reported by an adopter against
    /// v0.3.0 aarch64-apple-darwin. Both shapes must emit detections.
    #[test]
    fn merge_bio_accepts_bio_prefixed_label_keys() {
        let mut map = BTreeMap::new();
        map.insert("B-PER".to_string(), PiiClass::Name);
        map.insert("I-PER".to_string(), PiiClass::Name);
        map.insert("B-LOC".to_string(), PiiClass::Location);
        map.insert("I-LOC".to_string(), PiiClass::Location);
        let labels = LabelMap(map);
        let spans = vec![
            (0, 4),
            (5, 9),
            (10, 13),
            (14, 22),
            (23, 26),
            (27, 30),
            (31, 36),
            (37, 39),
            (40, 46),
        ];
        let tags = vec!["O", "O", "O", "B-PER", "O", "O", "O", "O", "B-LOC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner/ort");
        assert_eq!(out.len(), 2, "both Wolfgang + Berlin must emit: {out:?}");
        assert_eq!(out[0].span, 14..22);
        assert_eq!(out[0].class, PiiClass::Name);
        assert_eq!(out[1].span, 40..46);
        assert_eq!(out[1].class, PiiClass::Location);
    }

    /// Mixing both key shapes (bare entity + BIO-prefixed) in one labels.json
    /// must keep working — we don't want adopters editing a mixed file to
    /// discover a silent regression. BIO wins when both present.
    #[test]
    fn merge_bio_accepts_mixed_key_shapes() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        map.insert("B-LOC".to_string(), PiiClass::Location);
        map.insert("I-LOC".to_string(), PiiClass::Location);
        let labels = LabelMap(map);
        let spans = vec![(0, 4), (5, 11)];
        let tags = vec!["B-PER", "B-LOC"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner/ort");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].class, PiiClass::Name);
        assert_eq!(out[1].class, PiiClass::Location);
    }

    #[test]
    fn merge_bio_skips_special_token_empty_offsets() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 0), (0, 5)];
        let tags = vec!["B-PER", "B-PER"];
        let out = NerDetector::merge_bio_spans(&labels, &spans, &tags, "ner");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..5);
    }

    #[test]
    fn merge_bio_spans_returns_min_confidence_with_one_low_token() {
        let mut map = BTreeMap::new();
        map.insert("PER".to_string(), PiiClass::Name);
        let labels = LabelMap(map);
        let spans = vec![(0, 4), (5, 10), (11, 16)];
        let tags = vec!["B-PER", "I-PER", "I-PER"];
        let scores = vec![0.91, 0.34, 0.88];

        let out = NerDetector::merge_bio_span_results(&labels, &spans, &tags, &scores, "ner");

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..16);
        assert_eq!(out[0].score, 0.34);
    }

    #[test]
    fn ner_recognizer_filters_below_threshold() {
        struct FixedBackend {
            spans: Vec<NerSpanResult>,
        }

        impl NerBackend for FixedBackend {
            fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Ok(self.spans.clone())
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: None,
                threshold: 0.5,
                backend: Arc::new(FixedBackend {
                    spans: vec![
                        NerSpanResult {
                            span: 0..5,
                            class: PiiClass::Name,
                            score: 0.49,
                        },
                        NerSpanResult {
                            span: 6..11,
                            class: PiiClass::Name,
                            score: 0.50,
                        },
                    ],
                }),
            },
        };
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let candidates = Recognizer::detect(&recognizer, "alpha bravo", &ctx).unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].span, 6..11);
        assert_eq!(candidates[0].score, 0.50);
        assert_eq!(
            candidates[0].recognizer_version_id.as_deref(),
            Some("ner.fixed.v1")
        );
    }

    #[test]
    fn ner_recognizer_surfaces_backend_failure() {
        struct FailingBackend;

        impl NerBackend for FailingBackend {
            fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Err(NerRuntimeError::Inference("synthetic failure".to_string()))
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: None,
                threshold: 0.5,
                backend: Arc::new(FailingBackend),
            },
        };
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let err = Recognizer::detect(&recognizer, "Dr. Schmidt", &ctx)
            .expect_err("backend failure must be caller-visible");

        assert!(matches!(
            err,
            gaze_types::DetectError::Backend {
                recognizer_id,
                message,
                ..
            } if recognizer_id == "ner" && message.contains("synthetic failure")
        ));
    }

    #[test]
    fn ner_recognizer_chunks_long_input_and_offsets_spans() {
        struct WindowBackend;

        impl NerBackend for WindowBackend {
            fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                if input.split_whitespace().count() > 512 {
                    return Err(NerRuntimeError::Inference(
                        "window exceeded model limit".to_string(),
                    ));
                }
                Ok(input
                    .find("Dr. Schmidt")
                    .map(|start| NerSpanResult {
                        span: start..start + "Dr. Schmidt".len(),
                        class: PiiClass::Name,
                        score: 0.91,
                    })
                    .into_iter()
                    .collect())
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: None,
                threshold: 0.5,
                backend: Arc::new(WindowBackend),
            },
        };
        let input = format!("{} Dr. Schmidt", vec!["word"; 520].join(" "));
        let entity_start = input.find("Dr. Schmidt").expect("fixture entity");
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let candidates = Recognizer::detect(&recognizer, &input, &ctx).unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].span,
            entity_start..entity_start + "Dr. Schmidt".len()
        );
        assert_eq!(candidates[0].class, PiiClass::Name);
    }

    #[test]
    fn t21f_threshold_filtering_unit() {
        struct FixedBackend {
            spans: Vec<NerSpanResult>,
        }

        impl NerBackend for FixedBackend {
            fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Ok(self.spans.clone())
            }
        }

        let input = "Du antwortest als Artistfy-Support an Alice Example.";
        let name_start = input.find("Alice Example").expect("name span start");
        let name_end = name_start + "Alice Example".len();
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::DeDe, LocaleTag::Global], &dictionaries);
        let backend = Arc::new(FixedBackend {
            spans: vec![NerSpanResult {
                span: name_start..name_end,
                class: PiiClass::Name,
                score: 0.40,
            }],
        });
        let default_threshold = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: Some("de".to_string()),
                threshold: 0.3,
                backend: backend.clone(),
            },
        };
        let stricter_threshold = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: Some("de".to_string()),
                threshold: 0.5,
                backend,
            },
        };

        let default_candidates = Recognizer::detect(&default_threshold, input, &ctx).unwrap();
        let stricter_candidates = Recognizer::detect(&stricter_threshold, input, &ctx).unwrap();

        assert_eq!(default_candidates.len(), 1);
        assert_eq!(default_candidates[0].span, name_start..name_end);
        assert_eq!(&input[default_candidates[0].span.clone()], "Alice Example");
        assert_eq!(default_candidates[0].score, 0.40);
        assert!(stricter_candidates.is_empty());
    }

    #[test]
    fn ner_backend_error_fails_closed() {
        struct ErrorBackend;

        impl NerBackend for ErrorBackend {
            fn detect(&self, _input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Err(NerRuntimeError::Inference(
                    "forced backend failure".to_string(),
                ))
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: None,
                threshold: 0.5,
                backend: Arc::new(ErrorBackend),
            },
        };
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let err = recognizer
            .detect("Alice Example", &ctx)
            .expect_err("backend runtime failures must surface");

        assert!(matches!(
            err,
            gaze_types::DetectError::Backend {
                recognizer_id,
                message,
                ..
            } if recognizer_id == "ner" && message.contains("forced backend failure")
        ));
    }

    #[test]
    fn ner_long_input_chunked_detects() {
        struct FixedAliceBackend;

        impl NerBackend for FixedAliceBackend {
            fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
                Ok(input
                    .find("Alice Example")
                    .map(|start| NerSpanResult {
                        span: start..start + "Alice Example".len(),
                        class: PiiClass::Name,
                        score: 0.40,
                    })
                    .into_iter()
                    .collect())
            }
        }

        let recognizer = NerRecognizer {
            detector: NerDetector {
                model_dir: PathBuf::from("/test/fake"),
                backend_kind: NerBackendKind::Ort,
                recognizer_version_id: "ner.fixed.v1".to_string(),
                locale: None,
                threshold: 0.3,
                backend: Arc::new(FixedAliceBackend),
            },
        };
        let prefix = (0..540)
            .map(|idx| format!("word{idx}"))
            .collect::<Vec<_>>()
            .join(" ");
        let input = format!("{prefix} Alice Example met the team.");
        let start = input.find("Alice Example").expect("name span start");
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);

        let candidates = recognizer.detect(&input, &ctx).expect("detect");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].span, start..start + "Alice Example".len());
        assert_eq!(&input[candidates[0].span.clone()], "Alice Example");
    }
}
