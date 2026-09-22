use gaze::*;

struct Primary;
impl Detector for Primary {
    fn detect(&self, _: &str) -> Vec<Detection> {
        vec![Detection::new(5..12, PiiClass::Email, "synthetic-primary")]
    }
}
struct Net;
impl SafetyNet for Net {
    fn id(&self) -> &str {
        "synthetic-terminal-primary"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let mismatch = LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Email,
            safety_net_class: PiiClass::Name,
        };
        let (span, kind) = if let Some(i) = text.find("seed") {
            (i..i + 4, LeakKind::Uncovered)
        } else if let Some(i) = text.find("barrier ") {
            (i..i + 8, mismatch)
        } else {
            (
                context
                    .manifest
                    .spans
                    .iter()
                    .find(|s| s.class == PiiClass::Name)
                    .unwrap()
                    .clean_span
                    .clone(),
                mismatch,
            )
        };
        Ok(vec![LeakSuspect::new(
            span,
            PiiClass::Name,
            self.id(),
            None,
            kind,
            "synthetic",
            None,
        )])
    }
}

fn prove_primary_action(action: Action, staged: bool) {
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(DefaultRule::new(action))
        .register_safety_net(Net)
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let raw = RawDocument::Text("seed primary barrier residual é".into());
    let result = if staged {
        pipeline.clean_transaction_with_safety_net_policy_detect_context(
            &mut transaction,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
    } else {
        pipeline.clean_with_safety_net(&session, raw, &[LocaleTag::Global])
    };
    let (CleanDocument::Text(text), manifest, _) =
        result.unwrap_or_else(|error| panic!("action={action:?} staged={staged}: {error:?}"))
    else {
        panic!("text")
    };
    assert!(!text.contains("primary"));
    assert!(!text.contains("barrier"));
    // The safety-net fallback replaced "barrier " -- trailing space included -- with a one-way
    // marker, so the marker abuts "residual" where deleting used to leave the space behind.
    assert!(text.ends_with(&format!(
        "{}residual é",
        gaze::redaction_marker(&PiiClass::Name)
    )));
    assert_eq!(manifest[0].raw_span, 0..4);
    assert_eq!(manifest[1].raw_span, 5..12);
    assert_eq!(
        if staged {
            transaction.restore(&text[manifest[0].clean_span.clone()])
        } else {
            session.restore(&text[manifest[0].clean_span.clone()])
        }
        .as_deref(),
        Some("seed")
    );
    if staged {
        assert!(session.tokens().is_empty());
    }
}

#[test]
fn review_primary_redact_live() {
    prove_primary_action(Action::Redact, false);
}
#[test]
fn review_primary_redact_staged() {
    prove_primary_action(Action::Redact, true);
}
#[test]
fn review_primary_generalize_live() {
    prove_primary_action(Action::Generalize, false);
}
#[test]
fn review_primary_generalize_staged() {
    prove_primary_action(Action::Generalize, true);
}

/// Reflag the retained primary replacement, rather than the token created by Resolve.
struct ReflagPrimary;
impl SafetyNet for ReflagPrimary {
    fn id(&self) -> &str {
        Net.id()
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        Net.supported_locales()
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        if text.contains("seed") || text.contains("barrier ") {
            return Net.check(text, context);
        }
        let primary = context
            .manifest
            .spans
            .iter()
            .find(|s| s.class == PiiClass::Email)
            .unwrap();
        Ok(vec![LeakSuspect::new(
            primary.clean_span.clone(),
            PiiClass::Name,
            self.id(),
            None,
            LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            },
            "synthetic",
            None,
        )])
    }
}

fn reflag_primary(action: Action, staged: bool) {
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(DefaultRule::new(action))
        .register_safety_net(ReflagPrimary)
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let raw = RawDocument::Text("seed primary barrier residual é".into());
    let result = if staged {
        pipeline.clean_transaction_with_safety_net_policy_detect_context(
            &mut transaction,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
    } else {
        pipeline.clean_with_safety_net(&session, raw, &[LocaleTag::Global])
    };
    if action == Action::FormatPreserve {
        let (CleanDocument::Text(text), manifest, _) = result.unwrap() else {
            panic!("text")
        };
        let primary = &manifest[1];
        assert_eq!(primary.raw_span, 5..12);
        let replacement = &text[primary.clean_span.clone()];
        let restored = if staged {
            transaction.restore(replacement)
        } else {
            session.restore(replacement)
        };
        assert_eq!(restored.as_deref(), Some("primary"));
        assert!(text.ends_with(&format!(
            "{}residual é",
            gaze::redaction_marker(&PiiClass::Name)
        )));
    } else {
        assert!(
            matches!(result, Err(Error::SafetyNetFallback(_))),
            "unowned replacement cannot exempt a suspect: {result:?}"
        );
    }
    if staged {
        assert!(session.tokens().is_empty());
    }
}

#[test]
fn retained_primary_unowned_reflags_still_reject() {
    for action in [Action::Redact, Action::Generalize] {
        for staged in [false, true] {
            reflag_primary(action, staged);
        }
    }
}

#[test]
fn retained_format_preserve_uses_actual_live_and_staged_ownership() {
    for staged in [false, true] {
        reflag_primary(Action::FormatPreserve, staged);
    }
}
