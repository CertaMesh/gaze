//! Public bounded-restore API against the current strict parser and UTF-8 budgets.
use gaze::{BoundedRestoreError, PiiClass, Scope, Session};

#[test]
fn bounded_restore_preserves_utf8_and_current_family_parser_failures() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "synthetic-é🦀";
    let owned = session.tokenize(&PiiClass::Name, raw).unwrap();
    let family = session
        .tokenize_with_family("tenant-document", &PiiClass::Name, "synthetic family")
        .unwrap();
    let before = session.snapshot_entries();
    let frozen = session.begin_transaction();
    let input = format!("prefix/{owned}/{family}/suffix");
    let expected = format!("prefix/{raw}/synthetic family/suffix");
    assert_eq!(
        frozen
            .restore_strict_text_bounded(&input, expected.len())
            .unwrap(),
        expected
    );
    assert_eq!(
        frozen.restore_strict_text_bounded(&input, expected.len() - 1),
        Err(BoundedRestoreError::CapExceeded)
    );
    for malformed in [
        "<Custom:family:tenant-document_>",
        "<deadbeef:Custom:family:tenant-document_>",
        "<Email_>",
    ] {
        assert!(frozen.restore_strict_text(malformed).is_err());
        let error = frozen
            .restore_strict_text_bounded(malformed, 1024)
            .unwrap_err();
        assert_eq!(error, BoundedRestoreError::MalformedToken);
        assert_eq!(error.to_string(), "request token malformed");
        assert!(!format!("{error:?}").contains(malformed));
    }
    assert_eq!(frozen.restore_strict_text_bounded("", 0).unwrap(), "");
    drop(frozen);
    assert_eq!(session.snapshot_entries(), before);
}

#[test]
fn owned_and_foreign_tokens_share_one_frozen_read_only_request() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let foreign_session = Session::new(Scope::Ephemeral).unwrap();
    let first = session
        .tokenize(&PiiClass::Email, "first@example.invalid")
        .unwrap();
    let foreign = foreign_session
        .tokenize(&PiiClass::Email, "foreign@example.invalid")
        .unwrap();
    let frozen = session.begin_transaction();
    let later = session
        .tokenize(&PiiClass::Name, "synthetic later")
        .unwrap();
    let before_restore = session.snapshot_entries();
    for unknown in [foreign, later, "<deadbeef:Email_999>".into()] {
        let error = frozen
            .restore_strict_text_bounded(&format!("{first}/{unknown}"), 1024)
            .unwrap_err();
        assert_eq!(error, BoundedRestoreError::UnknownToken);
        assert_eq!(error.to_string(), "request token rejected");
        assert_eq!(format!("{error:?}"), "UnknownToken");
    }
    assert_eq!(
        frozen
            .restore_strict_text_bounded("first@example.invalid", 21)
            .unwrap(),
        "first@example.invalid"
    );
    drop(frozen);
    assert_eq!(session.snapshot_entries(), before_restore);
}
