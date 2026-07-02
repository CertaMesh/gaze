use std::sync::OnceLock;

use regex::Regex;

use crate::detector::BUILTIN_CLASS_NAMES;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TokenShapeShadow {
    pub recognizer_id: String,
    pub offending_pattern: String,
    pub shadowed_shape: String,
}

pub fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(&build_pattern()).expect("token shape regex must compile"))
}

pub fn contains_token(s: &str) -> bool {
    pattern().is_match(s)
}

pub fn find_token(s: &str) -> Option<&str> {
    pattern().find(s).map(|m| m.as_str())
}

pub fn find_tokens(s: &str) -> impl Iterator<Item = &str> {
    pattern().find_iter(s).map(|m| m.as_str())
}

pub fn sample_token_shapes() -> &'static [&'static str] {
    &[
        "<deadbeef:Email_1>",
        "<deadbeef:Name_1>",
        "<deadbeef:Location_1>",
        "<deadbeef:Organization_1>",
        "<deadbeef:Custom:class_alpha_1>",
        "email1.deadbeef@gaze-fake.invalid",
        "deadbeef:email_1",
        "deadbeef:custom:class_alpha_1",
        "<Email_1>",
        "<email_1>",
        "<Custom:class_alpha_1>",
        "<custom:class_alpha_1>",
        "Email_1",
        "email_1",
        "custom:class_alpha_1",
        "email1@example.test",
        "email1@gaze-fake.invalid",
    ]
}

pub fn reject_if_shadows_token_shape(
    compiled: &Regex,
    recognizer_id: &str,
) -> Result<(), TokenShapeShadow> {
    for sample in sample_token_shapes() {
        if compiled.is_match(sample) {
            return Err(TokenShapeShadow {
                recognizer_id: recognizer_id.to_string(),
                offending_pattern: compiled.as_str().to_string(),
                shadowed_shape: (*sample).to_string(),
            });
        }
    }

    Ok(())
}

pub fn starts_with_session_prefix(s: &str) -> bool {
    let bytes = s.as_bytes();
    let is_lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);

    if bytes.len() >= 10
        && bytes[0] == b'<'
        && bytes[9] == b':'
        && bytes[1..9].iter().copied().all(is_lower_hex)
    {
        return true;
    }

    if bytes.len() >= 15 && bytes.starts_with(b"email") {
        let mut i = 5;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i > 5 && i + 9 < bytes.len() && bytes[i] == b'.' {
            let hex_start = i + 1;
            let hex_end = hex_start + 8;
            if hex_end < bytes.len()
                && bytes[hex_end] == b'@'
                && bytes[hex_start..hex_end].iter().copied().all(is_lower_hex)
            {
                return true;
            }
        }
    }

    bytes.len() >= 9 && bytes[8] == b':' && bytes[0..8].iter().copied().all(is_lower_hex)
}

pub fn is_trap(match_text: &str) -> bool {
    !starts_with_session_prefix(match_text)
}

