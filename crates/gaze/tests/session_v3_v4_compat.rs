use gaze::{DocumentExtension, Error, Scope, SensitiveSnapshot, Session};

fn v0_6_reader_accepts_only_v2_or_v3(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() < 97 {
        return Err(Error::InvalidSnapshotSignature);
    }
    let version = bytes[0];
    if version != 2 && version != 3 {
        return Err(Error::InvalidSnapshotVersion(version));
    }
    Ok(())
}

#[test]
fn text_only_export_stays_v3_and_document_export_moves_to_v4() {
    let session = Session::new(Scope::Conversation("compat".to_string())).expect("session");
    let token = session
        .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
        .expect("token");

    let text_only = session.export().expect("text-only export").into_bytes();
    assert_eq!(text_only[0], 3);
    assert!(v0_6_reader_accepts_only_v2_or_v3(&text_only).is_ok());

    let document = session
        .export_with_extension(DocumentExtension::default())
        .expect("document export")
        .into_bytes();
    assert_eq!(document[0], 4);
    assert!(matches!(
        v0_6_reader_accepts_only_v2_or_v3(&document),
        Err(Error::InvalidSnapshotVersion(4))
    ));

    let imported =
        Session::import(SensitiveSnapshot::from(document)).expect("current reader imports v4");
    assert_eq!(
        imported.restore(&token).as_deref(),
        Some("alice@example.invalid")
    );
}
