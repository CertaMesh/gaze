//! Repeat-value sweep (solo todo 3849) at the library boundary.
//!
//! Two synthetic recognizers stand in for the evidence tiers: `Header` is a
//! rule (it finds the name in a `From:` header, like `email.header.name`),
//! `Ner` reports its spans with the learned-NER source. Synthetic names only.

use std::sync::{Arc, Mutex};

use gaze::*;
use regex::Regex;

/// Finds `From: <name> <` and reports the name, as a regex rule would.
struct Header;

impl Recognizer for Header {
    fn id(&self) -> &str {
        "header"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        let pattern = Regex::new(r"(?m)^From: ([^<\n]+?) <").unwrap();
        Ok(pattern
            .captures_iter(input)
            .map(|caps| candidate(caps.get(1).unwrap().range(), "header", "header"))
            .collect())
    }
}

/// Reports every occurrence of its needles as a learned NER span.
struct Ner(Vec<&'static str>);

impl Recognizer for Ner {
    fn id(&self) -> &str {
        "ner"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Learned
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .0
            .iter()
            .flat_map(|needle| input.match_indices(needle).map(|(at, m)| at..at + m.len()))
            .map(|span| candidate(span, "ner", "ner/test"))
            .collect())
    }
}

/// Stands in for a model-confirmed DOB without loading the ONNX bundle.
struct LearnedDob(PiiClass);

impl Recognizer for LearnedDob {
    fn id(&self) -> &str {
        "dob.gliner"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Learned
    }
    fn supported_class(&self) -> &PiiClass {
        &self.0
    }
    fn token_family(&self) -> &str {
        "birth_date"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        if !input.starts_with("Patient record: ") {
            return Ok(vec![]);
        }
        let date = "14.03.1987";
        let Some(start) = input.find(date) else {
            return Ok(vec![]);
        };
        Ok(vec![Candidate::new(
            start..start + date.len(),
            self.supported_class().clone(),
            self.id(),
            0.9,
            0,
            None,
            self.token_family(),
            self.id(),
            ConflictTier::None,
            vec![],
        )])
    }
}

fn candidate(span: std::ops::Range<usize>, id: &str, source: &str) -> Candidate {
    Candidate::new(
        span,
        PiiClass::Name,
        id,
        0.9,
        0,
        None,
        "counter",
        source,
        ConflictTier::None,
        vec![],
    )
}

#[derive(Clone, Default)]
struct Audit(Arc<Mutex<Vec<RedactionEntry>>>);

impl RedactionLogger for Audit {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

fn pipeline(ner: Vec<&'static str>, audit: Audit) -> Pipeline {
    Pipeline::builder()
        .recognizer(Header)
        .recognizer(Ner(ner))
        .rule(DefaultRule::new(Action::Tokenize))
        .redaction_logger(audit)
        .build()
        .unwrap()
}

fn clean(pipeline: &Pipeline, session: &Session, input: &str) -> String {
    let CleanDocument::Text(text) = pipeline
        .redact(session, RawDocument::Text(input.into()))
        .unwrap()
    else {
        panic!("text in, text out");
    };
    assert_eq!(
        session.restore_strict_text(&text).unwrap(),
        input,
        "restore must be byte-exact"
    );
    text
}

fn tokens(text: &str) -> Vec<String> {
    Regex::new(r"<[0-9a-f]{8}:Name_\d+>")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

const HEADER: &str = "From: Maria Schneider <m@example.invalid>\n";

fn swept(body: &str) -> String {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let text = clean(
        &pipeline(vec![], Audit::default()),
        &session,
        &format!("{HEADER}{body}"),
    );
    text[text.find('\n').unwrap() + 1..].to_string()
}

#[test]
fn recall_probes_whitespace_classes_and_case() {
    for body in [
        "Maria\u{00A0}Schneider called.",
        "Maria\u{202F}Schneider called.",
        "Maria\nSchneider called.",
        "Maria \t Schneider called.",
        "MARIA SCHNEIDER called.",
        "maria schneider called.",
    ] {
        let out = swept(body);
        assert_eq!(tokens(&out).len(), 1, "{body:?} -> {out:?}");
        assert!(out.ends_with(" called."), "{body:?} -> {out:?}");
    }
}

#[test]
fn recall_probe_length_changing_case_fold() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = "From: İpek Yildiz <i@example.invalid>\nSIGNED: İPEK YILDIZ.";
    let out = clean(&pipeline(vec![], Audit::default()), &session, input);
    assert!(
        out.ends_with("SIGNED: <") || out.contains("SIGNED: <"),
        "{out}"
    );
    assert!(!out.contains("YILDIZ"), "{out}");
}

#[test]
fn recall_probe_hyphenated_surname() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = "From: Jonas Albrecht-Quaye <j@example.invalid>\nregards, jonas albrecht-quaye\nDear Albrecht-Quaye,";
    let out = clean(&pipeline(vec![], Audit::default()), &session, input);
    assert!(!out.to_lowercase().contains("quaye"), "{out}");
}

