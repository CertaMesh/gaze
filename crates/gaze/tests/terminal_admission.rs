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

#[path = "support/stable_scan.rs"]
mod stable_scan;
use stable_scan::stable_scan;

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
            text,
            stable_scan(&expected),
            "each sweep must inspect its actual phase output through the stable scan view"
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
/// `"alpha bravo charlie delta"`: the second batch tokenizes `alpha`, the fallback redacts
/// `charlie`, and the terminal pass then sees `"<alpha> bravo [REDACTED:name] delta"`.
///
/// This fixture used to delete `charlie`, leaving `"<alpha> bravo  delta"` -- two adjacent spaces
/// with a deletion SEAM between them, which a later pass could read as a new, manufactured shape.
/// A marker leaves no seam: the fragments stay apart. What remains to pin is how the terminal
/// pass treats a suspect that touches the marker, which is three different cases -- wholly
/// inside it, straddling it, and merely abutting it.
struct Doc {
    session: Session,
    alpha: String,
    /// Text the terminal (fourth) sweep sees.
    terminal: String,
    /// `delta`'s span in `terminal`.
    delta_span: std::ops::Range<usize>,
    /// `bravo`'s span in `terminal`.
    bravo_span: std::ops::Range<usize>,
    /// The fallback's `[REDACTED:name]` marker, in `terminal`.
    marker_span: std::ops::Range<usize>,
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
        let marker = redaction_marker(&PiiClass::Name);
        let resolved = format!("{alpha} bravo charlie delta");
        let terminal = format!("{alpha} bravo {marker} delta");
        let lead = vec![
            (Self::RAW.into(), Ok(vec![])),
            (Self::RAW.into(), Ok(vec![uncovered(0..5)])),
            (
                resolved,
                Ok(vec![uncovered(alpha.len() + 7..alpha.len() + 14)]),
            ),
        ];
        let marker_start = alpha.len() + 7;
        let marker_end = marker_start + marker.len();
        Self {
            delta_span: marker_end + 1..marker_end + 6,
            bravo_span: alpha.len() + 1..alpha.len() + 6,
            marker_span: marker_start..marker_end,
            alpha,
            terminal,
            session,
            lead,
        }
    }

    fn marker(&self) -> String {
        redaction_marker(&PiiClass::Name)
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
    let resolved = format!("{} bravo {} {delta}", doc.alpha, doc.marker());
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
    // Restore is exact for everything the fallback did not redact: `charlie` is gone by the
    // documented `Redact` contract, its marker stays where it was, and both tokens still restore
    // their own source bytes.
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        format!("alpha bravo {} delta", doc.marker())
    );
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 12..19, 20..25],
        "the marker stands for charlie's ORIGINAL bytes and the round's token for delta's"
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
        "second batch, fallback redaction, then the terminal round"
    );
}

/// The extra round is bounded at one. A finding that appears only after it is reported honestly
/// and shipped, because nothing in the pipeline is permitted to act on it any more.
#[test]
fn terminal_round_happens_at_most_once_and_reports_what_it_could_not_act_on() {
    let doc = Doc::new();
    let delta = doc.token("delta");
    let resolved = format!("{} bravo {} {delta}", doc.alpha, doc.marker());
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

/// A suspect wholly inside the fallback's own marker is the net re-flagging gaze's output, not
/// the document: `REDACTED` is a capitalised word a NER model reads as an organization. It is
/// dropped as already protected, the document completes, and the marker is not touched again.
///
/// Replaces the seam-deletion case this file used to open with. That case cannot arise any more:
/// a marker leaves no seam, so there is no manufactured shape for a terminal pass to find.
#[test]
fn a_terminal_suspect_inside_the_marker_is_already_protected() {
    let doc = Doc::new();
    let h = doc.harness(vec![(
        doc.terminal.clone(),
        Ok(vec![uncovered(doc.marker_span.clone())]),
    )]);
    let (clean, spans, _) = doc
        .run(&h)
        .expect("re-flagging a marker must not deny the document");
    assert_eq!(text_of(clean), doc.terminal, "nothing was redacted twice");
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 12..19]
    );
    assert!(h.drained(), "no terminal round ran over a marker");
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact],
        "second batch, fallback redaction, and nothing else"
    );
}

/// A suspect straddling the marker and real text denies. Dropping it because half of it is a
/// marker would ship the other half raw: a net that reports `[REDACTED:name] delta` has flagged
/// `delta`. It is not a broken promise either -- the marker IS the fallback keeping its word --
/// so what denies it is the ordinary rule for any suspect that overlaps a manifest entry.
///
/// The mutation this must catch: relax the marker guard from containment to overlap, and this
/// document ships with `delta` raw.
#[test]
fn a_terminal_suspect_straddling_the_marker_denies() {
    let doc = Doc::new();
    let straddle = doc.marker_span.start..doc.delta_span.end;
    let h = doc.harness(vec![(doc.terminal.clone(), Ok(vec![uncovered(straddle)]))]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "a straddling suspect must fail closed, never ship its real half"
    );
    assert!(h.drained());
}

/// A fresh finding elsewhere in the report does not buy a straddling suspect through. The
/// reversible round is all-or-nothing over the report, so one suspect it may not act on denies
/// the document even beside one it could have resolved.
#[test]
fn a_straddling_suspect_denies_even_beside_a_resolvable_one() {
    let doc = Doc::new();
    let h = doc.harness(vec![(
        doc.terminal.clone(),
        Ok(vec![
            uncovered(doc.bravo_span.clone()),
            uncovered(doc.marker_span.start - 1..doc.marker_span.end),
        ]),
    )]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "the resolvable finding must not carry the straddling one through"
    );
    assert!(h.drained());
}

