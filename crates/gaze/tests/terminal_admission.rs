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
/// `bravo`, and the terminal pass then sees `"<alpha>  charlie delta"` — two adjacent spaces with
/// a deletion seam between them, a never-flagged `charlie`, and a never-flagged `delta`.
struct Doc {
    session: Session,
    alpha: String,
    charlie: String,
    /// Text the terminal (fourth) sweep sees.
    terminal: String,
    /// `charlie`'s span in `terminal`.
    charlie_span: std::ops::Range<usize>,
    /// The two spaces around the deletion seam, in `terminal`.
    seam_span: std::ops::Range<usize>,
    /// The deletion seam itself, in `terminal`.
    seam: usize,
    lead: Vec<Step>,
}

impl Doc {
    const RAW: &'static str = "alpha bravo charlie delta";

    fn new() -> Self {
        let session = Session::new(Scope::Ephemeral).unwrap();
        // Pre-own the replacements so the random per-session token bytes are known here.
        let alpha = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "alpha")
            .unwrap();
        let charlie = session
            .tokenize_with_family("safety_net", &PiiClass::Name, "charlie")
            .unwrap();
        let resolved = format!("{alpha} bravo charlie delta");
        let terminal = format!("{alpha}  charlie delta");
        let lead = vec![
            (Self::RAW.into(), Ok(vec![])),
            (Self::RAW.into(), Ok(vec![uncovered(0..5)])),
            (
                resolved,
                Ok(vec![uncovered(alpha.len() + 1..alpha.len() + 6)]),
            ),
        ];
        let seam = alpha.len() + 1;
        Self {
            charlie_span: alpha.len() + 2..alpha.len() + 9,
            seam_span: alpha.len()..alpha.len() + 2,
            seam,
            alpha,
            charlie,
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
    let resolved = format!("{}  {} delta", doc.alpha, doc.charlie);
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(doc.charlie_span.clone())])),
        (resolved.clone(), Ok(vec![])),
    ]);
    let (clean, spans, report) = doc
        .run(&h)
        .expect("a fresh terminal finding must be resolved, not denied");
    let text = text_of(clean);
    assert_eq!(text, resolved);
    // Restore is exact for everything the fallback did not delete: `bravo` is gone by the
    // documented `Redact` contract, and both tokens still restore their own source bytes.
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        "alpha  charlie delta"
    );
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 12..19],
        "the terminal round's token must name charlie's ORIGINAL bytes"
    );
    assert!(
        report
            .suspects
            .iter()
            .any(|s| s.span == doc.charlie_span),
        "the terminal finding must be surfaced in the returned report"
    );
    assert!(h.drained(), "exactly one extra sweep after the terminal scan");
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
    let resolved = format!("{}  {} delta", doc.alpha, doc.charlie);
    let fresh = doc.alpha.len() + doc.charlie.len() + 3..doc.alpha.len() + doc.charlie.len() + 8;
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(doc.charlie_span.clone())])),
        (resolved.clone(), Ok(vec![uncovered(fresh.clone())])),
    ]);
    let (clean, _, report) = doc
        .run(&h)
        .expect("a finding with no round left must complete with an honest report");
    assert_eq!(text_of(clean), resolved);
    assert!(
        report.suspects.iter().any(|s| s.span == fresh),
        "the unactionable finding must be in the returned report"
    );
    assert!(h.drained(), "no second terminal round");
}

/// The deletion joined two fragments into a shape the model calls a name. Those bytes are partly
/// gaze's own artefact, so they are deleted once, and the deletion is proved by byte check.
#[test]
fn terminal_deletes_a_seam_manufactured_suspect_once() {
    let doc = Doc::new();
    let deleted = format!("{}charlie delta", doc.alpha);
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(doc.seam_span.clone())])),
        (deleted.clone(), Ok(vec![])),
    ]);
    let (clean, spans, _) = doc
        .run(&h)
        .expect("a seam-manufactured suspect must be deleted, not denied");
    assert_eq!(text_of(clean), deleted);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5]
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
/// surviving original bytes either side of the removed range, never the removed ones.
#[test]
fn a_suspect_that_only_abuts_the_seam_is_resolved_not_deleted() {
    let doc = Doc::new();
    // Starts exactly at the seam and runs to the end of `charlie`.
    let abutting = doc.seam..doc.charlie_span.end;
    let token = doc
        .session
        .tokenize_with_family("safety_net", &PiiClass::Name, " charlie")
        .unwrap();
    let resolved = format!("{} {token} delta", doc.alpha);
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
        [0..5, 11..19],
        "the abutting token must skip the removed range, not swallow it"
    );
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        "alpha  charlie delta"
    );
    assert!(h.drained());
    assert_eq!(h.actions(), [Action::Tokenize, Action::Redact, Action::Tokenize]);
}

/// Exactly one seam deletion is permitted. A second seam-crossing suspect after it means the
/// deletion is manufacturing shapes faster than the bound allows, so the document fails closed.
#[test]
fn a_second_seam_manufactured_suspect_denies() {
    let doc = Doc::new();
    let deleted = format!("{}charlie delta", doc.alpha);
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(doc.seam_span.clone())])),
        (
            deleted.clone(),
            Ok(vec![uncovered(doc.alpha.len() - 1..doc.alpha.len() + 1)]),
        ),
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
#[test]
fn two_seam_manufactured_suspects_in_one_report_deny() {
    let doc = Doc::new();
    let h = doc.harness(vec![(
        doc.terminal.clone(),
        Ok(vec![
            uncovered(doc.seam_span.clone()),
            uncovered(doc.alpha.len() - 1..doc.alpha.len() + 2),
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

/// The reversible round refuses (the report is not wholly supported). Denial stands, and the
/// `Redact` fallback is NOT re-entered: nothing extra is deleted.
#[test]
fn a_refused_terminal_round_denies_without_deleting() {
    let doc = Doc::new();
    let mut mismatch = uncovered(doc.charlie_span.clone());
    mismatch.kind = LeakKind::ClassMismatch {
        pipeline_class: PiiClass::Email,
        safety_net_class: PiiClass::Name,
    };
    let h = doc.harness(vec![(doc.terminal.clone(), Ok(vec![mismatch]))]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::OverlapConflict))
        ),
        "a refused terminal round must deny"
    );
    assert!(h.drained());
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact],
        "the refused round must not delete anything"
    );
}

/// The fallback's audit row says it redacted a span, and part of that span is still in the
/// output. That is the fallback breaking its own promise, and it must stay denied.
#[test]
fn a_surviving_acted_on_span_stays_denied() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let primary = "[REDACTED] tail rest";
    let deleted = "[REDACTED] rest";
    let h = harness(
        vec![
            (primary.into(), Ok(vec![])),
            (primary.into(), Ok(vec![bleed(0..15, 10..15)])),
            (deleted.into(), Ok(vec![uncovered(0..10)])),
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
    assert!(h.drained(), "no terminal round on a broken fallback promise");
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
