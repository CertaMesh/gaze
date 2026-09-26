//! Opt-in, local GLiNER judge for dates the rule floor has not identified as birth dates.

use std::ops::Range;
use std::path::Path;
use std::sync::Mutex;

use gaze_types::{Candidate, ConflictTier, DetectContext, DetectError, PiiClass, Recognizer};
use regex::Regex;
use thiserror::Error;

use crate::bundle::{verify_bundle, BundleSpec};

pub const GLINER_DOB_HF_REPO: &str = "onnx-community/gliner_multi_pii-v1";
pub const GLINER_DOB_HF_COMMIT: &str = "2e0397a7e8a250d76c37122232b3cbde42c8d629";
pub const GLINER_DOB_MODEL_DIR_NAME: &str = "gliner-multi-pii-dob-int8";
pub const GLINER_DOB_SHA256SUMS: &str = concat!(
    "3efb3b91aef91ae11cd781126133063a33a2ffd7787ec73057bbd57a9781a7ab  model.onnx\n",
    "914bd3c8fb7b525af9e23b60d0ec7b1248ddb2b99014efd9c02ebeb022f8cab7  tokenizer.json\n",
    "69e141f7fe1864e0d81ab0e542c68387d588db62393bcd93b471d54dcf0f5c16  gliner_config.json\n",
);
pub const GLINER_DOB_BUNDLE_SHA256: &str =
    "eddb1943a13778f91bf9e51f23cdeb1f8b54d9c83f7a5831aa06320fe7cc4b23";
pub const REQUIRED_GLINER_DOB_ARTIFACTS: &[&str] = &[
    "SHA256SUMS",
    "model.onnx",
    "tokenizer.json",
    "gliner_config.json",
];
pub const GLINER_DOB_UPSTREAM_FILES: &[(&str, &str)] = &[
    ("onnx/model_int8.onnx", "model.onnx"),
    ("tokenizer.json", "tokenizer.json"),
    ("gliner_config.json", "gliner_config.json"),
];

const BUNDLE_SPEC: BundleSpec = BundleSpec {
    backend: "gliner-dob",
    checksum_file: "SHA256SUMS",
    required: REQUIRED_GLINER_DOB_ARTIFACTS,
};
const ID: &str = "dob.gliner";
const LABEL: &str = "date of birth";
const MAX_SUBTOKENS: usize = 384;
const MAX_WIDTH: usize = 12;

// The scanner only proposes date spans. It does not emit tokens without a model judgment.
// Numeric shapes and EN/DE/FR month names match the core birth-date rule's common cases.
const DATE_PATTERN: &str = r"(?ix)
\b(?:
    (?:19|20)[0-9]{2}[-/.](?:0?[1-9]|1[0-2])[-/.](?:0?[1-9]|[12][0-9]|3[01])
  | (?:19|20)[0-9]{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])
  | (?:0?[1-9]|[12][0-9]|3[01])[-./](?:0?[1-9]|1[0-2])[-./](?:(?:19|20)[0-9]{2}|[0-9]{2})
  | (?:0?[1-9]|[12][0-9]|3[01])(?:st|nd|rd|th|er)?\.?[\x20\t\x{A0}\x{202F}]+
    (?:(?:of|de)[\x20\t\x{A0}\x{202F}]+)?
    (?:january|february|march|april|may|june|july|august|september|october|november|december
      |januar|jänner|februar|märz|mai|juni|juli|oktober|dezember
      |janvier|février|fevrier|mars|avril|juin|juillet|août|aout|septembre|octobre|novembre|décembre|decembre
      |jan|feb|mar|apr|jun|jul|aug|sep|oct|nov|dec|mär|mrz|okt|dez|févr|déc)\.?,?
    [\x20\t\x{A0}\x{202F}]+(?:de[\x20\t\x{A0}\x{202F}]+)?(?:19|20)[0-9]{2}
  | (?:january|february|march|april|may|june|july|august|september|october|november|december
      |jan|feb|mar|apr|jun|jul|aug|sep|oct|nov|dec)\.?
    [\x20\t\x{A0}\x{202F}]+(?:0?[1-9]|[12][0-9]|3[01])(?:st|nd|rd|th)?,?
    [\x20\t\x{A0}\x{202F}]+(?:19|20)[0-9]{2}
)\b";
const WORD_PATTERN: &str = r"\w+(?:[-_]\w+)*|\S";
const NON_BIRTH_CONTEXT: &str = r"(?i)\b(?:account[[:space:]]+opened|invoice|due|shipped|version|log(?:ged)?|rechnung|facture|fällig|expédié)\b";

