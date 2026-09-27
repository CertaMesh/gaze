//! Opt-in, local GLiNER judge for dates the rule floor has not identified as birth dates.
//! Prompt tokenization, word masks, and span tensors are adapted from gline-rs
//! by Frédérik Bilhaut (Apache-2.0), commit f1f8923a7af972855909302a843063d958f69d95.

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
const LABELS: [&str; 3] = ["date of birth", "date", "event date"];
const DOB_SCORE_MARGIN: f32 = 0.65;
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
      |jan|feb|mar|apr|jun|jul|aug|sep|sept|oct|nov|dec|mär|mrz|okt|dez|févr|déc)\.?,?
    [\x20\t\x{A0}\x{202F}]+(?:de[\x20\t\x{A0}\x{202F}]+)?(?:19|20)[0-9]{2}
  | (?:january|february|march|april|may|june|july|august|september|october|november|december
      |jan|feb|mar|apr|jun|jul|aug|sep|sept|oct|nov|dec)\.?
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
    #[error("GLiNER DOB threshold must be greater than zero and less than one")]
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

struct ModelWindow {
    word_range: Range<usize>,
    logits: Option<Vec<f32>>,
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
        if !threshold.is_finite() || threshold <= 0.0 || threshold >= 1.0 {
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
        for part in LABELS
            .iter()
            .flat_map(|label| ["<<ENT>>", *label])
            .chain(["<<SEP>>"])
        {
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

    fn birth_date_score(scores: [f32; 3], threshold: f32) -> Option<f32> {
        let [birth, generic, event] = scores;
        (birth >= threshold && birth - generic.max(event) >= DOB_SCORE_MARGIN).then_some(birth)
    }

    fn word_bounds(words: &[Word], span: &Range<usize>) -> Result<(usize, usize), DetectError> {
        let start = words
            .iter()
            .position(|word| word.span.start == span.start)
            .ok_or_else(|| DetectError::backend(ID, "date start is not word-aligned"))?;
        let end = words
            .iter()
            .position(|word| word.span.end == span.end)
            .map(|index| index + 1)
            .ok_or_else(|| DetectError::backend(ID, "date end is not word-aligned"))?;
        Ok((start, end))
    }

    fn tile_words(words: &[Word], prompt_ids: &[i64]) -> Result<Vec<ModelWindow>, DetectError> {
        let budget = MAX_SUBTOKENS
            .checked_sub(prompt_ids.len() + 1)
            .filter(|budget| *budget > 0)
            .ok_or_else(|| DetectError::backend(ID, "prompt exceeds model token budget"))?;
        let overlap_tokens = budget / 4;
        let mut windows = Vec::new();
        let mut start = 0;
        while start < words.len() {
            // An unrelated oversized word cannot prevent later dates from being judged.
            if words[start].ids.len() > budget {
                start += 1;
                continue;
            }
            let mut end = start;
            let mut used = 0;
            while end < words.len() && words[end].ids.len() <= budget - used {
                used += words[end].ids.len();
                end += 1;
            }
            windows.push(ModelWindow {
                word_range: start..end,
                logits: None,
            });
            if end == words.len() || words[end].ids.len() > budget {
                start = end;
                continue;
            }
            let mut next_start = end;
            let mut overlap_used = 0;
            while next_start > start + 1 {
                let size = words[next_start - 1].ids.len();
                if size > budget - overlap_used
                    || (overlap_used >= overlap_tokens && end - next_start >= MAX_WIDTH - 1)
                {
                    break;
                }
                next_start -= 1;
                overlap_used += size;
            }
            start = next_start;
        }
        Ok(windows)
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
        windows: &mut [ModelWindow],
    ) -> Result<[f32; 3], DetectError> {
        if words.is_empty() || start >= end || end > words.len() || end - start > MAX_WIDTH {
            return Err(DetectError::backend(ID, "date span exceeds model width"));
        }
        let index = windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.word_range.start <= start && end <= window.word_range.end)
            .max_by_key(|(_, window)| {
                (start - window.word_range.start).min(window.word_range.end - end)
            })
            .map(|(index, _)| index)
            .ok_or_else(|| DetectError::backend(ID, "date span exceeds tiled model window"))?;
        let window = &mut windows[index];
        if window.logits.is_none() {
            window.logits = Some(self.infer_window(&words[window.word_range.clone()], prompt_ids)?);
        }
        let logits = window
            .logits
            .as_ref()
            .ok_or_else(|| DetectError::backend(ID, "window logits unavailable"))?;
        let relative_start = start - window.word_range.start;
        let width = end - start - 1;
        let offset = (relative_start * MAX_WIDTH + width) * LABELS.len();
        let selected = logits
            .get(offset..offset + LABELS.len())
            .ok_or_else(|| DetectError::backend(ID, "date logits unavailable"))?;
        if selected.iter().any(|logit| !logit.is_finite()) {
            return Err(DetectError::backend(ID, "non-finite logits"));
        }
        let selected: [f32; 3] = selected
            .try_into()
            .map_err(|_| DetectError::backend(ID, "invalid logits width"))?;
        Ok(selected.map(|logit| 1.0 / (1.0 + (-logit).exp())))
    }

    fn infer_window(&self, window: &[Word], prompt_ids: &[i64]) -> Result<Vec<f32>, DetectError> {
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
            || shape[3] != LABELS.len() as i64
        {
            return Err(DetectError::backend(ID, "invalid logits shape"));
        }
        Ok(logits.to_vec())
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
        let mut windows = Self::tile_words(&words, &prompt_ids)?;
        let mut candidates = Vec::new();
        for span in dates {
            let (start, end) = Self::word_bounds(&words, &span)?;
            let scores = self.score_span(&words, start, end, &prompt_ids, &mut windows)?;
            if let Some(score) = Self::birth_date_score(scores, self.threshold) {
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
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn pinned_bundle_digest_matches_checksum_manifest() {
        assert_eq!(
            crate::bundle::hex_sha256(GLINER_DOB_SHA256SUMS.as_bytes()),
            GLINER_DOB_BUNDLE_SHA256
        );
    }

    #[test]
    fn dob_threshold_requires_a_model_yes() {
        for threshold in [0.0, 1.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                DobJudgeRecognizer::load(Path::new("/missing"), threshold),
                Err(DobJudgeLoadError::Threshold)
            ));
        }
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
            "2 Sept 2015",
            "2 sept. 2024",
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

    #[test]
    fn fixed_tiles_cover_date_spans_across_boundaries() {
        let prompt = vec![1; 20];
        let budget = MAX_SUBTOKENS - prompt.len() - 1;
        let words: Vec<_> = (0..160)
            .map(|index| Word {
                span: index..index + 1,
                ids: vec![1; 10],
            })
            .collect();
        let windows = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
        assert!(windows.len() < 10);
        for window in &windows {
            let subtokens: usize = words[window.word_range.clone()]
                .iter()
                .map(|word| word.ids.len())
                .sum();
            assert!(subtokens <= budget);
        }
        for start in 0..=words.len() - MAX_WIDTH {
            assert!(windows.iter().any(|window| {
                window.word_range.start <= start && start + MAX_WIDTH <= window.word_range.end
            }));
        }
    }

    /// Run: `GAZE_GLINER_DOB_TEST_BUNDLE="$HOME/.local/share/gaze/models/gliner-multi-pii-dob-int8" cargo test -p gaze-recognizers --lib dob_judge::tests::live_model_separates_birth_dates_from_business_dates_without_keyword_filter -- --ignored --exact`.
    #[test]
    #[ignore = "requires the locally installed, pinned GLiNER ONNX bundle"]
    fn live_model_separates_birth_dates_from_business_dates_without_keyword_filter() {
        let bundle = std::env::var_os("GAZE_GLINER_DOB_TEST_BUNDLE")
            .expect("set GAZE_GLINER_DOB_TEST_BUNDLE to the pinned bundle directory");
        let judge = DobJudgeRecognizer::load(&PathBuf::from(bundle), 0.5).unwrap();
        let prompt = judge.prompt_ids().unwrap();
        let score_dates = |input: &str| {
            let words = judge.encode_words(input).unwrap();
            let mut windows = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
            judge
                .date_regex
                .find_iter(input)
                .filter(|found| date_boundary_is_valid(input, &found.range()))
                .map(|found| {
                    let (start, end) =
                        DobJudgeRecognizer::word_bounds(&words, &found.range()).unwrap();
                    let scores = judge
                        .score_span(&words, start, end, &prompt, &mut windows)
                        .unwrap();
                    DobJudgeRecognizer::birth_date_score(scores, 0.5).is_some()
                })
                .collect::<Vec<_>>()
        };
        let positives = [
            "Helena (14.03.1987) is listed in the patient file.",
            "Anna Weber, 03/07/1992, attended the appointment.",
            "Mr. John Smith (born in Leeds) - 12 June 1985 - joined the team.",
            "Participant: Maria Lopez, June 3, 1990, female.",
            "Herr Müller, 14. März 1987, wohnhaft in Berlin.",
            "Frau Schulz (geb. Meier), 02.11.1979, Stuttgart.",
            "Madame Dupont, 1er mars 1984, domiciliée à Lyon.",
            "Jean Martin, né le 12 mars 1984 à Paris.",
            "Name: Peter Brown\n1985-06-12\nAddress: 1 High Street",
        ];
        let negatives = [
            "Contract signed 14.03.1987 between the parties.",
            "Delivery scheduled for 03/07/1992 to the warehouse.",
            "The conference took place on 12 June 1985 in Berlin.",
            "Event: Summer party, June 3, 1990, main hall.",
            "Vertrag vom 14. März 1987, Laufzeit zwei Jahre.",
            "Lieferung am 02.11.1979 an das Lager.",
            "Réunion du 1er mars 1984 au siège.",
            "Contrat signé le 12 mars 1984 à Paris.",
            "Order #4411, 1985-06-12, qty 3, total 40 EUR.",
            "Customer since 03/07/1992, tier gold.",
            "Anna Weber, 03/07/1992, placed an order for 3 chairs.",
            "Helena Schmidt (14.03.1987 - 20.05.1990) served as CFO.",
            "Build 20240115 passed on CI.",
        ];
        let positive_scores: Vec<bool> = positives
            .iter()
            .flat_map(|text| score_dates(text))
            .collect();
        let negative_scores: Vec<bool> = negatives
            .iter()
            .flat_map(|text| score_dates(text))
            .collect();
        assert_eq!(positive_scores.len(), 9);
        assert_eq!(negative_scores.len(), 14);
        assert!(positive_scores.iter().filter(|&&emitted| emitted).count() >= 8);
        assert!(negative_scores.iter().filter(|&&emitted| emitted).count() <= 2);

        let filtered = "Invoice issued on 1990-12-05.";
        assert!(judge.proposed_dates(filtered, &[]).is_empty());
        assert_eq!(score_dates(filtered), vec![false]);

        let table_row = negatives[11];
        let words = judge.encode_words(table_row).unwrap();
        let spans: Vec<_> = judge
            .date_regex
            .find_iter(table_row)
            .map(|found| DobJudgeRecognizer::word_bounds(&words, &found.range()).unwrap())
            .collect();
        let baseline: Vec<_> = spans
            .iter()
            .map(|&(start, end)| {
                let mut fresh = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
                judge
                    .score_span(&words, start, end, &prompt, &mut fresh)
                    .unwrap()
                    .map(f32::to_bits)
            })
            .collect();
        let mut shared = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
        let cached: Vec<_> = spans
            .iter()
            .map(|&(start, end)| {
                judge
                    .score_span(&words, start, end, &prompt, &mut shared)
                    .unwrap()
                    .map(f32::to_bits)
            })
            .collect();
        assert_eq!(baseline, cached);
        assert_eq!(
            shared
                .iter()
                .filter(|window| window.logits.is_some())
                .count(),
            1
        );
    }

    /// Run: `GAZE_GLINER_DOB_TEST_BUNDLE="$HOME/.local/share/gaze/models/gliner-multi-pii-dob-int8" cargo test --release -p gaze-recognizers --lib dob_judge::tests::live_forty_row_table_reuses_tiled_windows -- --ignored --exact --nocapture`.
    #[test]
    #[ignore = "requires the locally installed, pinned GLiNER ONNX bundle"]
    fn live_forty_row_table_reuses_tiled_windows() {
        let bundle = std::env::var_os("GAZE_GLINER_DOB_TEST_BUNDLE")
            .expect("set GAZE_GLINER_DOB_TEST_BUNDLE to the pinned bundle directory");
        let judge = DobJudgeRecognizer::load(&PathBuf::from(bundle), 0.5).unwrap();
        let table = (0..40)
            .map(|index| format!(
                "| Kunde {index}: Maximilian Hoffmann-Schneider | 14.03.1987 | Musterstraße {index}, 10115 Berlin | Kundennummer 8841{index} |\n"
            ))
            .collect::<String>();
        let words = judge.encode_words(&table).unwrap();
        let prompt = judge.prompt_ids().unwrap();
        let budget = MAX_SUBTOKENS - prompt.len() - 1;
        let subtokens: usize = words.iter().map(|word| word.ids.len()).sum();
        assert!(subtokens > budget);
        let spans: Vec<_> = judge
            .date_regex
            .find_iter(&table)
            .map(|found| DobJudgeRecognizer::word_bounds(&words, &found.range()).unwrap())
            .collect();
        assert_eq!(spans.len(), 40);

        let mut baseline = Vec::new();
        let started = Instant::now();
        for &(start, end) in &spans {
            let mut fresh = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
            baseline.push(
                judge
                    .score_span(&words, start, end, &prompt, &mut fresh)
                    .unwrap()
                    .map(f32::to_bits),
            );
        }
        let repeated_time = started.elapsed();
        let mut shared = DobJudgeRecognizer::tile_words(&words, &prompt).unwrap();
        let started = Instant::now();
        let cached: Vec<_> = spans
            .iter()
            .map(|&(start, end)| {
                judge
                    .score_span(&words, start, end, &prompt, &mut shared)
                    .unwrap()
                    .map(f32::to_bits)
            })
            .collect();
        let cached_time = started.elapsed();
        assert_eq!(baseline, cached);
        let inferred_windows = shared
            .iter()
            .filter(|window| window.logits.is_some())
            .count();
        assert!(
            inferred_windows < 10,
            "expected fixed tiling to reduce 40 inferences"
        );

        let dictionaries = gaze_types::DictionaryBundle::default();
        let prior = [];
        let ctx = DetectContext::new(&[], &dictionaries).with_prior_candidates(&prior);
        let started = Instant::now();
        judge.detect(&table, &ctx).unwrap();
        let detect_time = started.elapsed();
        eprintln!(
            "40-row subtokens={subtokens} tiles={} inferences={inferred_windows} repeated={repeated_time:?} cached={cached_time:?} detect={detect_time:?}",
            shared.len()
        );
    }
}
