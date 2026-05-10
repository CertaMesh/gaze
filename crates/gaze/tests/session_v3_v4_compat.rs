use gaze::{
    DocumentExtension, EmittedTokenSpan, Error, PiiClass, Scope, SensitiveSnapshot, Session,
};
use sha2::{Digest, Sha256};

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

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
fn document_extension_signed_envelope_binds_bundle_files() {
    let session = Session::new(Scope::Conversation("doc-integrity".to_string())).expect("session");
    let token = session
        .tokenize(&PiiClass::Name, "Dr. Schmidt")
        .expect("token");
    let clean_md = format!("Reviewer: {token}\n");
    let layout_json = br#"{"schema_version":1,"items":[]}"#;
    let report_json = br#"{"schema_version":1,"status":"ok"}"#;
    let preview_png = b"\x89PNG\r\n\x1a\npreview";

    let mut extension = DocumentExtension::new(1, gaze::TextOrigin::EmbeddedText);
    extension.clean_md_sha256 = sha256(clean_md.as_bytes());
    extension.layout_json_sha256 = sha256(layout_json);
    extension.report_json_sha256 = sha256(report_json);
    extension.preview_png_sha256 = Some(sha256(preview_png));
    extension.page_count = 1;
    extension.audit_session_id = session.audit_session_id().to_string();
    extension.clean_spans = vec![EmittedTokenSpan::new(
        10..10 + token.len(),
        10..21,
        PiiClass::Name,
    )];

    let snapshot = session
        .export_with_extension(extension.clone())
        .expect("document export");
    let payload: serde_json::Value =
        serde_json::from_slice(&snapshot.into_bytes()[97..]).expect("snapshot payload");
    let signed_extension: DocumentExtension =
        serde_json::from_value(payload["document"].clone()).expect("document extension");

    assert_eq!(signed_extension, extension);
    assert_ne!(
        signed_extension.clean_md_sha256,
        sha256(format!("Reviewer: {token} swapped\n").as_bytes())
    );
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
