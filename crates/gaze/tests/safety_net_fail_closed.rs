//! Fail-closed integrity contract for safety-net RESOLVE (solo todo #2410).
//!
//! `SafetyNetMode::Resolve` used to have exactly one post-condition: re-run the nets and take the
//! fallback if the follow-up still counted uncovered/partial-bleed suspects. Every other guarantee
//! was delegated to the net agreeing with itself on a second pass over text the first pass had just
//! mutated. A net whose second pass disagrees with its first — an ML net, a cached net, a sampled
//! net — could therefore drive the pipeline to `Ok(...)` with raw PII surviving in the clean text
//! and a permanently unrestorable manifest.
//!
//! These tests pin the replacement contract. Each one asserts the TYPED outcome, because the
//! interesting failure is not "did it error" but "did it produce a reason an adopter can act on":
//! a typed `FallbackReason` lets `Tolerant`/`Redact` adopters degrade gracefully, whereas a bare
//! integrity error is a hard stop for everyone.
//!
//! Synthetic fixtures only.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Mutex;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, EmittedTokenSpan,
    FallbackReason, LeakKind, LeakReport, LeakSuspect, PiiClass, Pipeline, RawDocument, SafetyNet,
    SafetyNetContext, SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope,
    Session,
};

const EMAIL: &str = "alice@example.invalid";
const OTHER: &str = "bob@example.invalid";

/// One scripted suspect: the net reports exactly this, whatever the text says.
///
/// Lying stubs are the point. A real net can report a stale or wrong span — that is the failure
/// class this contract exists for — so the pipeline must not trust a suspect's coverage claim to
/// agree with the manifest. It has to check.
#[derive(Clone)]
struct ScriptedSuspect {
    span: Range<usize>,
    kind: LeakKind,
    class: PiiClass,
}

impl ScriptedSuspect {
    fn uncovered(span: Range<usize>) -> Self {
        Self {
            span,
            kind: LeakKind::Uncovered,
            class: PiiClass::Email,
        }
    }

    fn class_mismatch(span: Range<usize>) -> Self {
        Self {
            kind: LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            },
            span,
            class: PiiClass::Name,
        }
    }
}

/// A net that replays a fixed script, one entry per `check` call.
///
/// Deterministic by construction: no model, no sampling, no dependence on the mutated text.
struct ScriptedNet {
    passes: Mutex<VecDeque<Vec<ScriptedSuspect>>>,
}

impl SafetyNet for ScriptedNet {
    fn id(&self) -> &str {
        "scripted"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let pass = self.passes.lock().unwrap().pop_front().unwrap_or_default();
        Ok(pass
            .into_iter()
            .filter(|suspect| suspect.span.end <= clean_text.len())
            .map(|suspect| {
                LeakSuspect::new(
                    suspect.span,
                    suspect.class,
                    self.id(),
                    Some(0.99),
                    suspect.kind,
                    "private_email",
                    context.field_path.map(str::to_string),
                )
            })
            .collect())
    }
}

#[derive(Clone)]
struct FixedDetector {
    spans: Vec<Range<usize>>,
}

impl Detector for FixedDetector {
    fn detect(&self, _input: &str) -> Vec<Detection> {
        self.spans
            .iter()
            .map(|span| Detection::new(span.clone(), PiiClass::Email, "fixed"))
            .collect()
    }
}

/// One primary-pass detection. Named because a bare `vec![a..b]` of ranges is ambiguous enough
/// that clippy rejects it.
fn detected(span: Range<usize>) -> Vec<Range<usize>> {
    Vec::from([span])
}

fn text(clean: CleanDocument) -> String {
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

/// Tokenizing pipeline plus a scripted net. `detected` are the primary-pass Email spans.
fn pipeline_with(detected: Vec<Range<usize>>, passes: Vec<Vec<ScriptedSuspect>>) -> Pipeline {
    Pipeline::builder()
        .detector(FixedDetector { spans: detected })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(ScriptedNet {
            passes: Mutex::new(passes.into()),
        })
        .build()
        .expect("pipeline")
}

type CleanOutcome = gaze::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)>;

fn run(
    pipeline: &Pipeline,
    session: &Session,
    raw: &str,
    fallback: SafetyNetFallback,
) -> CleanOutcome {
    pipeline.clean_with_safety_net_policy_detect_context(
        session,
        RawDocument::Text(raw.to_string()),
        &[gaze::LocaleTag::Global],
        &gaze::DictionaryBundle::default(),
        SafetyNetPolicy::new(SafetyNetMode::Resolve, fallback),
    )
}

/// Clean text produced by the primary pass alone, for fixtures that need token offsets.
fn primary_clean(detected: Vec<Range<usize>>, raw: &str) -> String {
    let pipeline = pipeline_with(detected, Vec::new());
    let session = Session::new(Scope::Ephemeral).expect("probe session");
    text(
        run(&pipeline, &session, raw, SafetyNetFallback::Strict)
            .expect("primary-only baseline must succeed")
            .0,
    )
}

