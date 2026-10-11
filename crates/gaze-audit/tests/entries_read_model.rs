use gaze_audit::{AuditFilter, SqliteLogger};
use gaze_types::{Action, ConflictTier, DocumentKind, PiiClass, RedactionEntry};

#[test]
fn entries_and_query_retain_every_logged_provenance_field() {
    let temp = tempfile::NamedTempFile::new().unwrap();
    let logger = SqliteLogger::new(temp.path()).unwrap();
    let entry = RedactionEntry::new(
        "synthetic",
        PiiClass::Name,
        Action::Tokenize,
        None,
        DocumentKind::Text,
        false,
        ConflictTier::None,
        123,
        None,
    )
    .with_provenance_metadata(
        Some("ner".into()),
        Some("model-a".into()),
        Some("version-b".into()),
        Some("a".repeat(64)),
        Some("b".repeat(64)),
        Some("de-DE".into()),
        Some("exact".into()),
        Some("name".into()),
        Some("PER".into()),
        Some(0.875),
        Some("[\"rule-c\",\"dictionary-d\"]".into()),
    )
    .with_backend_silently_dropped(vec!["private-backend".into()]);
    logger.log(&entry).unwrap();
    let entries = logger.entries().unwrap();
    let rows = SqliteLogger::query(temp.path(), &AuditFilter::default()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(rows.len(), 1);
    macro_rules! retained {
        ($($field:ident),+ $(,)?) => {$(
            assert_eq!(entries[0].$field, entry.$field, stringify!($field));
            assert_eq!(rows[0].$field, entry.$field, stringify!($field));
        )+};
    }
    retained!(
        provenance_stage,
        provenance_model_id,
        provenance_model_version,
        provenance_artifact_sha256,
        provenance_tokenizer_sha256,
        provenance_locale_resolved,
        provenance_locale_match_kind,
        provenance_canonical_class,
        provenance_native_class,
        provenance_merged_from
    );
    assert_eq!(entries[0].provenance_confidence.as_deref(), Some("0.875"));
    assert_eq!(rows[0].provenance_confidence, Some(0.875));
    assert_eq!(
        entries[0].backend_silently_dropped,
        entry.backend_silently_dropped
    );
}
