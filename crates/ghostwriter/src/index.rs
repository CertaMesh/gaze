use gaze::{Detection, Detector, PiiClass};

use crate::types::Context;

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexEntry {
    raw: String,
    class: PiiClass,
}

pub struct IndexDetector {
    entries: Vec<IndexEntry>,
}

impl IndexDetector {
    pub fn new(context: &Context) -> Self {
        let mut entries = Vec::new();
        entries.extend(
            context
                .order_ids
                .iter()
                .filter(|value| !value.is_empty())
                .map(|value| IndexEntry {
                    raw: value.clone(),
                    class: PiiClass::custom("order_id"),
                }),
        );
        entries.extend(
            context
                .songs
                .iter()
                .filter(|value| !value.is_empty())
                .map(|value| IndexEntry {
                    raw: value.clone(),
                    class: PiiClass::custom("song"),
                }),
        );
        entries.extend(
            context
                .artists
                .iter()
                .filter(|value| !value.is_empty())
                .map(|value| IndexEntry {
                    raw: value.clone(),
                    class: PiiClass::custom("artist"),
                }),
        );
        Self { entries }
    }
}

impl Detector for IndexDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let mut detections = Vec::new();
        for entry in &self.entries {
            detections.extend(find_all(input, &entry.raw).into_iter().map(|span| Detection {
                span,
                class: entry.class.clone(),
                source: "ghostwriter_index".to_string(),
            }));
        }
        detections.sort_by(|left, right| {
            left.span
                .start
                .cmp(&right.span.start)
                .then(right.span.end.cmp(&left.span.end))
        });
        detections
    }
}

fn find_all(input: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut offset = 0;
    while let Some(found) = input[offset..].find(needle) {
        let start = offset + found;
        let end = start + needle.len();
        spans.push(start..end);
        offset = end;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_indexed_terms() {
        let detector = IndexDetector::new(&Context {
            order_ids: vec!["SO-12345".into()],
            songs: vec!["Midnight City".into()],
            artists: vec!["M83".into()],
            ..Context::default()
        });

        let detections = detector.detect("SO-12345 includes Midnight City by M83");
        assert_eq!(detections.len(), 3);
        assert_eq!(detections[0].class, PiiClass::custom("order_id"));
        assert_eq!(detections[1].class, PiiClass::custom("song"));
        assert_eq!(detections[2].class, PiiClass::custom("artist"));
    }
}
