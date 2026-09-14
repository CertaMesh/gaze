use gaze::{EmptyCustomClassName, Error, PiiClass, Policy, PolicyError, Scope, Session};

#[test]
fn empty_custom_class_constructor_fails_closed() {
    for name in ["", "!!!", "---", " _ ", "東京"] {
        assert_eq!(PiiClass::custom(name), Err(EmptyCustomClassName));
    }
}

#[test]
fn direct_empty_custom_classes_do_not_mutate_session() {
    for name in ["", "!!!", "---", " _ ", "東京"] {
        let direct = PiiClass::Custom(name.into());
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut transaction = session.begin_transaction();
        for result in [
            session.tokenize(&direct, "synthetic value"),
            session.format_preserving_fake(&direct, "synthetic value"),
            transaction.tokenize(&direct, "synthetic value"),
            transaction.format_preserving_fake(&direct, "synthetic value"),
        ] {
            assert!(matches!(result, Err(Error::EmptyCustomClassName(_))));
        }
        assert!(session.tokens().is_empty());
        assert!(transaction.tokens().is_empty());
        transaction.commit().unwrap();
        let valid = PiiClass::custom("record").unwrap();
        assert!(session
            .tokenize(&valid, "synthetic value")
            .unwrap()
            .ends_with(":Custom:record_1>"));
    }
}

#[test]
fn policy_rejects_empty_normalized_custom_classes_in_rules_and_recognizers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy.toml");
    for name in ["custom:!!!", "custom:---", "custom:東京"] {
        for entry in [
            format!("[[rule]]\nkind = \"class\"\nclass = \"{name}\"\naction = \"tokenize\"\n"),
            format!("[[policy.custom_recognizers]]\nkind = \"regex\"\nname = \"fixture\"\npattern = 'synthetic'\nclass = \"{name}\"\n"),
        ] {
            std::fs::write(&path, format!("[session]\nscope = \"ephemeral\"\n{entry}\n[[rule]]\nkind = \"default\"\naction = \"preserve\"\n")).unwrap();
            assert!(matches!(Policy::load(&path), Err(PolicyError::UnknownClass(found)) if found == name));
        }
    }
}
