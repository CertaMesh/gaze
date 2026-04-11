#![allow(dead_code)]
//! Maps column names to PII classes. In v0.1 this is a lookup against
//! the policy file's column rules. For M1a we ship a hand-rolled fallback
//! so the anonymizer can be tested without a full policy loader.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiiClass {
    /// Not personal data — passes through untouched.
    NonPii,
    /// Opaque primary-key integer, replaced via HMAC mod 2^31.
    Id,
    /// Full name → `Person_N`.
    Name,
    /// Email → `user_N@example.com`.
    Email,
    /// Phone number → structurally-preserving fake.
    Phone,
    /// Freeform postal address → `Musterstrasse_N_00000_City`.
    Address,
    /// IBAN → structurally valid fake IBAN.
    Iban,
    /// IPv4 → `10.0.0.N`.
    Ip,
    /// Date / datetime — shifted by a per-session constant offset.
    Date,
    /// Any other text column flagged by policy or detector → `redacted_N`.
    GenericText,
}

#[derive(Debug, Clone, Default)]
pub struct Classifier {
    explicit: HashMap<String, PiiClass>,
}

impl Classifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_column<S: Into<String>>(mut self, column: S, class: PiiClass) -> Self {
        self.explicit.insert(column.into(), class);
        self
    }

    pub fn classify(&self, column: &str) -> PiiClass {
        self.explicit
            .get(column)
            .copied()
            .unwrap_or(PiiClass::NonPii)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_rule_wins() {
        let c = Classifier::new().with_column("email", PiiClass::Email);
        assert_eq!(c.classify("email"), PiiClass::Email);
    }

    #[test]
    fn unknown_column_defaults_to_non_pii() {
        let c = Classifier::new();
        assert_eq!(c.classify("created_at"), PiiClass::NonPii);
    }
}
