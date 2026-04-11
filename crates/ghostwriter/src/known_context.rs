//! Stage 1: known context replacement.
//!
//! Replace exact string matches of customer_name / customer_email /
//! customer_phone with semantic placeholders BEFORE generic detection
//! runs. Multi-occurrence matches are all replaced.

use crate::placeholder::PlaceholderMap;
use crate::types::Context;

pub const CUSTOMER_NAME: &str = "<CUSTOMER_NAME>";
pub const CUSTOMER_EMAIL: &str = "<CUSTOMER_EMAIL>";
pub const CUSTOMER_PHONE: &str = "<CUSTOMER_PHONE>";

/// Replace known customer identity in `text` using exact string match.
/// Mutates `map` with the inserted known placeholders.
pub fn apply(text: &str, context: &Context, map: &mut PlaceholderMap) -> String {
    let mut out = text.to_string();
    if let Some(name) = context.customer_name.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(name) {
            out = out.replace(name, CUSTOMER_NAME);
            map.insert_known(CUSTOMER_NAME, name);
        }
    }
    if let Some(email) = context.customer_email.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(email) {
            out = out.replace(email, CUSTOMER_EMAIL);
            map.insert_known(CUSTOMER_EMAIL, email);
        }
    }
    if let Some(phone) = context.customer_phone.as_deref().filter(|s| !s.is_empty()) {
        if out.contains(phone) {
            out = out.replace(phone, CUSTOMER_PHONE);
            map.insert_known(CUSTOMER_PHONE, phone);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(n: Option<&str>, e: Option<&str>, p: Option<&str>) -> Context {
        Context {
            customer_name: n.map(String::from),
            customer_email: e.map(String::from),
            customer_phone: p.map(String::from),
        }
    }

    #[test]
    fn replaces_all_occurrences_of_customer_name() {
        let text = "Markus Mueller wrote to Markus Mueller";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &ctx(Some("Markus Mueller"), None, None), &mut map);
        assert_eq!(out, "<CUSTOMER_NAME> wrote to <CUSTOMER_NAME>");
        assert_eq!(map.entries().len(), 1);
    }

    #[test]
    fn replaces_known_email_before_known_phone() {
        let text = "email m@x.com, call +49 151 1";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(None, Some("m@x.com"), Some("+49 151 1")),
            &mut map,
        );
        assert_eq!(out, "email <CUSTOMER_EMAIL>, call <CUSTOMER_PHONE>");
    }

    #[test]
    fn missing_context_fields_are_ignored() {
        let text = "Hi there";
        let mut map = PlaceholderMap::new();
        let out = apply(text, &ctx(None, None, None), &mut map);
        assert_eq!(out, "Hi there");
        assert_eq!(map.entries().len(), 0);
    }

    #[test]
    fn absent_match_does_not_insert_placeholder() {
        let text = "Hi there";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(Some("Markus Mueller"), None, None),
            &mut map,
        );
        assert_eq!(out, "Hi there");
        assert_eq!(map.entries().len(), 0);
    }

    #[test]
    fn spec_example_preserves_alternate_email_for_stage_two() {
        // From the spec example: alternate email stays raw after stage 1,
        // stage 2 will later tokenize it.
        let text = "Can you send it to markus.mueller@example.de instead of mueller.markus@icloud.com? Thanks, Markus Mueller";
        let mut map = PlaceholderMap::new();
        let out = apply(
            text,
            &ctx(
                Some("Markus Mueller"),
                Some("mueller.markus@icloud.com"),
                None,
            ),
            &mut map,
        );
        assert_eq!(
            out,
            "Can you send it to markus.mueller@example.de instead of <CUSTOMER_EMAIL>? Thanks, <CUSTOMER_NAME>"
        );
    }
}
