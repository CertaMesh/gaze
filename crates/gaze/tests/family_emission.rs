use std::ops::Range;
use std::sync::{Arc, Mutex};

use gaze::{
    Action, Candidate, ClassRule, CleanDocument, CollisionMembership, ConflictTier, DefaultRule,
    DetectContext, DetectError, PiiClass, Pipeline, RawDocument, Recognizer, RedactionEntry,
    RedactionLogError, RedactionLogger, Scope, Session,
};
use gaze_types::AmbiguityReason;

#[derive(Clone, Default)]
struct Audit(Arc<Mutex<Vec<RedactionEntry>>>);
impl RedactionLogger for Audit {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

// A deterministic external recognizer whose display label differs from its identity.
struct Finding {
    id: &'static str,
    class: PiiClass,
    span: Range<usize>,
    priority: i32,
}
impl Recognizer for Finding {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(&self, _: &str, _: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        Ok(vec![Candidate::new(
            self.span.clone(),
            self.class.clone(),
            self.id,
            0.9,
            self.priority,
            None,
            "counter",
            format!("display label for {}", self.id),
            ConflictTier::None,
            vec![],
        )])
    }
}
fn finding(id: &'static str, span: Range<usize>, priority: i32) -> Finding {
    Finding {
        id,
        class: PiiClass::Custom(id.to_string()),
        span,
        priority,
    }
}
fn run(tie: bool, recovery: bool, defeat: bool) -> Vec<RedactionEntry> {
    let audit = Audit::default();
    let mut builder = Pipeline::builder()
        .recognizer(finding("alpha", 0..8, 10))
        .register_collision(
            "alpha",
            CollisionMembership::new(
                "documents",
                "alpha",
                10,
                (!tie).then(|| "document-cue".to_string()),
            ),
        );
    if tie {
        builder = builder
            .recognizer(finding("beta", 0..8, 10))
            .register_collision(
                "beta",
                CollisionMembership::new("documents", "beta", 10, None),
            );
    }
    if recovery {
        builder = builder
            .recognizer(finding("bridge", 1..20, 20))
            .recognizer(Finding {
                id: "later",
                class: PiiClass::Name,
                span: 10..24,
                priority: 30,
            });
    } else {
        builder = builder.recognizer(finding("foreign", 4..12, if defeat { 20 } else { 0 }));
    }
    let pipeline = builder
        .rule(ClassRule::new(
            PiiClass::family("documents"),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Tokenize))
        .redaction_logger(audit.clone())
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "CASE0001abcdefghijklmnop";
    let CleanDocument::Text(clean) = pipeline
        .redact(&session, RawDocument::Text(raw.into()))
        .unwrap()
    else {
        panic!("text expected")
    };
    assert_eq!(session.restore_strict_text(&clean).unwrap(), raw);
    let entries = audit.0.lock().unwrap().clone();
    entries
}
#[test]
fn precedence_tie_audit_survives_later_rule_priority_win() {
    let entries = run(true, false, false);
    let winner = entries.iter().find(|e| !e.conflict_loser).unwrap();
    assert_eq!(winner.class, PiiClass::family("documents"));
    assert_eq!(winner.decided_by, ConflictTier::RulePriority);
    assert_eq!(winner.collision_family.as_deref(), Some("documents"));
    let record = winner
        .ambiguity_record
        .as_ref()
        .expect("family emission must retain its record");
    assert_eq!(record.reason, AmbiguityReason::PrecedenceTie);
    let ids = record
        .losing_candidates
        .iter()
        .map(|c| c.recognizer_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["alpha", "beta", "foreign"]);
}
#[test]
fn defeated_family_emission_does_not_attach_to_foreign_winner() {
    let entries = run(true, false, true);
    let winner = entries.iter().find(|e| !e.conflict_loser).unwrap();
    assert_eq!(winner.recognizer_id.as_deref(), Some("foreign"));
    assert!(winner.ambiguity_record.is_none());
    assert!(winner.collision_family.is_none());
}
#[test]
fn no_anchor_audit_uses_recognizer_ids_in_primary_and_recovery() {
    for recovery in [false, true] {
        let entries = run(false, recovery, false);
        let winner = entries
            .iter()
            .find(|e| !e.conflict_loser && e.class == PiiClass::family("documents"))
            .expect("family winner on primary and recovery paths");
        assert_eq!(winner.decided_by, ConflictTier::AnchoredContext);
        let record = winner.ambiguity_record.as_ref().expect("NoAnchor record");
        assert_eq!(record.reason, AmbiguityReason::NoAnchor);
        let ids = record
            .losing_candidates
            .iter()
            .map(|c| c.recognizer_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            if recovery {
                vec!["alpha"]
            } else {
                vec!["alpha", "foreign"]
            }
        );
        assert!(record
            .losing_candidates
            .iter()
            .all(|c| c.class == PiiClass::Custom(c.recognizer_id.clone())));
    }
}