#[test]
fn precision_probes_stay_raw() {
    for body in [
        // Inside a longer word on either edge.
        "Mariasch and xMaria and Schneiderei.",
        // Inside URLs.
        "See https://example.invalid/Maria/Schneider and www.Schneider.invalid today.",
        // Lower-case single part: the stated leak.
        "thanks maria",
    ] {
        let out = swept(body);
        assert_eq!(out, body, "{body:?}");
    }
}

#[test]
fn precision_probe_common_word_parts() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = "From: Rose May <r@example.invalid>\nThe Rose garden opens in May.";
    let out = clean(&pipeline(vec![], Audit::default()), &session, input);
    assert!(out.ends_with("\nThe Rose garden opens in May."), "{out}");
}

#[test]
fn learned_values_do_not_propagate() {
    // NER finds the capitalised copy only; the lower-case copy must stay raw,
    // because NER-found values never seed the sweep.
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = "Anna Weber wrote. later anna weber wrote again.";
    let out = clean(
        &pipeline(vec!["Anna Weber"], Audit::default()),
        &session,
        input,
    );
    assert!(out.contains("later anna weber wrote"), "{out}");
}

#[test]
fn model_confirmed_birth_date_does_not_sweep_a_later_business_date() {
    let session = Session::new(Scope::Conversation("dob-evidence".into())).unwrap();
    let pipeline = Pipeline::builder()
        .recognizer(LearnedDob(PiiClass::Custom("birth_date".into())))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();

    let first = clean(&pipeline, &session, "Patient record: 14.03.1987.");
    assert!(!first.contains("14.03.1987"), "{first}");
    let second = clean(&pipeline, &session, "Account opened: 14.03.1987.");
    assert_eq!(second, "Account opened: 14.03.1987.");
}

#[test]
fn rule_values_propagate_across_documents_of_one_session() {
    let session = Session::new(Scope::Conversation("c".into())).unwrap();
    let p = pipeline(vec![], Audit::default());
    let first = clean(&p, &session, HEADER);
    let second = clean(&p, &session, "Maria Schneider asked for a callback.");
    let third = clean(&p, &session, "maria schneider asked again.");
    assert_eq!(tokens(&second), tokens(&first), "{second}");
    assert_eq!(tokens(&third).len(), 1, "{third}");
    assert_ne!(
        tokens(&third),
        tokens(&first),
        "variant gets a sibling token"
    );
}

#[test]
fn swept_copy_covers_the_union_with_an_ner_fragment() {
    // NER tags only `maria schneide` in the lower-case copy, which used to
    // leave a raw `r` behind the token. The swept copy encloses it and wins.
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = format!("{HEADER}hi, this is maria schneider again.");
    let out = clean(
        &pipeline(vec!["maria schneide"], Audit::default()),
        &session,
        &input,
    );
    let body = &out[out.find('\n').unwrap() + 1..];
    assert_eq!(tokens(body).len(), 1, "{out}");
    assert!(
        body.starts_with("hi, this is <") && body.ends_with("> again."),
        "{out}"
    );
}

#[test]
fn swept_copies_write_manifest_sweep_audit_rows() {
    let audit = Audit::default();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = format!("{HEADER}Maria Schneider, maria schneider, Maria.");
    clean(&pipeline(vec![], audit.clone()), &session, &input);
    let rows = audit.0.lock().unwrap().clone();
    let swept = rows
        .iter()
        .filter(|row| row.decided_by == ConflictTier::ManifestSweep)
        .collect::<Vec<_>>();
    assert_eq!(swept.len(), 3, "{rows:#?}");
    for row in &swept {
        assert_eq!(row.provenance_stage.as_deref(), Some("manifest_sweep"));
        assert_eq!(row.recognizer_id.as_deref(), Some("manifest_sweep"));
        assert!(!row.conflict_loser);
    }
    let links = swept
        .iter()
        .map(|row| row.provenance_merged_from.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        links,
        [
            "manifest_sweep:exact",
            "manifest_sweep:variant",
            "manifest_sweep:part"
        ]
    );
}

#[test]
fn session_blob_carries_evidence_across_export_and_import() {
    let session = Session::new(Scope::Conversation("c".into())).unwrap();
    let p = pipeline(vec![], Audit::default());
    let first = clean(&p, &session, HEADER);
    let imported = Session::import(session.export().unwrap()).unwrap();
    let second = clean(&p, &imported, "Maria Schneider asked.");
    assert_eq!(tokens(&second), tokens(&first), "{second}");
}

