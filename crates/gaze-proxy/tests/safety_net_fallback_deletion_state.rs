//! Precondition control for the proxy fallback-deletion leak proofs.
//!
//! The wire-level proofs in `legacy_adapter_leak_proof.rs` and `anthropic_direct.rs` are only
//! meaningful if the pipeline really does reach the state they describe: a SUCCESSFUL clean
//! whose safety-net fallback DELETED the only PII, leaving an empty manifest behind. If a
//! future pipeline change made that state unreachable — by failing closed earlier, or by
//! emitting a manifest entry for the deletion — those proofs would keep passing while proving
//! nothing.
//!
//! This test pins the state itself, so such a change turns this control RED and names the
//! reason instead of silently hollowing out the proofs.
//!
//! Fixtures are synthetic-only per AGENTS.md rule 2.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, PiiClass, Pipeline,
    RawDocument, Scope, Session,
};
use gaze_recognizers::RegexDetector;

#[path = "support/fallback_deleting_net.rs"]
mod fallback_deleting_net;
use fallback_deleting_net::{FallbackDeletingNet, MARKER};

#[test]
fn control_successful_fallback_deletion_returns_ok_with_an_empty_manifest() {
    let hits = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(FallbackDeletingNet { hits: hits.clone() })
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
        .expect("the fallback deletion path returns Ok, which is what makes it dangerous");

    let CleanDocument::Text(clean_text) = clean else {
        panic!("text in, text out");
    };
    assert!(hits.load(Ordering::SeqCst) > 0, "the net must have fired");
    // 1. The clean text is safe: the fallback did remove the bytes.
    assert!(
        !clean_text.contains(MARKER),
        "fallback deletion must remove the marker: {clean_text:?}"
    );
    // 2. The manifest is empty: deletion emits no `EmittedTokenSpan`, so nothing in `spans`
    //    describes where the PII was. This is the whole hazard for a manifest-only check.
    assert!(
        spans.is_empty(),
        "deletion must not emit a manifest entry: {spans:?}"
    );
    // 3. The report DOES name the suspect. It is the only surviving evidence, and both proxy
    //    call sites discard it.
    assert!(
        !report.suspects.is_empty(),
        "the report is the evidence the proxy throws away"
    );
    // 4. The input the proxy still holds is untouched and still carries the raw bytes.
    assert!(candidate.contains(MARKER));
}

/// Second precondition control: the shape in which the GAP comparison is the deciding check.
///
/// The control above reaches the fallback-deletion state with an EMPTY manifest, so
/// `manifest_accounts_for_every_change` refuses it on the TAIL comparison and the gap comparison
/// never decides anything. `manifest_accounting_refuses_a_deletion_that_falls_between_two_entries`
/// pins the gap comparison at unit level, but a unit test over a hand-written span pair cannot
/// say whether the pipeline still PRODUCES that pair. If a future change made this state
/// unreachable, that unit test would keep passing while pinning nothing anyone can hit.
///
/// So this pins the state: two detections that DO mint manifest entries, with the net-only marker
/// deleted from the gap between them, and the second detection ending the string so both tails
/// are empty and equal. That is the one arrangement where removing `raw_gap != clean_gap` makes
/// the pair read as fully accounted for.
///
/// Fixtures are synthetic-only per AGENTS.md rule 2.
#[test]
fn control_fallback_deletion_can_land_between_two_manifest_entries() {
    let hits = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .detector(
            RegexDetector::new("(alice|bob)@example\\.invalid", PiiClass::Email).expect("regex"),
        )
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(FallbackDeletingNet { hits: hits.clone() })
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    // The marker sits BETWEEN the two detections, and `bob@...` ends the string.
    let candidate = format!("alice@example.invalid {MARKER} bob@example.invalid");

    let (clean, spans, _report) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(candidate.clone()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("the fallback deletion path returns Ok");
    let CleanDocument::Text(clean_text) = clean else {
        panic!("text in, text out");
    };

    assert!(hits.load(Ordering::SeqCst) > 0, "the net must have fired");
    assert!(
        !clean_text.contains(MARKER),
        "the fallback must remove the marker: {clean_text:?}"
    );
    assert_eq!(
        spans.len(),
        2,
        "both emails must tokenize, or there is no gap to fall between: {spans:?}"
    );

    // 1. Both tails are empty, so the tail comparison cannot be what refuses this pair.
    let last = spans
        .iter()
        .max_by_key(|span| span.clean_span.end)
        .expect("a last entry");
    assert_eq!(
        last.raw_span.end,
        candidate.len(),
        "the raw tail must be empty"
    );
    assert_eq!(
        last.clean_span.end,
        clean_text.len(),
        "the clean tail must be empty"
    );

    // 2. The deleted bytes live entirely in the gap between the two entries, which is the only
    //    remaining evidence that anything was removed.
    let first = spans
        .iter()
        .min_by_key(|span| span.clean_span.start)
        .expect("a first entry");
    let raw_gap = &candidate[first.raw_span.end..last.raw_span.start];
    let clean_gap = &clean_text[first.clean_span.end..last.clean_span.start];
    assert!(
        raw_gap.contains(MARKER),
        "the raw gap must hold the marker: {raw_gap:?}"
    );
    assert!(
        !clean_gap.contains(MARKER),
        "the clean gap must not: {clean_gap:?}"
    );
    assert_ne!(
        raw_gap, clean_gap,
        "the gap must be the ONLY evidence of the deletion"
    );
}