fn build_pattern() -> String {
    let builtin_alt = BUILTIN_CLASS_NAMES.join("|");
    let builtin_lower_alt = BUILTIN_CLASS_NAMES
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("|");

    format!(
        r"<[0-9a-f]{{8}}:(?:{builtin_alt})_[0-9]+>|<[0-9a-f]{{8}}:Custom:[a-z0-9_]*_[0-9]+>|\bemail[0-9]+\.[0-9a-f]{{8}}@gaze-fake\.invalid\b|\b[0-9a-f]{{8}}:(?:{builtin_lower_alt})_[0-9]+\b|\b[0-9a-f]{{8}}:custom:[a-z0-9_]*_[0-9]+\b|<(?:{builtin_alt})_[0-9]+>|<Custom:[a-z0-9_]*_[0-9]+>|\b(?:{builtin_lower_alt})_[0-9]+\b|\bcustom:[a-z0-9_]*_[0-9]+\b|\bemail[0-9]+@example\.test\b|\bemail[0-9]+@gaze-fake\.invalid\b|<[A-Z][a-zA-Z0-9]+_[0-9]+>|<[a-z][a-zA-Z0-9_]*_[0-9]+>|\b[A-Z][a-zA-Z0-9]+_[0-9]+\b|\b[a-z][a-zA-Z0-9_]*_[0-9]+\b",
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
            PiiClass::Email => "alice@example.invalid",
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
        for class in PiiClass::builtin_variants()
            .iter()
            .cloned()
            .chain(std::iter::once(PiiClass::custom("class_alpha")))
        {
            assert!(contains_token(&tokenized_for(class.clone())));
            assert!(contains_token(&format_preserving_for(class)));
        }
    }

    #[test]
    fn every_sample_matches_pattern_regex() {
        for sample in sample_token_shapes() {
            assert!(
                contains_token(sample),
                "sample should match token regex: {sample}"
            );
        }
    }

    #[test]
    fn builtin_class_names_match_impl() {
        for (class, expected) in PiiClass::builtin_variants()
            .iter()
            .zip(BUILTIN_CLASS_NAMES.iter())
        {
            assert_eq!(class.class_name(), *expected);
        }
    }

    #[test]
    fn builtin_class_regex_superset() {
        for class in PiiClass::builtin_variants() {
            assert!(contains_token(&format!(
                "<a7f3b8e2:{}_1>",
                class.class_name()
            )));
            assert!(contains_token(&format!(
                "a7f3b8e2:{}_1",
                class.class_name().to_ascii_lowercase()
            )));
            assert!(contains_token(&format!("<{}_1>", class.class_name())));
        }
    }

    #[test]
    fn custom_and_builtin_do_not_collide() {
        let builtin = tokenized_for(PiiClass::Email);
        let custom = tokenized_for(PiiClass::custom("email"));

        assert!(builtin.ends_with(":Email_1>"));
        assert!(custom.ends_with(":Custom:email_1>"));
        assert_ne!(builtin, custom);
        assert!(contains_token(&builtin));
        assert!(contains_token(&custom));
    }

    #[test]
    fn empty_normalized_name_matches_current_shape() {
        let token = tokenized_for(PiiClass::custom("!!!"));
        assert!(token.ends_with(":Custom:_1>"));
        assert!(contains_token(&token));
    }

    #[test]
    fn single_char_custom_name_matches_current_shape() {
        let token = tokenized_for(PiiClass::custom("x"));
        assert!(token.ends_with(":Custom:x_1>"));
        assert!(contains_token(&token));
    }

    #[test]
    fn custom_token_matches_as_single_span() {
        let haystack = format!("before <Custom:{}> after", ["class_alpha", "1"].join("_"));
        let matched = pattern().find(&haystack).expect("custom token match");
        assert_eq!(
            matched.as_str(),
            format!("<Custom:{}>", ["class_alpha", "1"].join("_"))
        );
    }

    #[test]
    fn contains_bare_shapes_in_prose() {
        assert!(contains_token(&format!(
            "See <{}>.",
            ["Email", "1"].join("_")
        )));
        assert!(contains_token(&format!(
            "See <Custom:{}>.",
            ["class_alpha", "1"].join("_")
        )));
        assert!(contains_token("Reply to name_1."));
        assert!(contains_token("Email email1@example.test later."));
    }

    #[test]
    fn legacy_shape_parity_traps_all_known_v03_forms() {
        for shape in [
            format!("<{}>", ["Email", "1"].join("_")),
            format!("<Custom:{}>", ["class_alpha", "1"].join("_")),
            format!("<{}>", ["Foo", "5"].join("_")),
            format!("<{}>", ["foo", "1"].join("_")),
            ["Email", "7"].join("_"),
            ["location", "7"].join("_"),
            ["name", "1"].join("_"),
            ["organization", "1"].join("_"),
            ["email", "1"].join("_"),
            format!("custom:{}", ["class_alpha", "1"].join("_")),
            "email3@example.test".to_string(),
            "email3@gaze-fake.invalid".to_string(),
        ] {
            assert!(contains_token(&shape), "shape should be trapped: {shape}");
        }
    }

    #[test]
    fn session_prefix_scan_classifies_prefixed_forms() {
        assert!(starts_with_session_prefix("<a7f3b8e2:Email_1>"));
        assert!(starts_with_session_prefix(
            "email1.a7f3b8e2@gaze-fake.invalid"
        ));
        assert!(starts_with_session_prefix("a7f3b8e2:name_1"));
        assert!(!is_trap("<a7f3b8e2:Email_1>"));
    }

    #[test]
    fn session_prefix_scan_rejects_trap_forms() {
        assert!(is_trap(&format!("<{}>", ["Email", "1"].join("_"))));
        assert!(is_trap("email1@example.test"));
        assert!(is_trap("email1@gaze-fake.invalid"));
        assert!(is_trap("<A7F3B8E2:Email_1>"));
    }

    #[test]
    fn rejects_non_tokens() {
        assert!(!contains_token("See <Email_1bar>."));
        assert!(!contains_token("literal email@example.invalid address"));
        assert!(!contains_token("<Custom:-_1>"));
    }

    #[test]
    fn unicode_digits_do_not_match_token_ordinals() {
        for digit in ["\u{11DA0}", "\u{0966}", "\u{0660}", "\u{FF10}"] {
            for shape in [
                format!("a_{digit}"),
                format!("Email_{digit}"),
                format!("<Email_{digit}>"),
                format!("<deadbeef:Email_{digit}>"),
                format!("email{digit}@example.test"),
                format!("email{digit}.deadbeef@gaze-fake.invalid"),
            ] {
                assert!(
                    !contains_token(&shape),
                    "Unicode digit must not match token ordinal: {shape:?}"
                );
            }
        }
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
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .expect("first token");
        let second = session
            .tokenize(&PiiClass::Email, "bob@example.invalid")
            .expect("second token");

        let rendered = format!("See {first}. Reply {second}");
        let restored = pattern().replace_all(&rendered, |captures: &regex::Captures<'_>| {
            session.restore_strict(&captures[0]).expect("known token")
        });

        assert_eq!(
            restored,
            "See alice@example.invalid. Reply bob@example.invalid"
        );
    }
}
