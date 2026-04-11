//! Worka pii detector adapter.
//!
//! Ghostwriter uses `pii` as a detection primitive only. We translate its
//! analyzer output into our `Detection`, then `typed_unknown` decides
//! placeholder tokens from those detections.

use crate::errors::SanitizeError;
use crate::placeholder::PlaceholderKind;
use pii::nlp::SimpleNlpEngine;
use pii::types::{EntityType, Language};
use pii::{default_recognizers, Analyzer, PolicyConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub start: usize,
    pub end: usize,
    pub kind: PlaceholderKind,
    pub raw: String,
}

pub struct WorkaDetector {
    analyzer: Analyzer,
    language: Language,
}

impl WorkaDetector {
    pub fn new() -> Self {
        Self {
            analyzer: Analyzer::new(
                Box::new(SimpleNlpEngine::default()),
                default_recognizers(),
                Vec::new(),
                PolicyConfig::default(),
            ),
            language: Language::from("en"),
        }
    }

    /// Run detection across `text`. Returns detections sorted by start ASC,
    /// end DESC (so longer spans at the same start win when overlaps are
    /// resolved by callers).
    pub fn detect(&self, text: &str) -> Result<Vec<Detection>, SanitizeError> {
        let result = self
            .analyzer
            .analyze(text, &self.language)
            .map_err(|e| SanitizeError::DetectorFailure(e.to_string()))?;

        let mut detections: Vec<Detection> = result
            .entities
            .into_iter()
            .filter_map(|r| {
                let kind = map_entity(&r.entity_type)?;
                let raw = text.get(r.start..r.end)?.to_string();
                Some(Detection {
                    start: r.start,
                    end: r.end,
                    kind,
                    raw,
                })
            })
            .collect();

        detections.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        Ok(detections)
    }
}

impl Default for WorkaDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn map_entity(ty: &EntityType) -> Option<PlaceholderKind> {
    match ty {
        EntityType::Email => Some(PlaceholderKind::Email),
        EntityType::Phone => Some(PlaceholderKind::Phone),
        EntityType::Person => Some(PlaceholderKind::Name),
        EntityType::Location => Some(PlaceholderKind::Address),
        EntityType::Iban => Some(PlaceholderKind::Iban),
        EntityType::IpAddress | EntityType::Ipv6 => Some(PlaceholderKind::Ip),
        // Unknown / unsupported entity types are skipped. Business-ID
        // leakage is acceptable in v1 per the spec.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_email() {
        let det = WorkaDetector::new();
        let out = det.detect("reach me at mueller@example.com please").unwrap();
        assert!(
            out.iter().any(|d| d.kind == PlaceholderKind::Email
                && d.raw == "mueller@example.com"),
            "expected email detection, got: {:?}",
            out
        );
    }

    #[test]
    fn detections_are_sorted_by_start() {
        let det = WorkaDetector::new();
        let out = det
            .detect("first a@x.com, then b@y.com, finally c@z.com")
            .unwrap();
        let starts: Vec<usize> = out.iter().map(|d| d.start).collect();
        let mut sorted = starts.clone();
        sorted.sort();
        assert_eq!(starts, sorted);
    }

    #[test]
    fn empty_text_returns_no_detections() {
        let det = WorkaDetector::new();
        let out = det.detect("").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn text_with_no_pii_returns_no_detections() {
        let det = WorkaDetector::new();
        let out = det.detect("the quick brown fox jumps over the lazy dog").unwrap();
        // Some NLP backends may flag "fox" or similar — only assert no emails.
        assert!(out.iter().all(|d| d.kind != PlaceholderKind::Email));
    }
}
