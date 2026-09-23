//! Word-like safety-net suspects that start or end inside a word are never acted on.
//!
//! The explorer bundle on 9a3a788 showed `Passwort` -> `<Name_14>wort`, a random credential
//! string losing its last letter to a name token, and `IBAN` split into three adjacent name
//! tokens. Tokenizing or deleting part of a word protects nothing whole and mangles the text the
//! agent reads, so every stage that acts on suspects (first pass, second batch, terminal round,
//! `Redact` mode and the `Redact` fallback) leaves such a suspect's bytes alone and records an
//! `UnactionableSubword` telemetry row instead.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, LeakKind, LeakReport,
    LeakReportTelemetry, LeakSuspect, PiiClass, Pipeline, RawDocument, SafetyNet, SafetyNetContext,
    SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

type Locate = fn(&str) -> Option<Range<usize>>;

#[derive(Clone)]
struct Probe {
    locate: Locate,
    class: PiiClass,
    /// Report this kind instead of the manifest diff (used to force a resolve refusal).
    forced: Option<LeakKind>,
}

fn probe(locate: Locate) -> Probe {
    Probe {
        locate,
        class: PiiClass::Name,
        forced: None,
    }
}

/// Reports `script[call]` on each check; the last entry repeats.
#[derive(Clone)]
struct ScriptedNet {
    script: Vec<Vec<Probe>>,
    calls: Arc<AtomicUsize>,
}

impl ScriptedNet {
    fn new(script: Vec<Vec<Probe>>) -> Self {
        Self {
            script,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn every_call(probes: Vec<Probe>) -> Self {
        Self::new(vec![probes])
    }
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
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let probes = &self.script[call.min(self.script.len() - 1)];
        let mut out = Vec::new();
        for probe in probes {
            let Some(span) = (probe.locate)(clean_text) else {
                continue;
            };
            let kind = match &probe.forced {
                Some(kind) => kind.clone(),
                None => match context.manifest.diff_against(&span, &probe.class) {
                    Some(kind) => kind,
                    None => continue,
                },
            };
            out.push(LeakSuspect::new(
                span,
                probe.class.clone(),
                self.id(),
                Some(0.9),
                kind,
                "person",
                None,
            ));
        }
        Ok(out)
    }
}

#[derive(Clone)]
struct EmailDetector;

impl Detector for EmailDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        let needle = "alice@example.invalid";
        input
            .find(needle)
            .map(|start| {
                Detection::new(start..start + needle.len(), PiiClass::Email, "fixed-email")
            })
            .into_iter()
            .collect()
    }
}

fn find(text: &str, needle: &str, within: Range<usize>) -> Option<Range<usize>> {
    text.find(needle)
        .map(|start| start + within.start..start + within.end)
}

fn pipeline(net: ScriptedNet) -> Pipeline {
    Pipeline::builder()
        .detector(EmailDetector)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(net)
        .build()
        .expect("pipeline")
}

fn clean(
    net: ScriptedNet,
    raw: &str,
    policy: SafetyNetPolicy,
) -> (String, LeakReport, Session, Pipeline) {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = pipeline(net);
    let (document, _, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw.to_string()),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            policy,
        )
        .expect("document completes");
    let CleanDocument::Text(text) = document else {
        panic!("expected text");
    };
    (text, report, session, pipeline)
}

fn resolve() -> SafetyNetPolicy {
    SafetyNetPolicy::default()
}

fn subword_rows(report: &LeakReport) -> Vec<(Range<usize>, PiiClass)> {
    report
        .telemetry
        .iter()
        .filter_map(|event| match event {
            LeakReportTelemetry::UnactionableSubword { span, class, .. } => {
                Some((span.clone(), class.clone()))
            }
            _ => None,
        })
        .collect()
}

fn assert_restores(pipeline: &Pipeline, session: &Session, clean: &str, raw: &str) {
    assert_eq!(
        pipeline
            .restore_strict_text(session, clean)
            .expect("restore"),
        raw
    );
}

#[test]
fn passwort_prefix_is_not_tokenized_on_the_first_pass() {
    let raw = "Bitte Passwort ein";
    let net = ScriptedNet::every_call(vec![probe(|t| find(t, "Passwort", 0..4))]);
    let (text, report, session, pipeline) = clean(net, raw, resolve());
    assert_eq!(text, raw);
    assert_eq!(subword_rows(&report), vec![(6..10, PiiClass::Name)]);
    assert_restores(&pipeline, &session, &text, raw);
}

