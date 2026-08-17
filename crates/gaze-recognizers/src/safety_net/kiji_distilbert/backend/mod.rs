use std::cmp::Ordering;

use gaze_types::SafetyNetError;

pub mod artifacts;
#[cfg(feature = "runtime-candle")]
pub mod candle;
pub(crate) mod decode;
pub mod ort;
pub mod subprocess;
#[cfg(feature = "runtime-tract")]
pub mod tract;

/// Precision variant for the pinned Kiji DistilBERT ONNX artifact.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum KijiDistilbertPrecision {
    Fp32,
    Int8,
}

impl KijiDistilbertPrecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fp32 => "fp32",
            Self::Int8 => "int8",
        }
    }
}

/// Internal Kiji span after PII-bearing upstream fields have been dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSpan {
    /// Byte start in the clean text checked by the safety net.
    pub start: usize,
    /// Byte end in the clean text checked by the safety net.
    pub end: usize,
    /// Shape-validated raw Kiji label.
    pub label: String,
    /// Optional backend confidence score.
    pub score: Option<f32>,
}

impl RawSpan {
    pub fn new(start: usize, end: usize, label: impl Into<String>, score: Option<f32>) -> Self {
        Self {
            start,
            end,
            label: label.into(),
            score,
        }
    }
}

/// Backend abstraction for local Kiji DistilBERT NER inference.
pub trait KijiDistilbertBackend: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn decoding_params(&self) -> &[(&str, String)];
    fn infer(&self, clean: &str) -> Result<Vec<RawSpan>, SafetyNetError>;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum KijiBackendKind {
    Subprocess,
    Ort,
    #[cfg(feature = "runtime-tract")]
    Tract,
    #[cfg(feature = "runtime-candle")]
    Candle,
}

impl KijiBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subprocess => "subprocess",
            Self::Ort => "ort",
            #[cfg(feature = "runtime-tract")]
            Self::Tract => "tract",
            #[cfg(feature = "runtime-candle")]
            Self::Candle => "candle",
        }
    }
}

pub(crate) fn normalize_raw_spans(
    mut spans: Vec<RawSpan>,
    clean: &str,
) -> Result<Vec<RawSpan>, SafetyNetError> {
    for span in &spans {
        if span.start >= span.end
            || span.end > clean.len()
            || !clean.is_char_boundary(span.start)
            || !clean.is_char_boundary(span.end)
        {
            return Err(SafetyNetError::InvalidOutput {
                message: "kiji returned out-of-bounds span".to_string(),
            });
        }
    }

    spans.sort_by(|left, right| {
        (
            left.start,
            left.end,
            left.label.as_str(),
            ScoreSort(left.score),
        )
            .cmp(&(
                right.start,
                right.end,
                right.label.as_str(),
                ScoreSort(right.score),
            ))
    });

    for pair in spans.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(SafetyNetError::InvalidOutput {
                message: "kiji returned overlapping spans".to_string(),
            });
        }
    }

    Ok(spans)
}

#[derive(Clone, Copy, Debug)]
struct ScoreSort(Option<f32>);

impl PartialEq for ScoreSort {
    fn eq(&self, other: &Self) -> bool {
        score_bits(self.0) == score_bits(other.0)
    }
}

impl Eq for ScoreSort {}

impl PartialOrd for ScoreSort {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoreSort {
    fn cmp(&self, other: &Self) -> Ordering {
        score_bits(self.0).cmp(&score_bits(other.0))
    }
}

fn score_bits(score: Option<f32>) -> u32 {
    score.map(f32::to_bits).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_spans_deterministically() {
        let spans = vec![
            RawSpan::new(4, 7, "person", Some(0.4)),
            RawSpan::new(2, 4, "location", Some(0.9)),
            RawSpan::new(0, 2, "person", Some(0.1)),
        ];

        let normalized = normalize_raw_spans(spans, "abcdefg").unwrap();
        assert_eq!(normalized[0].score, Some(0.1));
        assert_eq!(normalized[1].score, Some(0.9));
        assert_eq!(normalized[2].label, "person");
    }

    #[test]
    fn rejects_out_of_bounds_and_overlaps() {
        assert!(normalize_raw_spans(vec![RawSpan::new(0, 9, "person", None)], "short").is_err());
        assert!(normalize_raw_spans(
            vec![
                RawSpan::new(0, 3, "person", None),
                RawSpan::new(2, 4, "location", None)
            ],
            "abcdef"
        )
        .is_err());
    }
}
