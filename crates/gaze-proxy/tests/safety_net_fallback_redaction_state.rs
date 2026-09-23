//! Precondition control for the proxy fallback-redaction leak proofs.
//!
//! The wire-level proofs in `legacy_adapter_leak_proof.rs` and `anthropic_direct.rs` assert an
//! OUTCOME: when a configured safety net routes provider-origin PII into the `Redact` fallback,
//! those bytes never reach the client. The outcome is only meaningful if the pipeline really does
//! reach the state the proofs describe, and that state changed.
//!
//! The fallback used to DELETE the flagged bytes and emit no manifest entry, leaving a SUCCESSFUL
//! clean with an EMPTY manifest -- the hazard the proxy's `manifest_accounts_for_every_change`
//! precondition exists to catch. It now writes a one-way `[REDACTED:<class>]` marker and records
//! it as a manifest entry standing for the original bytes. The proofs now hold on the ordinary
//! path instead: the residual check sees that entry's raw span and rejects it for lying outside
//! every authorized output range.
//!
//! This control pins THAT state. If a future change stopped recording the redaction -- reverting
//! to deletion, or writing the marker without an entry -- the leak proofs would still pass by
//! falling back on the accounting precondition, but the ordinary residual path they are now meant
//! to exercise would be silently gone. This turns RED and names the reason instead.
//!
//! Fixtures are synthetic-only per AGENTS.md rule 2.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gaze::{
    is_redaction_marker, Action, ClassRule, CleanDocument, DefaultRule, DictionaryBundle,
    LocaleTag, PiiClass, Pipeline, RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;

#[path = "support/fallback_redacting_net.rs"]
mod fallback_redacting_net;
use fallback_redacting_net::{FallbackRedactingNet, MARKER};

#[test]
fn control_successful_fallback_redaction_records_a_manifest_entry_over_the_original_bytes() {
    let hits = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(FallbackRedactingNet { hits: hits.clone() })
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let candidate = format!("lead {MARKER} tail");

    let (clean, spans, report) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(candidate.clone()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("the fallback redaction path returns Ok");

    let CleanDocument::Text(clean_text) = clean else {
        panic!("text in, text out");
    };
    assert!(hits.load(Ordering::SeqCst) > 0, "the net must have fired");
    // 1. The clean text is safe: the fallback removed the bytes.
    assert!(
        !clean_text.contains(MARKER),
        "fallback redaction must remove the flagged bytes: {clean_text:?}"
    );
    // 2. The redaction IS in the manifest, standing for exactly the original bytes. This is what
    //    the proxy's residual check now reads; an empty manifest here means the ordinary path
    //    the leak proofs depend on has gone.
    let start = candidate.find(MARKER).expect("fixture");
    assert_eq!(spans.len(), 1, "one entry for the one redaction: {spans:?}");
    assert_eq!(spans[0].raw_span, start..start + MARKER.len());
    assert!(
        is_redaction_marker(&clean_text[spans[0].clean_span.clone()]),
        "the entry must stand on a one-way marker, not a token: {clean_text:?}"
    );
    // 3. It is not restorable. A marker the session could turn back into the original bytes would
    //    make the redaction a reversible token in disguise.
    assert_eq!(
        session.restore(&clean_text[spans[0].clean_span.clone()]),
        None
    );
    // 4. The report names the suspect, and the input the proxy holds still carries the raw bytes.
    assert!(!report.suspects.is_empty());
    assert!(candidate.contains(MARKER));
}

/// Second control: a redaction BETWEEN two tokenized detections is its own entry, so every byte
/// of the document is accounted for by the manifest.
///
/// The deletion version of this control pinned the arrangement in which the gap comparison inside
/// `manifest_accounts_for_every_change` was the ONLY thing that caught the removal: the deleted
/// bytes fell between two entries and left no record. A marker leaves a record, so that
/// arrangement is no longer produced by the pipeline. The gap comparison stays in the proxy as
/// defence in depth against any future unaccounted mutation, and stays pinned at unit level by
/// `manifest_accounting_refuses_a_deletion_that_falls_between_two_entries`; what this control now
/// pins is that the pipeline no longer creates the gap.
#[test]
fn control_fallback_redaction_between_two_entries_is_its_own_entry() {
    let hits = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::new("(alice|bob)@example\\.invalid", PiiClass::Email).expect("regex"),
        )
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(FallbackRedactingNet { hits: hits.clone() })
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let candidate = format!("alice@example.invalid {MARKER} bob@example.invalid");

    let (clean, spans, _report) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(candidate.clone()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("the fallback redaction path returns Ok");
    let CleanDocument::Text(clean_text) = clean else {
        panic!("text in, text out");
    };

    assert!(hits.load(Ordering::SeqCst) > 0, "the net must have fired");
    assert!(!clean_text.contains(MARKER));
    assert_eq!(
        spans.len(),
        3,
        "two tokens and the redaction between them: {spans:?}"
    );
    let middle = &spans[1];
    let start = candidate.find(MARKER).expect("fixture");
    assert_eq!(middle.raw_span, start..start + MARKER.len());
    assert!(is_redaction_marker(&clean_text[middle.clean_span.clone()]));

    // Every gap between consecutive entries is untouched text, byte for byte: nothing was removed
    // that the manifest does not describe.
    for pair in spans.windows(2) {
        assert_eq!(
            &candidate[pair[0].raw_span.end..pair[1].raw_span.start],
            &clean_text[pair[0].clean_span.end..pair[1].clean_span.start],
            "a gap between entries changed without a manifest entry to say so"
        );
    }
}