#[derive(Debug, Error)]
pub enum DobJudgeLoadError {
    #[error("GLiNER DOB bundle verification failed: {0}")]
    Bundle(#[from] gaze_types::SafetyNetError),
    #[error("GLiNER DOB tokenizer is invalid")]
    Tokenizer,
    #[error("GLiNER DOB model could not load")]
    Model,
    #[error("GLiNER DOB threshold must be between zero and one")]
    Threshold,
    #[error("GLiNER DOB scanner could not compile")]
    Scanner,
}

pub fn verify_gliner_dob_bundle(model_dir: &Path) -> Result<(), gaze_types::SafetyNetError> {
    verify_bundle(model_dir, BUNDLE_SPEC, GLINER_DOB_BUNDLE_SHA256)
}

struct Word {
    span: Range<usize>,
    ids: Vec<i64>,
}

pub struct DobJudgeRecognizer {
    class: PiiClass,
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    date_regex: Regex,
    word_regex: Regex,
    negative_regex: Regex,
    threshold: f32,
}

impl DobJudgeRecognizer {
    pub fn load(model_dir: &Path, threshold: f32) -> Result<Self, DobJudgeLoadError> {
        if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
            return Err(DobJudgeLoadError::Threshold);
        }
        verify_gliner_dob_bundle(model_dir)?;
        let tokenizer = tokenizers::Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|_| DobJudgeLoadError::Tokenizer)?;
        let session = ort::session::Session::builder()
            .map_err(|_| DobJudgeLoadError::Model)?
            .commit_from_file(model_dir.join("model.onnx"))
            .map_err(|_| DobJudgeLoadError::Model)?;
        Ok(Self {
            class: PiiClass::Custom("birth_date".into()),
            tokenizer,
            session: Mutex::new(session),
            date_regex: Regex::new(DATE_PATTERN).map_err(|_| DobJudgeLoadError::Scanner)?,
            word_regex: Regex::new(WORD_PATTERN).map_err(|_| DobJudgeLoadError::Scanner)?,
            negative_regex: Regex::new(NON_BIRTH_CONTEXT)
                .map_err(|_| DobJudgeLoadError::Scanner)?,
            threshold,
        })
    }

    fn encode_words(&self, input: &str) -> Result<Vec<Word>, DetectError> {
        self.word_regex
            .find_iter(input)
            .map(|found| {
                let ids = self
                    .tokenizer
                    .encode(found.as_str(), false)
                    .map_err(|_| DetectError::backend(ID, "tokenization failed"))?
                    .get_ids()
                    .iter()
                    .map(|id| i64::from(*id))
                    .collect();
                Ok(Word {
                    span: found.range(),
                    ids,
                })
            })
            .collect()
    }

    fn prompt_ids(&self) -> Result<Vec<i64>, DetectError> {
        let mut ids = vec![1];
        for part in ["<<ENT>>", LABEL, "<<SEP>>"] {
            ids.extend(
                self.tokenizer
                    .encode(part, false)
                    .map_err(|_| DetectError::backend(ID, "prompt tokenization failed"))?
                    .get_ids()
                    .iter()
                    .map(|id| i64::from(*id)),
            );
        }
        Ok(ids)
    }

    fn proposed_dates(&self, input: &str, prior: &[Candidate]) -> Vec<Range<usize>> {
        self.date_regex
            .find_iter(input)
            .map(|found| found.range())
            .filter(|span| date_boundary_is_valid(input, span))
            .filter(|span| {
                !prior.iter().any(|candidate| {
                    candidate.class == self.class
                        && candidate.span.start < span.end
                        && span.start < candidate.span.end
                })
            })
            .filter(|span| !self.has_negative_context(input, span.start))
            .collect()
    }

    fn has_negative_context(&self, input: &str, start: usize) -> bool {
        let before = &input[..start];
        let clause = before
            .rsplit(['\n', ';', '.', '!', '?'])
            .next()
            .unwrap_or(before);
        self.negative_regex.is_match(clause)
    }

    fn score_span(
        &self,
        words: &[Word],
        start: usize,
        end: usize,
        prompt_ids: &[i64],
    ) -> Result<f32, DetectError> {
        if words.is_empty() || start >= end || end > words.len() || end - start > MAX_WIDTH {
            return Err(DetectError::backend(ID, "date span exceeds model width"));
        }
        let budget = MAX_SUBTOKENS.saturating_sub(prompt_ids.len() + 1);
        let all_subtokens: usize = words.iter().map(|word| word.ids.len()).sum();
        let (window_start, window_end) = if all_subtokens <= budget {
            (0, words.len())
        } else {
            let own: usize = words[start..end].iter().map(|word| word.ids.len()).sum();
            if own > budget {
                return Err(DetectError::backend(ID, "date exceeds model token budget"));
            }
            let (mut left, mut right, mut used) = (start, end, own);
            while left > 0 || right < words.len() {
                let before = left
                    .checked_sub(1)
                    .filter(|index| used + words[*index].ids.len() <= budget);
                let after = (right < words.len() && used + words[right].ids.len() <= budget)
                    .then_some(right);
                let choice = match (before, after) {
                    (Some(a), Some(b)) if start - a <= b - end => Some((true, a)),
                    (Some(_), Some(b)) => Some((false, b)),
                    (Some(a), None) => Some((true, a)),
                    (None, Some(b)) => Some((false, b)),
                    (None, None) => None,
                };
                let Some((take_before, index)) = choice else {
                    break;
                };
                used += words[index].ids.len();
                if take_before {
                    left -= 1;
                } else {
                    right += 1;
                }
            }
            (left, right)
        };
        let window = &words[window_start..window_end];
        let mut ids = prompt_ids.to_vec();
        let mut word_mask = vec![0i64; ids.len()];
        for (index, word) in window.iter().enumerate() {
            for (piece_index, piece) in word.ids.iter().enumerate() {
                ids.push(*piece);
                word_mask.push(if piece_index == 0 {
                    (index + 1) as i64
                } else {
                    0
                });
            }
        }
        ids.push(2);
        word_mask.push(0);
        let seq_len = ids.len();
        let num_words = window.len();
        let mut span_idx = Vec::with_capacity(num_words * MAX_WIDTH * 2);
        let mut span_mask = Vec::with_capacity(num_words * MAX_WIDTH);
        for word_start in 0..num_words {
            for width in 0..MAX_WIDTH {
                span_idx.push(word_start as i64);
                span_idx.push((word_start + width) as i64);
                span_mask.push(word_start + width < num_words);
            }
        }
        let inputs = ort::inputs![
            "input_ids" => ort::value::Tensor::from_array(([1, seq_len], ids))
                .map_err(|_| DetectError::backend(ID, "input tensor failed"))?,
            "attention_mask" => ort::value::Tensor::from_array(([1, seq_len], vec![1i64; seq_len]))
                .map_err(|_| DetectError::backend(ID, "attention tensor failed"))?,
            "words_mask" => ort::value::Tensor::from_array(([1, seq_len], word_mask))
                .map_err(|_| DetectError::backend(ID, "word mask tensor failed"))?,
            "text_lengths" => ort::value::Tensor::from_array(([1, 1], vec![num_words as i64]))
                .map_err(|_| DetectError::backend(ID, "length tensor failed"))?,
            "span_idx" => ort::value::Tensor::from_array(([1, num_words * MAX_WIDTH, 2], span_idx))
                .map_err(|_| DetectError::backend(ID, "span tensor failed"))?,
            "span_mask" => ort::value::Tensor::from_array(([1, num_words * MAX_WIDTH], span_mask))
                .map_err(|_| DetectError::backend(ID, "span mask tensor failed"))?,
        ];
        let mut session = self
            .session
            .lock()
            .map_err(|_| DetectError::backend(ID, "model lock failed"))?;
        let outputs = session
            .run(inputs)
            .map_err(|_| DetectError::backend(ID, "inference failed"))?;
        let (shape, logits) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|_| DetectError::backend(ID, "invalid logits type"))?;
        if shape.len() != 4
            || shape[0] != 1
            || shape[1] != num_words as i64
            || shape[2] != MAX_WIDTH as i64
            || shape[3] != 1
        {
            return Err(DetectError::backend(ID, "invalid logits shape"));
        }
        let relative_start = start - window_start;
        let width = end - start - 1;
        let logit = logits[relative_start * MAX_WIDTH + width];
        if !logit.is_finite() {
            return Err(DetectError::backend(ID, "non-finite logits"));
        }
        Ok(1.0 / (1.0 + (-logit).exp()))
    }
}

