use gaze::{Detection, Detector, PiiClass};

use crate::types::Context;

pub const CUSTOMER_NAME: &str = "<CUSTOMER_NAME>";
pub const CUSTOMER_EMAIL: &str = "<CUSTOMER_EMAIL>";
pub const CUSTOMER_PHONE: &str = "<CUSTOMER_PHONE>";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextEntry {
    raw: String,
    class: PiiClass,
}

pub struct ContextDetector {
    entries: Vec<ContextEntry>,
}

impl ContextDetector {
    pub fn new(context: &Context) -> Self {
        let mut entries = Vec::new();
        if let Some(value) = context.customer_name.as_deref().filter(|value| !value.is_empty()) {
            entries.push(ContextEntry {
                raw: value.to_string(),
                class: PiiClass::custom("customer_name"),
            });
        }
        if let Some(value) = context.customer_email.as_deref().filter(|value| !value.is_empty()) {
            entries.push(ContextEntry {
                raw: value.to_string(),
                class: PiiClass::custom("customer_email"),
            });
        }
        if let Some(value) = context.customer_phone.as_deref().filter(|value| !value.is_empty()) {
            entries.push(ContextEntry {
                raw: value.to_string(),
                class: PiiClass::custom("customer_phone"),
            });
        }
        Self { entries }
    }
}

impl Detector for ContextDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let mut detections = Vec::new();
        for entry in &self.entries {
            detections.extend(find_all(input, &entry.raw).into_iter().map(|span| Detection {
                span,
                class: entry.class.clone(),
                source: "ghostwriter_context".to_string(),
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

    fn context() -> Context {
        Context {
            customer_name: Some("Markus Mueller".into()),
            customer_email: Some("mueller.markus@icloud.com".into()),
            customer_phone: Some("+49 151 23456789".into()),
            ..Context::default()
        }
    }

    #[test]
    fn detects_known_context_matches() {
        let detector = ContextDetector::new(&context());
        let detections = detector.detect(
            "Markus Mueller wrote from mueller.markus@icloud.com and called +49 151 23456789",
        );

        assert_eq!(detections.len(), 3);
        assert_eq!(detections[0].class, PiiClass::custom("customer_name"));
        assert_eq!(detections[1].class, PiiClass::custom("customer_email"));
        assert_eq!(detections[2].class, PiiClass::custom("customer_phone"));
    }
}
