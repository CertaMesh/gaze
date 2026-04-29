use std::time::{SystemTime, UNIX_EPOCH};

pub use gaze_types::{ConflictTier, DocumentKind, RedactionEntry};

pub(crate) fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// `RedactionEntry` must remain metadata-only.
///
/// ```compile_fail
/// use gaze::{Action, DocumentKind, PiiClass, RedactionEntry};
///
/// let _entry = RedactionEntry {
///     source: "regex".to_string(),
///     class: PiiClass::Email,
///     action: Action::Tokenize,
///     field_name: None,
///     document_kind: DocumentKind::Text,
///     conflict_loser: false,
///     decided_by: gaze::ConflictTier::None,
///     created_at: 0,
///     session_id: None,
///     // fixture-cited(crates/gaze/src/pipeline.rs:pipeline::tests::pipeline_builder_detects_email)
///     raw: Some("alice@example.invalid".to_string()),
/// };
/// ```
const _: () = ();