fn expect_fallback(outcome: CleanOutcome, expected: FallbackReason, context: &str) {
    match outcome {
        Err(gaze::Error::SafetyNetFallback(reason)) => {
            assert_eq!(reason, expected, "{context}: wrong typed fallback reason");
        }
        Err(other) => panic!(
            "{context}: expected a typed SafetyNetFallback({expected:?}) an adopter can act on, \
             got {other:?}"
        ),
        Ok((clean, manifest, _)) => panic!(
            "{context}: expected fail-closed, got Ok(clean={:?}, manifest={manifest:?})",
            text(clean)
        ),
    }
}

/// Overlapping resolutions must be rejected before any of them is applied.
///
/// Applied in sequence they tokenize over a token emitted moments earlier: the second suspect's
/// raw fragment survives in the clean text and the manifest can no longer restore the document.
/// This is the shipped-main leak from todo #2410.
#[test]
fn overlapping_resolutions_fail_closed_with_a_typed_overlap_conflict() {
    let raw = format!("{EMAIL} and {OTHER}");
    let pipeline = pipeline_with(
        Vec::new(),
        vec![vec![
            ScriptedSuspect::uncovered(0..EMAIL.len()),
            ScriptedSuspect::uncovered(10..EMAIL.len() + 5),
        ]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    expect_fallback(
        run(&pipeline, &session, &raw, SafetyNetFallback::Strict),
        FallbackReason::OverlapConflict,
        "overlapping resolutions",
    );
}

/// The same overlap under the DEFAULT fallback still delivers a document.
///
/// This is the adopter-facing half of the contract: integrity failures route through the existing
/// fallback path, so `SafetyNetFallback::Redact` keeps meaning redact-and-deliver.
#[test]
fn overlapping_resolutions_under_redact_fallback_still_deliver_a_document() {
    let raw = format!("{EMAIL} and {OTHER}");
    let pipeline = pipeline_with(
        Vec::new(),
        vec![vec![
            ScriptedSuspect::uncovered(0..EMAIL.len()),
            ScriptedSuspect::uncovered(10..EMAIL.len() + 5),
        ]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    let (clean, _, _) = run(&pipeline, &session, &raw, SafetyNetFallback::Redact)
        .expect("Redact fallback must deliver a redacted document, not a hard error");
    let clean = text(clean);
    assert!(
        !clean.contains("example.invalid"),
        "redact fallback left raw fixture bytes in {clean:?}"
    );
}

/// A suspect claiming `Uncovered` while overlapping a live token contradicts the manifest.
///
/// Resolving it would tokenize across an existing token. The suspect/manifest agreement gate
/// rejects it instead of trusting the net's coverage claim.
#[test]
fn uncovered_suspect_overlapping_a_live_token_fails_closed_with_overlap_conflict() {
    let raw = format!("{EMAIL} tail");
    let clean_len = primary_clean(detected(0..EMAIL.len()), &raw).len();
    // The net claims the WHOLE clean document is uncovered — a claim the manifest contradicts,
    // yet whose boundaries both map cleanly, so span arithmetic alone cannot catch it.
    let pipeline = pipeline_with(
        detected(0..EMAIL.len()),
        vec![vec![ScriptedSuspect::uncovered(0..clean_len)]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    expect_fallback(
        run(&pipeline, &session, &raw, SafetyNetFallback::Strict),
        FallbackReason::OverlapConflict,
        "uncovered suspect overlapping a live token",
    );
}

/// Overlap detection is only correct on sorted plans.
///
/// Two overlapping suspects reported non-adjacently (A, C, B with A∩B) are invisible to a pairwise
/// scan of the report order. Without the ordering discipline both would be applied and the
/// document would be corrupted.
#[test]
fn overlapping_suspects_reported_non_adjacently_still_fail_closed() {
    let raw = format!("{EMAIL} and {OTHER} and more text here");
    let pipeline = pipeline_with(
        Vec::new(),
        vec![vec![
            ScriptedSuspect::uncovered(0..5),
            ScriptedSuspect::uncovered(40..45),
            ScriptedSuspect::uncovered(3..8),
        ]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    expect_fallback(
        run(&pipeline, &session, &raw, SafetyNetFallback::Strict),
        FallbackReason::OverlapConflict,
        "non-adjacent overlapping suspects",
    );
}

/// A residual suspect that only appears on the FOLLOW-UP pass must still be decided.
///
/// The follow-up verdict used to be `uncovered_count + partial_bleed_count > 0`, so a follow-up
/// class mismatch counted as zero residue and the document completed silently.
#[test]
fn follow_up_class_mismatch_outside_a_token_fails_closed() {
    let raw = format!("{EMAIL} tail");
    // Pass 1 reports nothing; the follow-up pass reports a mismatch over untokenized text.
    let pipeline = pipeline_with(
        detected(0..EMAIL.len()),
        vec![Vec::new(), vec![ScriptedSuspect::class_mismatch(19..23)]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    expect_fallback(
        run(&pipeline, &session, &raw, SafetyNetFallback::Strict),
        FallbackReason::OverlapConflict,
        "follow-up class mismatch",
    );
}

/// A suspect wholly inside a live token is the net re-flagging protected text.
///
/// It must be a reversible no-op: acting on it would tokenize a token and destroy restore, and
/// failing closed would deny a document that is already safe.
#[test]
fn suspect_inside_a_live_token_is_a_reversible_noop() {
    let raw = format!("{EMAIL} tail");
    let baseline = primary_clean(detected(0..EMAIL.len()), &raw);
    let token_end = baseline.find(" tail").expect("token suffix");

    let pipeline = pipeline_with(
        detected(0..EMAIL.len()),
        vec![vec![ScriptedSuspect::uncovered(2..token_end - 1)]],
    );
    let session = Session::new(Scope::Ephemeral).expect("session");

    let (clean, manifest, _) = run(&pipeline, &session, &raw, SafetyNetFallback::Strict)
        .expect("a suspect inside a live token must not fail closed");
    let clean = text(clean);
    assert_eq!(manifest.len(), 1, "no second token may be emitted");
    assert_eq!(
        session
            .restore_strict_text(&clean)
            .expect("protected suspect keeps the document restorable"),
        raw
    );
}

/// One corpus row: raw document, primary-pass detections, scripted net passes.
type CorpusDocument = (String, Vec<Range<usize>>, Vec<Vec<ScriptedSuspect>>);

/// AVAILABILITY REGRESSION CORPUS.
///
/// Every document here completes on unmodified main. The integrity checks must not turn any of
/// them into a failed-closed outcome under any fallback setting. This is the explicit guard
/// against importing the availability drop the upstream fail-closed commit shipped.
#[test]
fn availability_corpus_has_no_new_failed_closed_outcomes() {
    let tail = format!("{EMAIL} tail");
    let two = format!("{EMAIL} and {OTHER}");
    let foreign = format!("ticket <deadbeef:Email_9> from {EMAIL}");
    let foreign_at = foreign.find(EMAIL).expect("email offset");
    let both = vec![
        0..EMAIL.len(),
        EMAIL.len() + 5..EMAIL.len() + 5 + OTHER.len(),
    ];

    let corpus: Vec<CorpusDocument> = vec![
        // No detections, no suspects.
        (
            "plain text with no pii at all".to_string(),
            Vec::new(),
            Vec::new(),
        ),
        // Primary tokenization, net silent.
        (tail.clone(), detected(0..EMAIL.len()), Vec::new()),
        // Two primary tokens, net silent.
        (two.clone(), both, Vec::new()),
        // A single promotion of untokenized text.
        (
            tail.clone(),
            Vec::new(),
            vec![vec![ScriptedSuspect::uncovered(0..EMAIL.len())]],
        ),
        // Two non-overlapping promotions.
        (
            two.clone(),
            Vec::new(),
            vec![vec![
                ScriptedSuspect::uncovered(0..EMAIL.len()),
                ScriptedSuspect::uncovered(EMAIL.len() + 5..EMAIL.len() + 5 + OTHER.len()),
            ]],
        ),
        // A promotion of trailing text alongside an existing token.
        (
            tail.clone(),
            detected(0..EMAIL.len()),
            vec![vec![ScriptedSuspect::uncovered(0..0)]], // replaced below with real offsets
        ),
        // A document quoting a FOREIGN gaze token: its clean text is not restorable, so the
        // restore-integrity baseline is undefined. It must still complete.
        (
            foreign.clone(),
            detected(foreign_at..foreign_at + EMAIL.len()),
            Vec::new(),
        ),
    ];

    // Fill in the one fixture whose suspect span depends on the emitted token length.
    let tail_clean = primary_clean(detected(0..EMAIL.len()), &tail);
    let tail_token_end = tail_clean.find(" tail").expect("token suffix");
    let promotion_next_to_token = vec![vec![ScriptedSuspect::uncovered(
        tail_token_end + 1..tail_clean.len(),
    )]];

    let fallbacks = [
        SafetyNetFallback::Strict,
        SafetyNetFallback::Tolerant,
        SafetyNetFallback::Redact,
    ];
    let mut completed = 0usize;
    let mut failed_closed = Vec::new();
    for (index, (raw, detected, passes)) in corpus.iter().enumerate() {
        for fallback in fallbacks {
            let passes = if index == 5 {
                promotion_next_to_token.clone()
            } else {
                passes.clone()
            };
            let pipeline = pipeline_with(detected.clone(), passes);
            let session = Session::new(Scope::Ephemeral).expect("session");
            match run(&pipeline, &session, raw, fallback) {
                Ok(_) => completed += 1,
                Err(error) => failed_closed.push(format!("doc {index} / {fallback:?}: {error:?}")),
            }
        }
    }
    assert!(
        failed_closed.is_empty(),
        "{} of {} corpus runs failed closed: {failed_closed:?}",
        failed_closed.len(),
        completed + failed_closed.len()
    );
    assert_eq!(completed, corpus.len() * fallbacks.len());
}
