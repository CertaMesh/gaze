use std::cmp::Ordering;

use gaze_types::SafetyNetError;

pub mod subprocess;

/// Internal OPF span after PII-bearing upstream fields have been dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSpan {
    /// Byte start in the clean text checked by the safety net.
    pub start: usize,
    /// Byte end in the clean text checked by the safety net.
    pub end: usize,
    /// Shape-validated raw OPF label, never source text.
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

/// Backend abstraction for local OpenAI Privacy Filter inference.
pub trait OpenAiFilterBackend: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn decoding_params(&self) -> &[(&str, String)];
    fn infer(&self, clean: &str) -> Result<Vec<RawSpan>, SafetyNetError>;
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
                message: "opf returned out-of-bounds span".to_string(),
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
                message: "opf returned overlapping spans".to_string(),
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
            RawSpan::new(4, 7, "secret", Some(0.4)),
            RawSpan::new(2, 4, "private_email", Some(0.9)),
            RawSpan::new(0, 2, "private_email", Some(0.1)),
        ];

        let normalized = normalize_raw_spans(spans, "abcdefg").unwrap();
        assert_eq!(normalized[0].score, Some(0.1));
        assert_eq!(normalized[1].score, Some(0.9));
        assert_eq!(normalized[2].label, "secret");
    }

    #[test]
    fn rejects_out_of_bounds_and_overlaps() {
        assert!(
            normalize_raw_spans(vec![RawSpan::new(0, 9, "private_email", None)], "short").is_err()
        );
        assert!(normalize_raw_spans(
            vec![
                RawSpan::new(0, 3, "private_email", None),
                RawSpan::new(2, 4, "secret", None)
            ],
            "abcdef"
        )
        .is_err());
    }
}
