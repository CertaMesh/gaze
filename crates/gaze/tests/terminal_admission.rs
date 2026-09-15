//! Terminal-admission proofs for the `Resolve` + `Redact` fallback path.
//!
//! Every value here is synthetic; no model evidence and no document text from any corpus.
//!
//! The shape under test: after the fallback deletes, the terminal scan is a fourth full model
//! pass over text no earlier pass ever saw. It can report a span that is simply a fresh finding,
//! one the deletion manufactured by joining two fragments, or one the deletion was supposed to
//! remove and did not. Those are three different facts about the document, and they now get three
//! different outcomes instead of one blanket denial.
use gaze::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Step = (
    String,
    std::result::Result<Vec<LeakSuspect>, SafetyNetError>,
);

/// Asserts each sweep inspects its actual phase output, so a test cannot pass by scanning a
/// cached report against text the pipeline has since changed.
struct Script(Arc<Mutex<VecDeque<Step>>>);
impl SafetyNet for Script {
    fn id(&self) -> &str {
        "terminal.fixture"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let (expected, result) = self.0.lock().unwrap().pop_front().expect("no extra sweep");
        assert_eq!(
            text, expected,
            "each sweep must inspect its actual phase output"
        );
        result
    }
}

fn uncovered(span: std::ops::Range<usize>) -> LeakSuspect {
    LeakSuspect::new(
        span,
        PiiClass::Name,
        "terminal.fixture",
        Some(1.0),
        LeakKind::Uncovered,
        "synthetic",
        Some("field".into()),
    )
}

fn bleed(span: std::ops::Range<usize>, gap: std::ops::Range<usize>) -> LeakSuspect {
    let mut suspect = uncovered(span);
    suspect.kind = LeakKind::PartialBleed { uncovered: gap };
    suspect
}

struct Capture(Arc<Mutex<Vec<RedactionEntry>>>);
impl RedactionLogger for Capture {
    fn log(&self, row: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.0.lock().unwrap().push(row.clone());
        Ok(())
    }
}

/// Primary detector so a one-way `[REDACTED]` replacement can sit in the manifest: it is an
/// entry, but it is not a live token, so a terminal suspect over it counts as unprotected.
struct Primary;
impl Detector for Primary {
    fn detect(&self, text: &str) -> Vec<Detection> {
        text.match_indices("primary")
            .map(|(i, _)| Detection::new(i..i + 7, PiiClass::Email, "primary.fixture"))
            .collect()
    }
}

struct Harness {
    pipeline: Pipeline,
    steps: Arc<Mutex<VecDeque<Step>>>,
    rows: Arc<Mutex<Vec<RedactionEntry>>>,
}

fn harness(steps: Vec<Step>, primary: bool) -> Harness {
    let steps = Arc::new(Mutex::new(VecDeque::from(steps)));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let mut builder = Pipeline::builder();
    if primary {
        builder = builder
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Redact));
    }
    Harness {
        pipeline: builder
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Script(steps.clone()))
            .redaction_logger(Capture(rows.clone()))
            .build()
            .unwrap(),
        steps,
        rows,
    }
}

impl Harness {
    fn run(
        &self,
        session: &Session,
        text: &str,
        policy: SafetyNetPolicy,
    ) -> gaze::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
        self.pipeline.clean_with_safety_net_policy_detect_context(
            session,
            RawDocument::Text(text.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            policy,
        )
    }
    fn drained(&self) -> bool {
        self.steps.lock().unwrap().is_empty()
    }
    fn actions(&self) -> Vec<Action> {
        self.rows.lock().unwrap().iter().map(|r| r.action).collect()
    }
}

/// The scripted document every terminal case starts from.
///
/// `"alpha bravo charlie delta"`: the second batch tokenizes `alpha`, the fallback deletes
/// `charlie`, and the terminal pass then sees `"<alpha> bravo  delta"` — two adjacent spaces with
/// a deletion seam between them, and two never-flagged words on either side of it. `charlie` is
/// deleted from the MIDDLE so that a seam-containing span exists that touches no token: a span
/// overlapping a token is rejected for that reason alone, which would make a bound look enforced
/// when it is not.
struct Doc {
    session: Session,
    alpha: String,
    /// Text the terminal (fourth) sweep sees.
    terminal: String,
    /// `delta`'s span in `terminal`. One byte clear of the seam.
    delta_span: std::ops::Range<usize>,
    /// `bravo`'s span in `terminal`. On the other side of the seam.
    bravo_span: std::ops::Range<usize>,
    /// The two spaces around the seam: the smallest span the deletion manufactured.
    seam_span: std::ops::Range<usize>,
    /// The deletion seam itself, in `terminal`.
    seam: usize,
    lead: Vec<Step>,
}

