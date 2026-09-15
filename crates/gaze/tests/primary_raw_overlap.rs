//! Defensive guard for scalar expansion, independent of recovery policy.
use gaze::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
struct ScalarDetector;
impl Detector for ScalarDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .char_indices()
            .map(|(start, ch)| {
                Detection::new(start..start + ch.len_utf8(), PiiClass::Name, "scalar")
            })
            .collect()
    }
}
struct CountAudit(Arc<AtomicUsize>);
impl RedactionLogger for CountAudit {
    fn log(&self, _: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
fn pipeline() -> (Pipeline, Arc<AtomicUsize>) {
    let audits = Arc::new(AtomicUsize::new(0));
    (
        Pipeline::builder()
            .detector(ScalarDetector)
            .rule(DefaultRule::new(Action::Tokenize))
            .redaction_logger(CountAudit(audits.clone()))
            .build()
            .unwrap(),
        audits,
    )
}
#[test]
fn ordinary_primary_expansion_cannot_publish_malformed_manifest() {
    let (pipeline, audits) = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let result = pipeline.clean_with_safety_net_policy_detect_context(
        &session,
        RawDocument::Text("\u{0344}".into()),
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    );
    assert!(matches!(
        result,
        Err(Error::SafetyNet(SafetyNetError::InvalidOutput { .. }))
    ));
    assert!(session.tokens().is_empty());
    assert_eq!(audits.load(Ordering::SeqCst), 0);
}
#[test]
fn traced_primary_expansion_fails_before_audit_or_allocation() {
    let (pipeline, audits) = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let result = pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_trace(
        &session,
        "\u{0344}",
        &[LocaleTag::Global],
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    );
    assert!(matches!(
        result,
        Err(Error::SafetyNet(SafetyNetError::InvalidOutput { .. }))
    ));
    assert!(session.tokens().is_empty());
    assert_eq!(audits.load(Ordering::SeqCst), 0);
}
#[test]
fn staged_primary_expansion_fails_before_audit_and_discards_cleanly() {
    let (pipeline, audits) = pipeline();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    assert!(pipeline
        .protect_text_transaction(
            &mut tx,
            "\u{0344}",
            ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default())
        )
        .is_err());
    assert_eq!(audits.load(Ordering::SeqCst), 0);
    drop(tx);
    assert!(session.tokens().is_empty());
}
#[test]
fn adjacent_utf8_nfc_fullwidth_and_joiner_controls_restore_exactly() {
    for raw in ["\u{0308}\u{0301}", "éx", "ＡＢ", "A\u{200d}B"] {
        let (pipeline, _) = pipeline();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, spans, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        assert!(spans
            .windows(2)
            .all(|w| w[0].raw_span.end <= w[1].raw_span.start));
        let CleanDocument::Text(text) = clean else {
            panic!("text")
        };
        assert_eq!(session.restore_strict_text(&text).unwrap(), raw);
    }
}