#[test]
fn trailing_letter_of_a_credential_string_is_not_tokenized() {
    let raw = "key gh78Klm91NpQr23St4UvWx56YzaBcDeF end";
    let net = ScriptedNet::every_call(vec![probe(|t| {
        find(t, "gh78Klm91NpQr23St4UvWx56YzaBcDeF", 31..32)
    })]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(text, raw);
    assert_eq!(subword_rows(&report).len(), 1);
}

#[test]
fn suffix_that_ends_on_a_boundary_but_starts_inside_a_word_is_unactionable() {
    let raw = "Bitte Passwort ein";
    let net = ScriptedNet::every_call(vec![probe(|t| find(t, "Passwort", 4..8))]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(text, raw);
    assert_eq!(subword_rows(&report), vec![(10..14, PiiClass::Name)]);
}

#[test]
fn adjacent_subword_suspects_on_one_word_are_all_unactionable() {
    let raw = "using her Canadian IBAN now";
    let net = ScriptedNet::every_call(vec![
        probe(|t| find(t, "IBAN", 0..1)),
        probe(|t| find(t, "IBAN", 1..3)),
        probe(|t| find(t, "IBAN", 3..4)),
    ]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(text, raw);
    assert_eq!(subword_rows(&report).len(), 3);
}

#[test]
fn a_standalone_initial_still_resolves() {
    // A single letter bounded by non-word characters is a whole word, often a name initial.
    let raw = "Gruss J. Smith";
    let net = ScriptedNet::every_call(vec![probe(|t| find(t, "J.", 0..1))]);
    let (text, report, session, pipeline) = clean(net, raw, resolve());
    assert!(!text.contains("J."), "{text}");
    assert!(subword_rows(&report).is_empty());
    assert_restores(&pipeline, &session, &text, raw);
}

#[test]
fn whole_words_still_resolve_next_to_an_unactionable_subword() {
    let raw = "Al und Anna Meier mit Passwort";
    let net = ScriptedNet::every_call(vec![
        probe(|t| find(t, "Al ", 0..2)),
        probe(|t| find(t, "Anna Meier", 0..10)),
        probe(|t| find(t, "Passwort", 0..4)),
    ]);
    let (text, report, session, pipeline) = clean(net, raw, resolve());
    assert!(!text.contains("Al "), "two-letter word resolves: {text}");
    assert!(!text.contains("Anna Meier"), "whole name resolves: {text}");
    assert!(text.ends_with(" mit Passwort"), "{text}");
    // One row per distinct pass text: the re-run sees `Pass` at offsets shifted by the new tokens.
    let rows = subword_rows(&report);
    assert!(!rows.is_empty());
    assert!(rows
        .iter()
        .all(|(span, class)| span.len() == 4 && *class == PiiClass::Name));
    assert_restores(&pipeline, &session, &text, raw);
}

#[test]
fn strict_fallback_refuses_a_subword_residual_instead_of_shipping_it() {
    // A net that does not decode whole words flags the name inside a genitive. It is not cut,
    // but `Strict` promises to reject any residual suspect, so the document is refused.
    let raw = "Das ist Meiers Auto";
    let net = ScriptedNet::every_call(vec![probe(|t| find(t, "Meiers", 0..5))]);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let result = pipeline(net).clean_with_safety_net_policy_detect_context(
        &session,
        RawDocument::Text(raw.to_string()),
        &[gaze::LocaleTag::Global],
        &gaze::DictionaryBundle::default(),
        SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict),
    );
    assert!(
        matches!(result, Err(gaze::Error::SafetyNetFallback(_))),
        "{result:?}"
    );
}

#[test]
fn identifier_classes_keep_resolving_inside_words() {
    let raw = "Kunde ID12345 in 8001 Zurich";
    let postcode = PiiClass::custom("postcode").expect("valid class");
    let net = ScriptedNet::every_call(vec![
        Probe {
            locate: |t| find(t, "ID12345", 2..7),
            class: PiiClass::custom("customer_id").expect("valid class"),
            forced: None,
        },
        Probe {
            locate: |t| find(t, "8001", 0..4),
            class: postcode,
            forced: None,
        },
    ]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert!(!text.contains("12345"), "{text}");
    assert!(!text.contains("8001"), "{text}");
    assert!(subword_rows(&report).is_empty());
}

#[test]
fn second_batch_does_not_tokenize_a_subword_found_by_the_re_run() {
    let raw = "Bitte Anna Passwort ein";
    // First pass reports nothing; every re-run reports a whole name and a sub-word.
    let net = ScriptedNet::new(vec![
        vec![],
        vec![
            probe(|t| find(t, "Anna", 0..4)),
            probe(|t| find(t, "Passwort", 0..4)),
        ],
    ]);
    let (text, report, session, pipeline) = clean(net, raw, resolve());
    assert!(!text.contains("Anna"), "{text}");
    assert!(text.ends_with(" Passwort ein"), "{text}");
    assert!(!subword_rows(&report).is_empty());
    assert_restores(&pipeline, &session, &text, raw);
}

fn forced_refusal() -> Probe {
    Probe {
        locate: |t| find(t, "residue", 0..7),
        class: PiiClass::Name,
        forced: Some(LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Email,
            safety_net_class: PiiClass::Name,
        }),
    }
}

#[test]
fn redact_fallback_redacts_the_residual_but_not_a_subword() {
    let raw = "Bitte residue Passwort ein";
    let net = ScriptedNet::new(vec![
        vec![forced_refusal(), probe(|t| find(t, "Passwort", 0..4))],
        vec![],
    ]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(
        text,
        format!(
            "Bitte {} Passwort ein",
            gaze::redaction_marker(&PiiClass::Name)
        )
    );
    assert!(!subword_rows(&report).is_empty());
}

#[test]
fn terminal_round_admits_a_subword_instead_of_tokenizing_it() {
    let raw = "Bitte residue Passwort ein";
    let net = ScriptedNet::new(vec![
        vec![forced_refusal()],
        vec![probe(|t| find(t, "Passwort", 0..4))],
    ]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(
        text,
        format!(
            "Bitte {} Passwort ein",
            gaze::redaction_marker(&PiiClass::Name)
        )
    );
    assert!(!subword_rows(&report).is_empty());
    // Located in the output rather than hard-coded: the marker is wider than the gap deleting
    // left, so "Passwort" sits further right than it used to.
    let at = text.find("Passwort").expect("the subword survives");
    assert!(
        report.suspects.iter().any(|s| s.span == (at..at + 4)),
        "the admitted finding stays in the report"
    );
}

/// Deleting `residue` out of `Pasresiduewort` used to glue `Pas` and `wort` into `Paswort` -- a
/// word that exists only because gaze removed what sat between the fragments, and that the next
/// net pass then flagged as a new finding. That seam-manufactured class is what the marker
/// removes: the marker keeps the fragments apart, so the manufactured word never exists for any
/// pass to see.
///
/// The mutation this must catch: write `""` instead of the marker, and `Paswort` reappears in
/// the output and in the net's input.
#[test]
fn a_redaction_marker_leaves_no_seam_for_a_subword_to_cross() {
    let raw = "Pasresiduewort ein";
    let net = ScriptedNet::new(vec![
        vec![forced_refusal()],
        vec![probe(|t| find(t, "Paswort", 1..5))],
    ]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert_eq!(
        text,
        format!("Pas{}wort ein", gaze::redaction_marker(&PiiClass::Name))
    );
    assert!(
        !text.contains("Paswort"),
        "the manufactured word must not exist in the output"
    );
    assert!(
        subword_rows(&report).is_empty(),
        "no pass may find a seam-crossing subword when there is no seam"
    );
}

#[test]
fn redact_mode_redacts_whole_words_and_leaves_subwords() {
    let raw = "Anna und Passwort";
    let net = ScriptedNet::every_call(vec![
        probe(|t| find(t, "Anna", 0..4)),
        probe(|t| find(t, "Passwort", 0..4)),
    ]);
    let (text, report, _, _) = clean(
        net,
        raw,
        SafetyNetPolicy::new(SafetyNetMode::Redact, SafetyNetFallback::Redact),
    );
    let marker = gaze::redaction_marker(&PiiClass::Name);
    assert_eq!(text, format!("{marker} und Passwort"));
    // The subword row names the span in the text the net was given, which is the raw document.
    assert_eq!(subword_rows(&report), vec![(9..13, PiiClass::Name)]);
}

#[test]
fn observe_mode_reports_subwords_as_plain_suspects() {
    let raw = "Bitte Passwort ein";
    let net = ScriptedNet::every_call(vec![probe(|t| find(t, "Passwort", 0..4))]);
    let (text, report, _, _) = clean(
        net,
        raw,
        SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Strict),
    );
    assert_eq!(text, raw);
    assert_eq!(report.suspects.len(), 1);
    assert!(subword_rows(&report).is_empty(), "observe acts on nothing");
}

#[test]
fn a_token_edge_is_a_word_boundary() {
    // `<token>Anna`: the gap after the email token starts right after `>`.
    let raw = "alice@example.invalidAnna ruft an";
    let net = ScriptedNet::every_call(vec![probe(|t| {
        let end = t.find(" ruft")?;
        Some(end - 4..end)
    })]);
    let (text, report, session, pipeline) = clean(net, raw, resolve());
    assert!(!text.contains("Anna"), "{text}");
    assert!(subword_rows(&report).is_empty());
    assert_restores(&pipeline, &session, &text, raw);
}

#[test]
fn a_gap_after_a_token_that_ends_inside_a_word_is_unactionable() {
    // The suspect covers the email token plus ` An` of `Anna`: its uncovered gap ends mid-word.
    let raw = "alice@example.invalid Anna ruft an";
    let net = ScriptedNet::every_call(vec![probe(|t| {
        let name = t.find(" Anna")?;
        Some(0..name + 3)
    })]);
    let (text, report, _, _) = clean(net, raw, resolve());
    assert!(text.ends_with(" Anna ruft an"), "{text}");
    assert_eq!(subword_rows(&report).len(), 1);
}