impl Doc {
    const RAW: &'static str = "alpha bravo charlie delta";

    fn new() -> Self {
        let session = Session::new(Scope::Ephemeral).unwrap();
        // Pre-own the replacement so the random per-session token bytes are known here.
        let alpha = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "alpha")
            .unwrap();
        let resolved = format!("{alpha} bravo charlie delta");
        let terminal = format!("{alpha} bravo  delta");
        let lead = vec![
            (Self::RAW.into(), Ok(vec![])),
            (Self::RAW.into(), Ok(vec![uncovered(0..5)])),
            (
                resolved,
                Ok(vec![uncovered(alpha.len() + 7..alpha.len() + 14)]),
            ),
        ];
        Self {
            delta_span: alpha.len() + 8..alpha.len() + 13,
            bravo_span: alpha.len() + 1..alpha.len() + 6,
            seam_span: alpha.len() + 6..alpha.len() + 8,
            seam: alpha.len() + 7,
            alpha,
            terminal,
            session,
            lead,
        }
    }

    /// `steps` are the sweeps after the three lead-in sweeps: the terminal scan, then whatever
    /// the terminal round produces.
    fn harness(&self, steps: Vec<Step>) -> Harness {
        let mut all = self.lead.clone();
        all.extend(steps);
        harness(all, false)
    }

    fn run(&self, h: &Harness) -> gaze::Result<(CleanDocument, Vec<EmittedTokenSpan>, LeakReport)> {
        h.run(&self.session, Self::RAW, SafetyNetPolicy::default())
    }

    fn token(&self, value: &str) -> String {
        self.session
            .tokenize_with_family("safety_net", &PiiClass::Name, value)
            .unwrap()
    }
}

fn text_of(doc: CleanDocument) -> String {
    let CleanDocument::Text(text) = doc else {
        panic!("text")
    };
    text
}

/// The terminal pass reports a span that no earlier pass flagged and that the deletion neither
/// created nor was supposed to remove. Today the document is denied over it. It must instead be
/// tokenized once, reversibly, and the document must complete.
#[test]
fn terminal_resolves_a_fresh_finding_once_and_completes() {
    let doc = Doc::new();
    let delta = doc.token("delta");
    let resolved = format!("{} bravo  {delta}", doc.alpha);
    let h = doc.harness(vec![
        (
            doc.terminal.clone(),
            Ok(vec![uncovered(doc.delta_span.clone())]),
        ),
        (resolved.clone(), Ok(vec![])),
    ]);
    let (clean, spans, report) = doc
        .run(&h)
        .expect("a fresh terminal finding must be resolved, not denied");
    let text = text_of(clean);
    assert_eq!(text, resolved);
    // Restore is exact for everything the fallback did not delete: `charlie` is gone by the
    // documented `Redact` contract, and both tokens still restore their own source bytes.
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        "alpha bravo  delta"
    );
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 20..25],
        "the terminal round's token must name delta's ORIGINAL bytes"
    );
    assert!(
        report.suspects.iter().any(|s| s.span == doc.delta_span),
        "the terminal finding must be surfaced in the returned report"
    );
    assert!(
        h.drained(),
        "exactly one extra sweep after the terminal scan"
    );
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact, Action::Tokenize],
        "second batch, fallback deletion, then the terminal round"
    );
}

/// The extra round is bounded at one. A finding that appears only after it is reported honestly
/// and shipped, because nothing in the pipeline is permitted to act on it any more.
#[test]
fn terminal_round_happens_at_most_once_and_reports_what_it_could_not_act_on() {
    let doc = Doc::new();
    let delta = doc.token("delta");
    let resolved = format!("{} bravo  {delta}", doc.alpha);
    let h = doc.harness(vec![
        (
            doc.terminal.clone(),
            Ok(vec![uncovered(doc.delta_span.clone())]),
        ),
        (
            resolved.clone(),
            Ok(vec![uncovered(doc.bravo_span.clone())]),
        ),
    ]);
    let (clean, _, report) = doc
        .run(&h)
        .expect("a finding with no round left must complete with an honest report");
    let text = text_of(clean);
    assert_eq!(text, resolved);
    assert!(
        text.contains("bravo"),
        "with both bounds spent the finding ships raw, which is what the report must say"
    );
    assert!(
        report.suspects.iter().any(|s| s.span == doc.bravo_span),
        "the unactionable finding must be in the returned report"
    );
    assert!(h.drained(), "no second terminal round");
}

