use std::sync::OnceLock;

use regex::Regex;

use crate::detector::BUILTIN_CLASS_NAMES;

pub fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(&build_pattern()).expect("token shape regex must compile"))
}

pub fn contains_token(s: &str) -> bool {
    pattern().is_match(s)
}

fn build_pattern() -> String {
    let builtin_alt = BUILTIN_CLASS_NAMES.join("|");
    let builtin_lower_alt = BUILTIN_CLASS_NAMES
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("|");

    format!(
        r"<(?:{builtin_alt})_\d+>|<Custom:[a-z0-9_]*_\d+>|\b(?:{builtin_lower_alt})_\d+\b|\bcustom:[a-z0-9_]*_\d+\b|\bemail\d+@example\.test\b|<[A-Z][a-zA-Z]+_\d+>|<[a-z][a-z_]+_\d+>|\b[A-Z][a-zA-Z]+_\d+\b|\b[a-z][a-z_]+_\d+\b",
        builtin_alt = builtin_alt,
        builtin_lower_alt = builtin_lower_alt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::PiiClass;
    use crate::session::{Scope, Session};

    fn raw_for(class: &PiiClass) -> &'static str {
        match class {
            PiiClass::Email => "alice@example.com",
            PiiClass::Name => "Alice Smith",
            PiiClass::Location => "Dublin",
            PiiClass::Organization => "Acme Inc",
            PiiClass::Custom(_) => "42",
        }
    }

    fn tokenized_for(class: PiiClass) -> String {
        let session = Session::new(Scope::Ephemeral).expect("session");
        session
            .tokenize(&class, raw_for(&class))
            .expect("tokenized placeholder")
    }

    fn format_preserving_for(class: PiiClass) -> String {
        let session = Session::new(Scope::Ephemeral).expect("session");
        session
            .format_preserving_fake(&class, raw_for(&class))
            .expect("format-preserving placeholder")
    }

    #[test]
    fn pattern_is_stable_across_calls() {
        assert!(std::ptr::eq(pattern(), pattern()));
    }

    #[test]
    fn every_emitted_token_matches_shape_regex() {
        for class in [
            PiiClass::Email,
            PiiClass::Name,
            PiiClass::Location,
            PiiClass::Organization,
            PiiClass::custom("order_id"),
        ] {
            assert!(contains_token(&tokenized_for(class.clone())));
            assert!(contains_token(&format_preserving_for(class)));
        }
    }

    #[test]
    fn builtin_class_names_match_impl() {
        let classes = [
            PiiClass::Email,
            PiiClass::Name,
            PiiClass::Location,
            PiiClass::Organization,
        ];

        for (class, expected) in classes.into_iter().zip(BUILTIN_CLASS_NAMES.iter()) {
            assert_eq!(class.class_name(), *expected);
        }
    }

    #[test]
    fn custom_and_builtin_do_not_collide() {
        let builtin = tokenized_for(PiiClass::Email);
        let custom = tokenized_for(PiiClass::custom("email"));

        assert_eq!(builtin, "<Email_1>");
        assert_eq!(custom, "<Custom:email_1>");
        assert_ne!(builtin, custom);
        assert!(contains_token(&builtin));
        assert!(contains_token(&custom));
    }

    #[test]
    fn empty_normalized_name_matches_current_shape() {
        let token = tokenized_for(PiiClass::custom("!!!"));
        assert_eq!(token, "<Custom:_1>");
        assert!(contains_token(&token));
    }

    #[test]
    fn single_char_custom_name_matches_current_shape() {
        let token = tokenized_for(PiiClass::custom("x"));
        assert_eq!(token, "<Custom:x_1>");
        assert!(contains_token(&token));
    }

    #[test]
    fn custom_token_matches_as_single_span() {
        let haystack = "before <Custom:order_id_1> after";
        let matched = pattern().find(haystack).expect("custom token match");
        assert_eq!(matched.as_str(), "<Custom:order_id_1>");
    }

    #[test]
    fn contains_bare_shapes_in_prose() {
        assert!(contains_token("See <Email_1>."));
        assert!(contains_token("See <Custom:order_id_1>."));
        assert!(contains_token("Reply to name_1."));
        assert!(contains_token("Email email1@example.test later."));
    }

    #[test]
    fn rejects_non_tokens() {
        assert!(!contains_token("See <Email_1bar>."));
        assert!(!contains_token("literal email@example.com address"));
        assert!(!contains_token("<Custom:-_1>"));
    }

    #[test]
    fn wrapped_tokens_match_across_text_contexts() {
        assert!(contains_token("See <Email_1>."));
        assert!(contains_token("Plain <Email_1> token"));
        assert!(contains_token("<<Email_1>>"));
    }

    #[test]
    fn restore_wrapped_token_in_prose() {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let first = session
            .tokenize(&PiiClass::Email, "alice@example.com")
            .expect("first token");
        let second = session
            .tokenize(&PiiClass::Email, "bob@example.com")
            .expect("second token");

        let rendered = format!("See {first}. Reply {second}");
        let restored = pattern().replace_all(&rendered, |captures: &regex::Captures<'_>| {
            session.restore_strict(&captures[0]).expect("known token")
        });

        assert_eq!(restored, "See alice@example.com. Reply bob@example.com");
    }
}
