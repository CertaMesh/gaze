use gaze::{PiiClass, RestorePolicy, Scope, Session};

#[test]
fn known_bare_tokens_restore_after_unicode_words_and_before_punctuation() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for class in [
        PiiClass::Name,
        PiiClass::Location,
        PiiClass::Organization,
        PiiClass::custom("class_alpha"),
    ] {
        let token = session
            .format_preserving_fake(&class, "Synthetic Value")
            .unwrap();
        for leading in ["rec_", "x", "7", "é", "中", "\u{301}", "(", ""] {
            for trailing in ["", ".", ")", "!", " ", "—"] {
                let text = format!("{leading}{token}{trailing}");
                assert_eq!(
                    session.restore_strict_text(&text).unwrap(),
                    format!("{leading}Synthetic Value{trailing}")
                );
            }
        }
    }
}

#[test]
fn bare_token_trailing_word_adjacency_never_partially_restores() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for class in [PiiClass::Name, PiiClass::custom("class_alpha")] {
        let token = session
            .format_preserving_fake(&class, "Synthetic Value")
            .unwrap();
        for leading in ["", "rec_", "é"] {
            for trailing in ["0", "01", "x", "_", "é", "中", "\u{301}", "_1"] {
                let input = format!("{leading}{token}{trailing}");
                let assessment = session.assess_restore_text(&input).unwrap();
                assert_eq!(assessment.into_restored().text, input);
            }
        }
    }
}

#[test]
fn known_longer_ordinals_and_custom_labels_win_without_authorizing_unknowns() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tokens = Vec::new();
    for ordinal in 1..=10 {
        tokens.push(
            session
                .format_preserving_fake(&PiiClass::Name, &format!("Synthetic {ordinal}"))
                .unwrap(),
        );
    }
    let short = session
        .format_preserving_fake(&PiiClass::custom("class_alpha"), "Synthetic Short")
        .unwrap();
    let long = session
        .format_preserving_fake(&PiiClass::custom("class_alpha_1"), "Synthetic Long")
        .unwrap();
    assert_eq!(
        session
            .restore_strict_text(&format!(
                "rec_{} rec_{} rec_{long} rec_{short}",
                tokens[9], tokens[0]
            ))
            .unwrap(),
        "rec_Synthetic 10 rec_Synthetic 1 rec_Synthetic Long rec_Synthetic Short"
    );
    for unknown in [format!("{}0", tokens[9]), format!("{long}0")] {
        for leading in ["", "rec_", "é"] {
            let text = format!("{leading}{unknown}");
            let assessment = session.assess_restore_text(&text).unwrap();
            assert_eq!(assessment.unknown_tokens().len(), 1);
            assert_eq!(assessment.into_restored().text, text);
            assert!(session.restore_strict_text(&text).is_err());
        }
    }
}

#[test]
fn foreign_legacy_and_partial_prefixes_keep_rejection() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let foreign = Session::new(Scope::Ephemeral).unwrap();
    for class in [
        PiiClass::Name,
        PiiClass::Location,
        PiiClass::Organization,
        PiiClass::custom("class_alpha"),
    ] {
        let known = session
            .format_preserving_fake(&class, "Synthetic Value")
            .unwrap();
        let foreign = foreign
            .format_preserving_fake(&class, "Synthetic Foreign")
            .unwrap();
        let legacy = known.split_once(':').unwrap().1;
        for text in [
            foreign.clone(),
            format!("rec_{foreign}"),
            format!("é{foreign}"),
            known[1..].to_owned(),
            legacy.to_owned(),
        ] {
            let assessment = session.assess_restore_text(&text).unwrap();
            assert_eq!(assessment.unknown_tokens().len(), 1, "{text}");
            assert_eq!(assessment.into_restored().text, text);
            assert!(session.restore_strict_text(&text).is_err());
        }
    }
}

#[test]
fn restored_token_like_values_are_not_recursively_expanded_or_reclassified() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let inner = session
        .format_preserving_fake(&PiiClass::Name, "Synthetic Inner")
        .unwrap();
    let raw = format!("{inner} <deadbeef:Email_999> location_999");
    let outer = session
        .format_preserving_fake(&PiiClass::Location, &raw)
        .unwrap();
    let input = format!("é{outer}.");
    let assessment = session.assess_restore_text(&input).unwrap();
    assert!(assessment.unknown_tokens().is_empty());
    let restored = assessment.into_restored();
    assert_eq!(restored.text, format!("é{raw}."));
    assert_eq!(restored.authorized_output_ranges, vec![2..2 + raw.len()]);
    let assessment = session
        .assess_restore_text(&format!("{input}<deadbeef:Email_999>"))
        .unwrap();
    assert_eq!(assessment.unknown_tokens(), &["<deadbeef:Email_999>"]);
    assert_eq!(
        assessment
            .telemetry(RestorePolicy::Strict)
            .unknown_token_count,
        1
    );
}

#[test]
fn email_shaped_known_tokens_keep_their_leading_boundary() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session
        .format_preserving_fake(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    assert_eq!(
        session.restore_strict_text(&format!("({token})")).unwrap(),
        "(alice@example.invalid)"
    );
    for leading in ["x", "_", "é"] {
        let text = format!("{leading}{token}");
        assert_eq!(
            session
                .assess_restore_text(&text)
                .unwrap()
                .into_restored()
                .text,
            text
        );
    }
}

#[test]
fn overlapping_family_tokens_do_not_restore_an_unknown_longer_label() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let short = session
        .format_preserving_fake(&PiiClass::Custom("family:tenant".into()), "Synthetic Short")
        .unwrap();
    let long = session
        .format_preserving_fake(
            &PiiClass::Custom("family:tenant_1-extra".into()),
            "Synthetic Long",
        )
        .unwrap();
    assert_eq!(
        session
            .restore_strict_text(&format!("rec_{long} rec_{short}."))
            .unwrap(),
        "rec_Synthetic Long rec_Synthetic Short."
    );
    let unknown = format!("rec_{short}-other_999");
    let assessment = session.assess_restore_text(&unknown).unwrap();
    assert_eq!(assessment.unknown_tokens().len(), 1, "input={unknown}");
    assert_eq!(assessment.into_restored().text, unknown);
    assert!(session.restore_strict_text(&unknown).is_err());
}