/// The deletion joined two fragments into a shape the model calls a name. Those bytes are partly
/// gaze's own artefact, so they are deleted once, and the deletion is proved by byte check.
#[test]
fn terminal_deletes_a_seam_manufactured_suspect_once() {
    let doc = Doc::new();
    let deleted = format!("{} bravodelta", doc.alpha);
    let h = doc.harness(vec![
        (
            doc.terminal.clone(),
            Ok(vec![uncovered(doc.seam_span.clone())]),
        ),
        (deleted.clone(), Ok(vec![])),
    ]);
    let (clean, spans, _) = doc
        .run(&h)
        .expect("a seam-manufactured suspect must be deleted, not denied");
    assert_eq!(text_of(clean), deleted);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        vec![0..5],
        "the seam deletion leaves the second batch's token untouched"
    );
    assert!(h.drained());
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact, Action::Redact],
        "second batch, fallback deletion, then the single bounded seam deletion"
    );
}

/// Abutting a seam is not containing it: no byte of the suspect was manufactured by the deletion,
/// so it is a fresh finding and takes the reversible round. The token it mints must name the
/// surviving original bytes, never the removed ones.
#[test]
fn a_suspect_that_only_abuts_the_seam_is_resolved_not_deleted() {
    let doc = Doc::new();
    // Starts exactly at the seam and runs to the end of `delta`.
    let abutting = doc.seam..doc.delta_span.end;
    let token = doc.token(" delta");
    let resolved = format!("{} bravo {token}", doc.alpha);
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(abutting)])),
        (resolved.clone(), Ok(vec![])),
    ]);
    let (clean, spans, _) = doc
        .run(&h)
        .expect("an abutting suspect must be resolved, not deleted");
    let text = text_of(clean);
    assert_eq!(text, resolved);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 19..25],
        "the abutting token must skip the removed range, not swallow it"
    );
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        "alpha bravo  delta"
    );
    assert!(h.drained());
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact, Action::Tokenize]
    );
}

/// Exactly one seam deletion is permitted. A second seam-crossing suspect after it means the
/// deletion is manufacturing shapes at least as fast as it removes them, so the document fails
/// closed rather than deleting again.
#[test]
fn a_second_seam_manufactured_suspect_denies() {
    let doc = Doc::new();
    let deleted = format!("{} bravodelta", doc.alpha);
    // The union of both deletions puts the new seam between `bravo` and `delta`.
    let across = doc.alpha.len() + 5..doc.alpha.len() + 7;
    let h = doc.harness(vec![
        (
            doc.terminal.clone(),
            Ok(vec![uncovered(doc.seam_span.clone())]),
        ),
        (deleted.clone(), Ok(vec![uncovered(across)])),
    ]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "the second seam-manufactured suspect must fail closed"
    );
    assert!(h.drained());
}

/// Two seam-crossing suspects in the same terminal report exceed the bound before any deletion.
/// Both are anchored at the same start so that neither touches a token: a suspect overlapping one
/// is rejected for that reason instead, which would make the bound look enforced when it is not.
#[test]
fn two_seam_manufactured_suspects_in_one_report_deny() {
    let doc = Doc::new();
    let h = doc.harness(vec![(
        doc.terminal.clone(),
        Ok(vec![
            uncovered(doc.seam_span.clone()),
            uncovered(doc.seam_span.start..doc.delta_span.end),
        ]),
    )]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "more seam-manufactured suspects than the bound must fail closed"
    );
    assert!(h.drained());
}

/// The reversible round refuses the report as a whole — here a bleed onto a one-way `[REDACTED]`
/// replacement the round may not re-tokenize. Denial stands, and the `Redact` fallback is NOT
/// re-entered: nothing extra is deleted.
#[test]
fn a_refused_terminal_round_denies_without_deleting() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let primary = "[REDACTED] alpha bravo charlie";
    let deleted = "[REDACTED] alpha  charlie";
    let mut mismatch = uncovered(17..22);
    mismatch.kind = LeakKind::ClassMismatch {
        pipeline_class: PiiClass::Email,
        safety_net_class: PiiClass::Name,
    };
    let h = harness(
        vec![
            (primary.into(), Ok(vec![])),
            (primary.into(), Ok(vec![mismatch])),
            (deleted.into(), Ok(vec![bleed(0..16, 10..16)])),
        ],
        true,
    );
    assert!(
        matches!(
            h.run(
                &session,
                "primary alpha bravo charlie",
                SafetyNetPolicy::default()
            ),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "a refused terminal round must deny"
    );
    assert!(h.drained());
    assert_eq!(
        h.actions(),
        // The primary pass's own `[REDACTED]`, then the fallback deletion. Nothing after.
        [Action::Redact, Action::Redact],
        "the refused round must not delete anything"
    );
}

