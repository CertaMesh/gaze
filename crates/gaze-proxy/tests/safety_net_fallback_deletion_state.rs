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
    Action, CleanDocument, DefaultRule, DictionaryBundle, LocaleTag, Pipeline, RawDocument, Scope,
    Session,
};

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
