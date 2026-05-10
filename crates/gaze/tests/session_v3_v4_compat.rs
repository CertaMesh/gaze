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

    let extension = DocumentExtension::builder(1)
        .clean_md_sha256(sha256(clean_md.as_bytes()))
        .layout_json_sha256(sha256(layout_json))
        .report_json_sha256(sha256(report_json))
        .preview_png_sha256(sha256(preview_png))
        .page_count(1)
        .audit_session_id(session.audit_session_id())
        .clean_spans(vec![EmittedTokenSpan::new(
            10..10 + token.len(),
            10..21,
            PiiClass::Name,
        )])
        .build()
        .expect("document extension");

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
fn current_exports_use_v5_and_legacy_readers_fail_closed() {
    let session = Session::new(Scope::Conversation("compat".to_string())).expect("session");
    let token = session
        .tokenize(&gaze::PiiClass::Email, "alice@example.invalid")
        .expect("token");

    let text_only = session.export().expect("text-only export").into_bytes();
    assert_eq!(text_only[0], 5);
    assert!(matches!(
        v0_6_reader_accepts_only_v2_or_v3(&text_only),
        Err(Error::InvalidSnapshotVersion(5))
    ));

    let document = session
        .export_with_extension(
            DocumentExtension::builder(1)
                .clean_md_sha256([1; 32])
                .layout_json_sha256([2; 32])
                .report_json_sha256([3; 32])
                .page_count(1)
                .audit_session_id(session.audit_session_id())
                .build()
                .expect("document extension"),
        )
        .expect("document export")
        .into_bytes();
    assert_eq!(document[0], 5);
    assert!(matches!(
        v0_6_reader_accepts_only_v2_or_v3(&document),
        Err(Error::InvalidSnapshotVersion(5))
    ));

    let imported =
        Session::import(SensitiveSnapshot::from(document)).expect("current reader imports v5");
    assert_eq!(
        imported.restore(&token).as_deref(),
        Some("alice@example.invalid")
    );
}
