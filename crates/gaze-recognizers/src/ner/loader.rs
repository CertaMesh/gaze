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
        for required in [MODEL_FILE, TOKENIZER_FILE, LABELS_FILE] {
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

        let parsed_labels = parse_labels(&model_dir.join(LABELS_FILE))?;
        let config = if entries.iter().any(|(name, _)| name == CONFIG_FILE) {
            parse_config(&model_dir.join(CONFIG_FILE))?
        } else if let Some(kiji_config) = parsed_labels.kiji_config {
            kiji_config
        } else {
            return Err(NerLoadError::MissingArtifact {
                path: model_dir.join(CONFIG_FILE),
            });
        };
        let backend_kind = NerBackendKind::parse(config.backend.as_deref())?;
        let recognizer_model_id = config
            .recognizer_model_id()
            .map(sanitize_recognizer_segment)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_string());
        let recognizer_model_version = config
            .recognizer_model_version()
            .map(sanitize_version_segment)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "v0".to_string());
        let id2label = config_to_id2label(config.id2label)?;

        Ok(VerifiedArtifacts {
            model_dir: model_dir.to_path_buf(),
            backend_kind,
            recognizer_model_id,
            recognizer_model_version,
            labels: parsed_labels.labels,
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

struct ParsedLabels {
    labels: LabelMap,
    kiji_config: Option<ConfigFile>,
}

fn parse_labels(path: &Path) -> Result<ParsedLabels, NerLoadError> {
    let bytes = fs::read(path).map_err(|source| NerLoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if let Ok(raw) = serde_json::from_slice::<BTreeMap<String, String>>(&bytes) {
        return parse_flat_labels(raw).map(|labels| ParsedLabels {
            labels,
            kiji_config: None,
        });
    }

    let raw: KijiLabelsFile =
        serde_json::from_slice(&bytes).map_err(|err| NerLoadError::LabelsParse(err.to_string()))?;
    parse_kiji_labels(raw)
}

fn parse_flat_labels(raw: BTreeMap<String, String>) -> Result<LabelMap, NerLoadError> {
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
struct KijiLabelsFile {
    schema_version: u64,
    source: String,
    source_commit: String,
    labels: Vec<KijiLabelEntry>,
}

#[derive(Deserialize)]
struct KijiLabelEntry {
    id: String,
    upstream: Vec<String>,
}

fn parse_kiji_labels(raw: KijiLabelsFile) -> Result<ParsedLabels, NerLoadError> {
    if raw.schema_version != 1
        || raw.source != "onnx-community/distilbert-NER-ONNX"
        || raw.source_commit != "3a19fe9404a4469d91aa3d551558a97f68872f67"
    {
        return Err(NerLoadError::LabelsParse(
            "unsupported Kiji labels.json manifest".to_string(),
        ));
    }

    let mut labels = BTreeMap::new();
    let mut id2label = vec!["O".to_string()];
    for entry in raw.labels {
        let class = match entry.id.as_str() {
            "person" => PiiClass::Name,
            "location" => PiiClass::Location,
            "organization" => PiiClass::Organization,
            "miscellaneous" => PiiClass::custom("miscellaneous"),
            other => {
                return Err(NerLoadError::LabelsParse(format!(
                    "unsupported Kiji label id `{other}`"
                )));
            }
        };
        labels.insert(entry.id, class.clone());
        for upstream in entry.upstream {
            if upstream.is_empty() || upstream.contains('/') || upstream.contains('\\') {
                return Err(NerLoadError::LabelsParse(
                    "invalid Kiji upstream label".to_string(),
                ));
            }
            if !id2label.iter().any(|existing| existing == &upstream) {
                id2label.push(upstream.clone());
            }
            labels.insert(upstream, class.clone());
        }
    }
    if labels.is_empty() {
        return Err(NerLoadError::LabelsParse(
            "labels.json produced no usable mappings".into(),
        ));
    }

    let id2label = id2label
        .into_iter()
        .enumerate()
        .map(|(index, label)| (index.to_string(), label))
        .collect();
    Ok(ParsedLabels {
        labels: LabelMap(labels),
        kiji_config: Some(ConfigFile {
            backend: Some("ort".to_string()),
            model_id: Some(raw.source),
            model_name: None,
            model: None,
            version: None,
            model_version: Some(raw.source_commit),
            recognizer_version: None,
            id2label: Some(id2label),
        }),
    })
}

#[derive(Deserialize)]
struct ConfigFile {
    backend: Option<String>,
    model_id: Option<String>,
    model_name: Option<String>,
    model: Option<String>,
    version: Option<String>,
    model_version: Option<String>,
    recognizer_version: Option<String>,
    id2label: Option<BTreeMap<String, String>>,
}

impl ConfigFile {
    fn recognizer_model_id(&self) -> Option<&str> {
        self.model_id
            .as_deref()
            .or(self.model_name.as_deref())
            .or(self.model.as_deref())
    }

    fn recognizer_model_version(&self) -> Option<&str> {
        self.recognizer_version
            .as_deref()
            .or(self.model_version.as_deref())
            .or(self.version.as_deref())
    }
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

fn sanitize_recognizer_segment(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            _ => '-',
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn sanitize_version_segment(raw: &str) -> String {
    let sanitized = sanitize_recognizer_segment(raw);
    if sanitized.is_empty() || sanitized.starts_with('v') {
        sanitized
    } else {
        format!("v{sanitized}")
    }
}