#[test]
fn precision_probe_occupational_and_verb_surnames() {
    // German capitalises every noun and English every sentence start, so a
    // title-case surname part is not enough evidence when the surname is
    // also an everyday word.
    for (header, body) in [
        (
            "From: Thomas Richter <t@example.invalid>\n",
            "Der Richter hat entschieden.",
        ),
        (
            "From: Anna Bauer <a@example.invalid>\nFrom: Paul Fischer <p@example.invalid>\n",
            "Der Bauer verkaufte dem Fischer Eier.",
        ),
        (
            "From: Hugh Grant <h@example.invalid>\n",
            "Grant access to the repo.",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let out = clean(
            &pipeline(vec![], Audit::default()),
            &session,
            &format!("{header}{body}"),
        );
        assert!(out.ends_with(body), "{out:?}");
    }
}

/// Solo todo 3897: the cue-found value carries its honorific, so every bare
/// spelling of the name is a run of its parts. All-caps, lower-case, mixed
/// case and NBSP copies each ship as one token and restore exactly.
#[test]
fn recall_probes_honorific_header_name_in_every_case() {
    let header = "From: Herr Tobias Brenner <t@example.invalid>\n";
    for body in [
        "later: tobias brenner called.",
        "later: TOBIAS BRENNER called.",
        "later: tObIaS bReNnEr called.",
        "later: Tobias\u{00A0}Brenner called.",
        "later: TOBIAS\u{202F}BRENNER called.",
        "later: BRENNER called.",
        "later: Brenner called.",
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let out = clean(
            &pipeline(vec![], Audit::default()),
            &session,
            &format!("{header}{body}"),
        );
        let out_body = &out[out.find('\n').unwrap() + 1..];
        assert_eq!(tokens(out_body).len(), 1, "{body:?} -> {out:?}");
        assert!(
            out_body.starts_with("later: <") && out_body.ends_with("> called."),
            "{body:?} -> {out:?}"
        );
    }
}

#[test]
fn recall_probes_glued_name_continuations() {
    for (header, body) in [
        (
            "From: Jorunn Vasquez <j@example.invalid>\n",
            "ping jorunn vasquez-ellery now",
        ),
        (
            "From: Jorunn Vasquez <j@example.invalid>\n",
            "ping JORUNN VASQUEZ-ELLERY now",
        ),
        (
            "From: Jorunn Vasquez <j@example.invalid>\n",
            "ping Ellery-Vasquez now",
        ),
        (
            "From: Siobhan O'Brien <s@example.invalid>\n",
            "ping SIOBHAN O'BRIEN now",
        ),
        (
            "From: Siobhan O'Brien <s@example.invalid>\n",
            "ping O'BRIEN now",
        ),
        (
            "From: Siobhan Brien <s@example.invalid>\n",
            "ping O\u{2019}Brien now",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let out = clean(
            &pipeline(vec![], Audit::default()),
            &session,
            &format!("{header}{body}"),
        );
        let out_body = &out[out.find('\n').unwrap() + 1..];
        assert_eq!(tokens(out_body).len(), 1, "{body:?} -> {out:?}");
        assert!(
            out_body.starts_with("ping <") && out_body.ends_with("> now"),
            "{body:?} -> {out:?}"
        );
    }
}

#[test]
fn precision_probes_runs_and_continuations() {
    for (header, body) in [
        // An adjacent run without a distinctive part: `grace` and `hall` are
        // common words, so the venue is not a name.
        (
            "From: Grace Hall Kowalski <g@example.invalid>\n",
            "book the grace hall for friday.",
        ),
        // A lone lower-case part stays raw (stated leak, unchanged).
        (
            "From: Herr Tobias Brenner <t@example.invalid>\n",
            "thanks tobias",
        ),
        // Mixed-case single part is not a spelling of the part.
        (
            "From: Herr Tobias Brenner <t@example.invalid>\n",
            "thanks tOBIAS",
        ),
    ] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let out = clean(
            &pipeline(vec![], Audit::default()),
            &session,
            &format!("{header}{body}"),
        );
        assert!(out.ends_with(&format!("\n{body}")), "{body:?} -> {out:?}");
    }
    // A possessive stays outside the token.
    let session = Session::new(Scope::Ephemeral).unwrap();
    let out = clean(
        &pipeline(vec![], Audit::default()),
        &session,
        "From: Jorunn Vasquez <j@example.invalid>\nVasquez's desk",
    );
    assert!(out.ends_with(">'s desk"), "{out}");
}

#[test]
fn honorific_name_runs_propagate_to_a_later_turn() {
    let session = Session::new(Scope::Conversation("c3897".into())).unwrap();
    let audit = Audit::default();
    let p = pipeline(vec![], audit.clone());
    clean(
        &p,
        &session,
        "From: Herr Tobias Brenner <t@example.invalid>\n",
    );
    let second = clean(
        &p,
        &session,
        "tobias brenner and TOBIAS BRENNER-KLEE agreed.",
    );
    assert_eq!(tokens(&second).len(), 2, "{second}");
    assert!(second.ends_with("> agreed."), "{second}");
    assert!(!second.to_lowercase().contains("brenner"), "{second}");
    let rows = audit.0.lock().unwrap().clone();
    let links = rows
        .iter()
        .filter(|row| row.decided_by == ConflictTier::ManifestSweep)
        .map(|row| row.provenance_merged_from.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        links,
        ["manifest_sweep:part", "manifest_sweep:part"],
        "{rows:#?}"
    );
}