/// Abutting the marker is not touching it: no byte of the suspect is gaze's output, so it is a
/// fresh finding and takes the reversible round. The token must name the suspect's own original
/// bytes -- the space and `delta` -- and never reach back into what the marker stands for.
///
/// Pins the containment boundary from the other side: an off-by-one that counted an abutting
/// span as overlapping would deny this document.
#[test]
fn a_suspect_that_only_abuts_the_marker_is_resolved() {
    let doc = Doc::new();
    let abutting = doc.marker_span.end..doc.delta_span.end;
    let token = doc.token(" delta");
    let resolved = format!("{} bravo {}{token}", doc.alpha, doc.marker());
    let h = doc.harness(vec![
        (doc.terminal.clone(), Ok(vec![uncovered(abutting)])),
        (resolved.clone(), Ok(vec![])),
    ]);
    let (clean, spans, _) = doc
        .run(&h)
        .expect("an abutting suspect must be resolved, not denied");
    let text = text_of(clean);
    assert_eq!(text, resolved);
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        [0..5, 12..19, 19..25],
        "the abutting token names its own original bytes and stops at the marker"
    );
    assert_eq!(
        doc.session.restore_strict_text(&text).unwrap(),
        format!("alpha bravo {} delta", doc.marker())
    );
    assert!(h.drained());
    assert_eq!(
        h.actions(),
        [Action::Tokenize, Action::Redact, Action::Tokenize]
    );
}

/// One suspect inside the marker is dropped as protected; that must not excuse a straddling
/// sibling in the same report. The two guards are independent per suspect.
#[test]
fn a_protected_marker_suspect_does_not_excuse_a_straddling_sibling() {
    let doc = Doc::new();
    let h = doc.harness(vec![(
        doc.terminal.clone(),
        Ok(vec![
            uncovered(doc.marker_span.clone()),
            uncovered(doc.marker_span.start..doc.delta_span.end),
        ]),
    )]);
    assert!(
        matches!(
            doc.run(&h),
            Err(Error::SafetyNetFallback(FallbackReason::ResidualSuspect))
        ),
        "the protected suspect must not carry its straddling sibling through"
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
    let redacted = format!(
        "[REDACTED] alpha {} charlie",
        redaction_marker(&PiiClass::Name)
    );
    let mut mismatch = uncovered(17..22);
    mismatch.kind = LeakKind::ClassMismatch {
        pipeline_class: PiiClass::Email,
        safety_net_class: PiiClass::Name,
    };
    let h = harness(
        vec![
            (primary.into(), Ok(vec![])),
            (primary.into(), Ok(vec![mismatch])),
            (redacted, Ok(vec![bleed(0..16, 10..16)])),
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
        // The primary pass's own `[REDACTED]`, then the fallback's marker. Nothing after.
        [Action::Redact, Action::Redact],
        "the refused round must not redact anything"
    );
}

/// A terminal suspect whose class-mismatch claim contradicts the manifest cannot be judged.
#[test]
fn a_contradictory_terminal_class_mismatch_denies() {
    let doc = Doc::new();
    let mut mismatch = uncovered(doc.delta_span.clone());
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
        "a contradictory terminal claim must fail closed"
    );
    assert!(h.drained());
}

/// The fallback's audit row says it redacted a span, and part of that span is still in the
/// output. That is the fallback breaking its own promise, and it must stay denied.
///
/// The audit row names the WHOLE suspect, `primary tail`, while the redactor acts only on its
/// uncovered gap. The primary pass's one-way `[REDACTED]` for `primary` therefore survives inside
/// the promised range: a genuine broken promise.
///
/// The terminal suspect spans both the primary `[REDACTED]` and the fallback's own marker, with
/// its gap on the plain text after them. That makes it judgeable, so the survivor clause is what
/// decides.
///
/// What this does NOT pin: that the fallback's own marker is never read as a survivor. The primary
/// `[REDACTED]` survivor denies this document either way, so that half is pinned directly by
/// `fallback_promise_does_not_count_its_own_marker_as_a_survivor` in `pipeline.rs`.
#[test]
fn a_surviving_acted_on_span_stays_denied() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let primary = "[REDACTED] tail rest";
    let marker = redaction_marker(&PiiClass::Name);
    let redacted = format!("[REDACTED]{marker} rest");
    let end = redacted.len();
    let h = harness(
        vec![
            (primary.into(), Ok(vec![])),
            (primary.into(), Ok(vec![bleed(0..15, 10..15)])),
            (redacted, Ok(vec![bleed(0..end, end - 5..end)])),
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
    assert_eq!(
        h.actions(),
        // The primary pass's own `[REDACTED]`, then the fallback's marker. Nothing after.
        [Action::Redact, Action::Redact],
        "a document refused over a broken promise must not be redacted from first"
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
    let marker = redaction_marker(&PiiClass::Name);
    let redacted = format!("[REDACTED]{marker} rest");
    let rest_at = redacted.len() - 4;
    let h = harness(
        vec![
            ("[REDACTED] tail rest".into(), Ok(vec![])),
            (
                "[REDACTED] tail rest".into(),
                Ok(vec![bleed(0..15, 10..15)]),
            ),
            (redacted, Ok(vec![uncovered(rest_at..rest_at + 4)])),
            (
                format!("[REDACTED]{marker} {rest}"),
                Ok(vec![uncovered(0..10)]),
            ),
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
