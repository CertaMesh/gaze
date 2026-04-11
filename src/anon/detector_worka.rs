//! Worka-backed PII detector. Translates the `pii` crate's findings
//! into our `Detection` shape. For v0.1 we care about email, phone,
//! IBAN, IP; everything else maps to GenericText.

use crate::anon::detector::{Detection, PiiDetector};
use crate::policy::classifier::PiiClass;
use pii::nlp::SimpleNlpEngine;
use pii::types::{EntityType, Language};
use pii::{default_recognizers, Analyzer, PolicyConfig};

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
}

impl Default for WorkaDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl PiiDetector for WorkaDetector {
    fn detect(&self, text: &str) -> Vec<Detection> {
        let Ok(result) = self.analyzer.analyze(text, &self.language) else {
            return Vec::new();
        };
        result
            .entities
            .into_iter()
            .map(|d| Detection {
                class: map_entity(&d.entity_type),
                start: d.start,
                end: d.end,
            })
            .collect()
    }
}

fn map_entity(entity: &EntityType) -> PiiClass {
    match entity {
        EntityType::Email => PiiClass::Email,
        EntityType::Phone => PiiClass::Phone,
        EntityType::Iban => PiiClass::Iban,
        EntityType::IpAddress | EntityType::Ipv6 => PiiClass::Ip,
        EntityType::Person => PiiClass::Name,
        EntityType::Location => PiiClass::Address,
        _ => PiiClass::GenericText,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_embedded_email() {
        let d = WorkaDetector::new();
        let hits = d.detect("contact krishan@example.com for details");
        assert!(hits.iter().any(|h| h.class == PiiClass::Email));
    }

    #[test]
    fn finds_embedded_ip() {
        let d = WorkaDetector::new();
        let hits = d.detect("request from 192.168.1.50 failed");
        assert!(hits.iter().any(|h| h.class == PiiClass::Ip));
    }
}