impl Recognizer for DobJudgeRecognizer {
    fn id(&self) -> &str {
        ID
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "birth_date"
    }
    fn detect_is_locale_invariant(&self) -> bool {
        true
    }
    fn requires_prior_candidates(&self) -> bool {
        true
    }

    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let prior = ctx
            .prior_candidates
            .ok_or_else(|| DetectError::backend(ID, "rule-floor candidates unavailable"))?;
        let dates = self.proposed_dates(input, prior);
        if dates.is_empty() {
            return Ok(Vec::new());
        }
        let words = self.encode_words(input)?;
        let prompt_ids = self.prompt_ids()?;
        let mut candidates = Vec::new();
        for span in dates {
            let start = words
                .iter()
                .position(|word| word.span.start == span.start)
                .ok_or_else(|| DetectError::backend(ID, "date start is not word-aligned"))?;
            let end = words
                .iter()
                .position(|word| word.span.end == span.end)
                .map(|index| index + 1)
                .ok_or_else(|| DetectError::backend(ID, "date end is not word-aligned"))?;
            let score = self.score_span(&words, start, end, &prompt_ids)?;
            if score >= self.threshold {
                candidates.push(
                    Candidate::new(
                        span,
                        self.class.clone(),
                        ID,
                        score,
                        90,
                        None,
                        self.token_family(),
                        ID,
                        ConflictTier::None,
                        Vec::new(),
                    )
                    .with_recognizer_version_id(format!("{ID}@{GLINER_DOB_HF_COMMIT}")),
                );
            }
        }
        Ok(candidates)
    }
}

fn date_boundary_is_valid(input: &str, span: &Range<usize>) -> bool {
    let before = input[..span.start].chars().next_back();
    let after = input[span.end..].chars().next();
    if before.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.'))
        || after.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/'))
    {
        return false;
    }
    if after == Some('.') {
        return !input[span.end + 1..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/'));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_bundle_digest_matches_checksum_manifest() {
        assert_eq!(
            crate::bundle::hex_sha256(GLINER_DOB_SHA256SUMS.as_bytes()),
            GLINER_DOB_BUNDLE_SHA256
        );
    }

    #[test]
    fn date_scanner_covers_common_en_de_fr_shapes_without_extensions() {
        let scanner = Regex::new(DATE_PATTERN).unwrap();
        for text in [
            "12.03.1987",
            "14/08/1993",
            "1990-12-05",
            "3 June 1990",
            "14. März 1987",
            "1er mars 1984",
        ] {
            let found = scanner.find(text).expect(text);
            assert_eq!(found.as_str(), text);
        }
        for text in ["1990-12-05-04", "12.03.1987x", "version_19840312"] {
            assert!(scanner
                .find(text)
                .is_none_or(|found| !date_boundary_is_valid(text, &found.range())));
        }
    }
}
