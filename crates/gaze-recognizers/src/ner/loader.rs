use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use gaze_types::PiiClass;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::decode::split_bio;
use super::detector::NerDetector;
use super::error::NerLoadError;
use super::types::{
    LabelMap, NerBackendKind, VerifiedArtifacts, CHECKSUMS_FILE, CONFIG_FILE, LABELS_FILE,
    MODEL_FILE, TOKENIZER_FILE,
};

impl NerDetector {
    /// Verify SHA256SUMS, parse labels.json and config.json. No backend
    /// initialization. Used both by `load` and by unit tests.
    pub fn verify_artifacts(model_dir: &Path) -> Result<VerifiedArtifacts, NerLoadError> {
        if !model_dir.is_dir() {
            return Err(NerLoadError::ModelDirMissing {
                path: model_dir.to_path_buf(),
            });
        }

        let sums_path = model_dir.join(CHECKSUMS_FILE);
        if !sums_path.exists() {
            return Err(NerLoadError::ChecksumsMissing { path: sums_path });
        }

        let entries = parse_checksums(&sums_path)?;
        for required in [MODEL_FILE, TOKENIZER_FILE, CONFIG_FILE, LABELS_FILE] {
            if !entries.iter().any(|(name, _)| name == required) {
                return Err(NerLoadError::MissingArtifact {
                    path: model_dir.join(required),
                });
            }
        }

        for (rel, expected) in &entries {
            let path = model_dir.join(rel);
            if !path.exists() {
                return Err(NerLoadError::MissingArtifact { path });
            }
            let got = hash_file(&path)?;
            if !got.eq_ignore_ascii_case(expected) {
                return Err(NerLoadError::ChecksumMismatch {
                    path,
                    expected: expected.clone(),
                    got,
                });
            }
        }

        let labels = parse_labels(&model_dir.join(LABELS_FILE))?;
        let config = parse_config(&model_dir.join(CONFIG_FILE))?;
        let backend_kind = NerBackendKind::parse(config.backend.as_deref())?;
        let id2label = config_to_id2label(config.id2label)?;

        Ok(VerifiedArtifacts {
            model_dir: model_dir.to_path_buf(),
            backend_kind,
            labels,
            id2label,
        })
    }
}

pub(crate) fn warn_on_label_vocab_mismatch(
    labels: &LabelMap,
    id2label: &[String],
    model_dir: &Path,
) {
    let mut usable = 0usize;
    for tag in id2label {
        let (_, entity) = split_bio(tag);
        if entity.is_empty() {
            continue;
        }
        if labels.resolve(tag, entity).is_some() {
            usable += 1;
        }
    }
    if usable == 0 {
        let sample_label: String = labels.keys().take(5).collect::<Vec<_>>().join(",");
        let sample_id: String = id2label
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        tracing::warn!(
            model_dir = %model_dir.display(),
            label_keys = %sample_label,
            id2label_sample = %sample_id,
            "ner: labels.json has zero overlap with model id2label — detector will emit zero detections. Expected keys like 'PER'/'LOC' or 'B-PER'/'I-PER'."
        );
    }
}

fn parse_checksums(path: &Path) -> Result<Vec<(String, String)>, NerLoadError> {
    let file = fs::File::open(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.map_err(|source| NerLoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let Some(hash) = parts.next() else {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "missing hash".into(),
            });
        };
        let Some(rest) = parts.next() else {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "missing path".into(),
            });
        };
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: format!("invalid sha256 hex: {hash}"),
            });
        }
        let file = rest.trim_start().trim_start_matches('*').trim().to_string();
        if file.is_empty() {
            return Err(NerLoadError::ChecksumsMalformed {
                line: line_no,
                reason: "empty path".into(),
            });
        }
        entries.push((file, hash.to_ascii_lowercase()));
    }
    Ok(entries)
}

fn hash_file(path: &Path) -> Result<String, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn parse_labels(path: &Path) -> Result<LabelMap, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let raw: BTreeMap<String, String> =
        serde_json::from_slice(&bytes).map_err(|err| NerLoadError::LabelsParse(err.to_string()))?;
    let mut map = BTreeMap::new();
    for (key, value) in raw {
        let class = match value.to_ascii_lowercase().as_str() {
            "name" | "per" | "person" => PiiClass::Name,
            "location" | "loc" => PiiClass::Location,
            "organization" | "org" => PiiClass::Organization,
            "email" => PiiClass::Email,
            "drop" | "ignore" | "" => continue,
            other => PiiClass::custom(other),
        };
        map.insert(key, class);
    }
    if map.is_empty() {
        return Err(NerLoadError::LabelsParse(
            "labels.json produced no usable mappings".into(),
        ));
    }
    Ok(LabelMap(map))
}

#[derive(Deserialize)]
struct ConfigFile {
    backend: Option<String>,
    id2label: Option<BTreeMap<String, String>>,
}

fn parse_config(path: &Path) -> Result<ConfigFile, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|err| NerLoadError::ConfigParse(err.to_string()))
}

fn config_to_id2label(
    id2label: Option<BTreeMap<String, String>>,
) -> Result<Vec<String>, NerLoadError> {
    let map = id2label
        .ok_or_else(|| NerLoadError::ConfigParse("config.json missing id2label".to_string()))?;
    let mut pairs: Vec<(usize, String)> = map
        .into_iter()
        .map(|(key, value)| {
            key.parse::<usize>()
                .map(|index| (index, value))
                .map_err(|err| NerLoadError::ConfigParse(format!("id2label key {key}: {err}")))
        })
        .collect::<Result<_, _>>()?;
    pairs.sort_by_key(|(index, _)| *index);
    let max_idx = pairs.last().map(|(index, _)| *index).unwrap_or(0);
    let mut out = vec!["O".to_string(); max_idx + 1];
    for (index, label) in pairs {
        out[index] = label;
    }
    Ok(out)
}
