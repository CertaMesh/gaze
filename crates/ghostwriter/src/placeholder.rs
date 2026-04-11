//! Placeholder numbering + dedupe.
//!
//! `PlaceholderMap` owns the mapping from raw values to placeholder
//! tokens during a single sanitize call. It guarantees:
//!
//! - Same raw value within one call → same placeholder token.
//! - Numbering is per `PlaceholderKind` (EMAIL_1, EMAIL_2, PHONE_1, ...).
//! - Insertion order within a kind determines numbering.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceholderKind {
    Email,
    Phone,
    Name,
    Address,
    Iban,
    Ip,
    GenericPii,
}

impl PlaceholderKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Email => "EMAIL",
            Self::Phone => "PHONE",
            Self::Name => "NAME",
            Self::Address => "ADDRESS",
            Self::Iban => "IBAN",
            Self::Ip => "IP",
            Self::GenericPii => "PII",
        }
    }
}

#[derive(Debug, Default)]
pub struct PlaceholderMap {
    /// raw value → placeholder token (for dedupe).
    raw_to_token: HashMap<String, String>,
    /// token → raw value (for blob assembly and ordered listing).
    token_to_raw: Vec<(String, String)>,
    counters: HashMap<PlaceholderKind, u32>,
}

impl PlaceholderMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a KNOWN semantic placeholder (e.g. <CUSTOMER_NAME>).
    /// Does not consume a counter. If `token` already maps to a different
    /// raw value the original mapping wins — known context is inserted
    /// exactly once.
    pub fn insert_known(&mut self, token: &str, raw: &str) {
        if self.raw_to_token.contains_key(raw) {
            return;
        }
        self.raw_to_token.insert(raw.to_string(), token.to_string());
        self.token_to_raw.push((token.to_string(), raw.to_string()));
    }

    /// Lookup or allocate a typed placeholder for `raw` under `kind`.
    /// Same raw value returns the same token; a fresh raw value
    /// increments the per-kind counter.
    pub fn intern_typed(&mut self, kind: PlaceholderKind, raw: &str) -> String {
        if let Some(t) = self.raw_to_token.get(raw) {
            return t.clone();
        }
        let counter = self.counters.entry(kind).or_insert(0);
        *counter += 1;
        let token = format!("<{}_{}>", kind.prefix(), counter);
        self.raw_to_token.insert(raw.to_string(), token.clone());
        self.token_to_raw.push((token.clone(), raw.to_string()));
        token
    }

    pub fn token_for(&self, raw: &str) -> Option<&str> {
        self.raw_to_token.get(raw).map(String::as_str)
    }

    /// Ordered pairs (token, raw) in insertion order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.token_to_raw
    }

    pub fn token_list(&self) -> Vec<String> {
        self.token_to_raw.iter().map(|(t, _)| t.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_known_is_idempotent() {
        let mut m = PlaceholderMap::new();
        m.insert_known("<CUSTOMER_NAME>", "Markus Mueller");
        m.insert_known("<CUSTOMER_NAME>", "Markus Mueller");
        assert_eq!(m.entries().len(), 1);
    }

    #[test]
    fn intern_typed_allocates_sequential_numbering() {
        let mut m = PlaceholderMap::new();
        let a = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let b = m.intern_typed(PlaceholderKind::Email, "b@x.com");
        assert_eq!(a, "<EMAIL_1>");
        assert_eq!(b, "<EMAIL_2>");
    }

    #[test]
    fn intern_typed_dedupes_same_raw_value() {
        let mut m = PlaceholderMap::new();
        let a = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let a2 = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        assert_eq!(a, a2);
    }

    #[test]
    fn counters_are_per_kind() {
        let mut m = PlaceholderMap::new();
        let e1 = m.intern_typed(PlaceholderKind::Email, "a@x.com");
        let p1 = m.intern_typed(PlaceholderKind::Phone, "+49 151 1");
        assert_eq!(e1, "<EMAIL_1>");
        assert_eq!(p1, "<PHONE_1>");
    }

    #[test]
    fn known_insertion_prevents_later_typed_collision() {
        let mut m = PlaceholderMap::new();
        m.insert_known("<CUSTOMER_EMAIL>", "m@x.com");
        let t = m.intern_typed(PlaceholderKind::Email, "m@x.com");
        assert_eq!(t, "<CUSTOMER_EMAIL>");
    }
}