/// A terminal suspect whose own coverage claim contradicts the manifest, or that carries a token
/// shape this pipeline never minted, cannot be judged — so it is neither resolved nor shipped.
#[test]
fn an_unjudgeable_terminal_suspect_denies() {
    let doc = Doc::new();
    let mut mismatch = uncovered(doc.delta_span.clone());
    mismatch.kind = LeakKind::ClassMismatch {
        pipeline_class: PiiClass::Email,
        safety_net_class: PiiClass::Name,
    };
    for (suspect, expected) in [
        // Claims to cover nothing the manifest covers, while spanning a live token.
        (
            uncovered(0..doc.alpha.len() + 6),
            FallbackReason::ResidualSuspect,
        ),
        (mismatch, FallbackReason::OverlapConflict),
    ] {
        let h = doc.harness(vec![(doc.terminal.clone(), Ok(vec![suspect]))]);
        assert!(
            matches!(doc.run(&h), Err(Error::SafetyNetFallback(reason)) if reason == expected),
            "an unjudgeable terminal suspect must fail closed"
        );
        assert!(h.drained());
    }
}

/// The fallback's audit row says it redacted a span, and part of that span is still in the
/// output. That is the fallback breaking its own promise, and it must stay denied.
///
/// The terminal suspect also CONTAINS the deletion seam, so the two denial clauses genuinely
/// collide here. Without that collision the test would pass on whichever clause happened to fire
/// and would prove nothing about precedence: a broken promise means the document's own record of
/// itself is wrong, which has to outrank a shape the deletion manufactured.
#[test]
fn a_surviving_acted_on_span_stays_denied() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let primary = "[REDACTED] tail rest";
    let deleted = "[REDACTED] rest";
    let h = harness(
        vec![
            (primary.into(), Ok(vec![])),
            (primary.into(), Ok(vec![bleed(0..15, 10..15)])),
            (deleted.into(), Ok(vec![bleed(0..15, 10..15)])),
        ],
        true,
    );
    assert!(
        matches!(
            h.run(&session, "primary tail rest", SafetyNetPolicy::default()),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "a span the fallback claims to have removed, still present, must fail closed"
    );
    assert!(
        h.drained(),
        "no terminal round on a broken fallback promise"
    );
}

/// The broken promise is only visible after the round has run. The extra scan is what finds it,
/// so admitting on the pre-round classification alone would ship the document.
#[test]
fn a_surviving_acted_on_span_found_only_after_the_round_still_denies() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let rest = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "rest")
        .unwrap();
    let h = harness(
        vec![
            ("[REDACTED] tail rest".into(), Ok(vec![])),
            (
                "[REDACTED] tail rest".into(),
                Ok(vec![bleed(0..15, 10..15)]),
            ),
            ("[REDACTED] rest".into(), Ok(vec![uncovered(11..15)])),
            (format!("[REDACTED] {rest}"), Ok(vec![uncovered(0..10)])),
        ],
        true,
    );
    assert!(
        matches!(
            h.run(&session, "primary tail rest", SafetyNetPolicy::default()),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "a broken fallback promise must deny however late it shows up"
    );
    assert!(h.drained(), "the settled scan must happen");
}

/// The new round is `Resolve`-only. `Strict` and `Tolerant` never reach a terminal scan, and
/// their outcomes are untouched.
#[test]
fn strict_and_tolerant_fallbacks_are_unchanged() {
    for fallback in [SafetyNetFallback::Strict, SafetyNetFallback::Tolerant] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let h = harness(
            vec![
                ("raw tail".into(), Ok(vec![])),
                ("raw tail".into(), Ok(vec![uncovered(0..3)])),
            ],
            false,
        );
        let result = h.run(
            &session,
            "raw tail",
            SafetyNetPolicy::new(SafetyNetMode::Resolve, fallback),
        );
        match fallback {
            SafetyNetFallback::Strict => assert!(matches!(
                result,
                Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
            )),
            _ => assert_eq!(text_of(result.unwrap().0), "raw tail"),
        }
        assert!(h.drained(), "no terminal sweep outside the Redact fallback");
    }
}
