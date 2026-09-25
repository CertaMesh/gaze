//! Synthetic proof of truthful first-gap reports across owned replacements.
use std::sync::{Arc, Mutex};

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, DictionaryBundle,
    LeakSuspect, LocaleTag, PiiClass, Pipeline, RawDocument, SafetyNet, SafetyNetContext,
    SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

#[path = "support/stable_scan.rs"]
mod stable_scan;
use stable_scan::{emitted_text, stable_scan};

const RAW: &str = "pré alice@example.invalid 尾";

struct Primary;
impl Detector for Primary {
    fn detect(&self, _: &str) -> Vec<Detection> {
        vec![Detection::new(5..26, PiiClass::Email, "primary.fixture")]
    }
}

struct WholeParent {
    seen: Arc<Mutex<Vec<String>>>,
}
impl SafetyNet for WholeParent {
    fn id(&self) -> &str {
        "multigap.fixture"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(text.to_owned());
        let span = 0..text.len();
        Ok(context
            .manifest
            .diff_against(&span, &PiiClass::Email)
            .map(|kind| {
                LeakSuspect::new(
                    span,
                    PiiClass::Email,
                    self.id(),
                    Some(1.0),
                    kind,
                    "synthetic",
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}

#[test]
fn truthful_first_gap_resolves_both_sides_and_restores_original_bytes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(WholeParent { seen: seen.clone() })
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict),
        )
        .expect("truthful multiple gaps must resolve");
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..5, 5..26, 26..30]
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), RAW);
    for entry in &manifest {
        assert_eq!(
            session.restore(&clean[entry.clean_span.clone()]).unwrap(),
            RAW[entry.raw_span.clone()]
        );
    }
    let before = seen.lock().unwrap()[0].clone();
    assert_eq!(
        stable_scan(&clean[manifest[1].clean_span.clone()]),
        &before[5..before.len() - 4]
    );
    assert_eq!(report.suspects.len(), 2);
    assert_eq!(report.suspects[0].span, 0..5);
    assert_eq!(report.suspects[1].span, before.len() - 4..before.len());
    assert!(report
        .suspects
        .iter()
        .all(|suspect| matches!(suspect.kind, gaze::LeakKind::Uncovered)));
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(seen.lock().unwrap()[1], stable_scan(&clean));
}

#[derive(Clone)]
struct Markers(Vec<&'static str>);
impl Detector for Markers {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.0
            .iter()
            .flat_map(|marker| {
                input.match_indices(marker).map(|(start, _)| {
                    Detection::new(
                        start..start + marker.len(),
                        PiiClass::Email,
                        "primary.fixture",
                    )
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
enum Followup {
    Clear,
    EachToken,
    Whole,
    Raw,
    Error,
    Malformed,
}
type NetObservations = Arc<Mutex<Vec<(String, Vec<gaze::EmittedTokenSpan>)>>>;

struct Scripted {
    seen: NetObservations,
    followup: Followup,
    parent_class: PiiClass,
    wrong_first: bool,
}
impl SafetyNet for Scripted {
    fn id(&self) -> &str {
        "scripted.fixture"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let mut seen = self.seen.lock().unwrap();
        let first = seen.is_empty();
        seen.push((text.into(), context.manifest.spans.clone()));
        let suspect = |span, kind| {
            LeakSuspect::new(
                span,
                self.parent_class.clone(),
                self.id(),
                Some(1.0),
                kind,
                "synthetic",
                None,
            )
        };
        if first {
            let span = 0..text.find('|').unwrap_or(text.len());
            let kind = if self.wrong_first {
                gaze::LeakKind::PartialBleed {
                    uncovered: context.manifest.spans[0].clean_span.end..text.len(),
                }
            } else {
                context
                    .manifest
                    .diff_against(&span, &self.parent_class)
                    .unwrap()
            };
            return Ok(vec![suspect(span, kind)]);
        }
        match self.followup {
            Followup::Clear => Ok(vec![]),
            Followup::Error => Err(SafetyNetError::Runtime {
                message: "synthetic net error".into(),
            }),
            Followup::Malformed => Ok(vec![suspect(0..text.len() + 1, gaze::LeakKind::Uncovered)]),
            Followup::Raw => {
                let start = text.find("residual").expect("raw residual fixture");
                Ok(vec![suspect(start..start + 8, gaze::LeakKind::Uncovered)])
            }
            Followup::EachToken => Ok(context
                .manifest
                .spans
                .iter()
                .map(|s| {
                    suspect(
                        s.clean_span.clone(),
                        gaze::LeakKind::ClassMismatch {
                            pipeline_class: s.class.clone(),
                            safety_net_class: self.parent_class.clone(),
                        },
                    )
                })
                .collect()),
            Followup::Whole => {
                if seen.len() > 2 {
                    return Ok(vec![]);
                }
                let span = 0..text.len();
                Ok(vec![suspect(
                    span,
                    gaze::LeakKind::ClassMismatch {
                        pipeline_class: PiiClass::Email,
                        safety_net_class: self.parent_class.clone(),
                    },
                )])
            }
        }
    }
}
struct Capture(Arc<Mutex<Vec<gaze::RedactionEntry>>>);
impl gaze::RedactionLogger for Capture {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

#[test]
fn multiple_tokens_utf8_trace_and_metadata_audit_match_original_geometry() {
    let raw = "pré alice@example.invalid 中 bob@example.invalid 尾";
    let seen = Arc::new(Mutex::new(vec![]));
    let logs = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Markers(vec![
            "alice@example.invalid",
            "bob@example.invalid",
        ]))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(Scripted {
            seen: seen.clone(),
            followup: Followup::EachToken,
            parent_class: PiiClass::Name,
            wrong_first: false,
        })
        .redaction_logger(Capture(logs.clone()))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (doc, spans, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict),
        )
        .unwrap();
    let CleanDocument::Text(text) = doc else {
        panic!("text")
    };
    let expected = [0..5, 5..26, 26..31, 31..50, 50..54];
    assert_eq!(
        spans.iter().map(|s| s.raw_span.clone()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        session
            .restore_strict_text(&emitted_text(&seen.lock().unwrap()[0].0, &session.tokens()))
            .unwrap(),
        raw
    );
    assert_eq!(session.restore_strict_text(&text).unwrap(), raw);
    assert_eq!(trace.len(), 5);
    for (i, item) in trace.iter().enumerate() {
        assert_eq!(item.raw_start()..item.raw_end(), expected[i]);
        assert_eq!(item.class(), &spans[i].class);
        assert_eq!(item.action(), "tokenize");
        assert_eq!(
            item.decision(),
            if i % 2 == 0 { "resolve" } else { "policy" }
        );
        assert_eq!(
            item.source_ids(),
            &[if i % 2 == 0 {
                "scripted.fixture".to_string()
            } else {
                "primary.fixture".to_string()
            }]
        );
        assert_eq!(
            session.restore(&text[spans[i].clean_span.clone()]).unwrap(),
            raw[expected[i].clone()]
        );
    }
    let entries = logs.lock().unwrap();
    let actions = entries
        .iter()
        .filter(|e| e.decided_by == gaze::ConflictTier::Resolve && e.action == Action::Tokenize)
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 3);
    assert!(actions.iter().all(|e| e.class == PiiClass::Name
        && e.source == "safety_net.scripted.fixture"
        && !e.conflict_loser
        && e.fallback_triggered.is_none()));
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn staged_multigap_is_invisible_until_commit_and_drop_discards() {
    for commit in [false, true] {
        let seen = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(WholeParent { seen })
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let (doc, spans, _) = pipeline
            .clean_transaction_with_safety_net_policy_detect_context(
                &mut tx,
                RawDocument::Text(RAW.into()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict),
            )
            .unwrap();
        let CleanDocument::Text(text) = doc else {
            panic!("text")
        };
        assert_eq!(tx.restore_strict_text(&text).unwrap(), RAW);
        assert_eq!(spans.len(), 3);
        assert_eq!(tx.tokens().len(), 3);
        assert!(session.tokens().is_empty());
        if commit {
            tx.commit().unwrap();
            assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
        } else {
            drop(tx);
            assert!(session.tokens().is_empty());
        }
    }
}

#[test]
fn primary_format_preserve_is_owned_and_can_resolve_multiple_gaps() {
    for staged in [false, true] {
        let seen = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::FormatPreserve))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(WholeParent { seen: seen.clone() })
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let policy = SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict);
        let args = RawDocument::Text(RAW.into());
        let result = if staged {
            pipeline.clean_transaction_with_safety_net_policy_detect_context(
                &mut tx,
                args,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                policy,
            )
        } else {
            pipeline.clean_with_safety_net_policy_detect_context(
                &session,
                args,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                policy,
            )
        };
        let (CleanDocument::Text(text), spans, _) = result.unwrap() else {
            panic!("text")
        };
        assert_eq!(spans.len(), 3);
        let primary = &text[spans[1].clean_span.clone()];
        assert!(primary.contains("@gaze-fake.invalid"));
        if staged {
            assert!(tx.contains_token(primary));
            assert_eq!(tx.restore_strict_text(&text).unwrap(), RAW);
            assert!(session.tokens().is_empty());
        } else {
            assert!(session.contains_token(primary));
            assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
        }
        assert!(seen.lock().unwrap()[0].contains(&stable_scan(primary)));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }
}

#[test]
fn stale_first_gap_kind_is_recomputed_after_placeholder_clipping() {
    for fallback in [
        SafetyNetFallback::Strict,
        SafetyNetFallback::Tolerant,
        SafetyNetFallback::Redact,
    ] {
        let seen = Arc::new(Mutex::new(vec![]));
        let logs = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Scripted {
                seen: seen.clone(),
                followup: Followup::Clear,
                parent_class: PiiClass::Email,
                wrong_first: true,
            })
            .redaction_logger(Capture(logs.clone()))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let result = pipeline.clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, fallback),
        );
        let (CleanDocument::Text(text), spans, report) = result.unwrap() else {
            panic!("text")
        };
        assert_eq!(spans.len(), 3);
        assert_eq!(report.suspects.len(), 2);
        assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
        assert_eq!(session.tokens().len(), 3);
        let entries = logs.lock().unwrap();
        assert!(!entries
            .iter()
            .any(|e| e.decided_by == gaze::ConflictTier::Fallback));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }
}

#[test]
fn compatibility_primary_nonowned_replacements_keep_old_outputs() {
    for action in [Action::Redact, Action::Generalize] {
        for fallback in [
            SafetyNetFallback::Strict,
            SafetyNetFallback::Tolerant,
            SafetyNetFallback::Redact,
        ] {
            let seen = Arc::new(Mutex::new(vec![]));
            let pipeline = Pipeline::builder()
                .detector(Primary)
                .rule(ClassRule::new(PiiClass::Email, action))
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(Scripted {
                    seen: seen.clone(),
                    followup: Followup::Clear,
                    parent_class: PiiClass::Email,
                    wrong_first: false,
                })
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let result = pipeline.clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(RAW.into()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::new(SafetyNetMode::Resolve, fallback),
            );
            if matches!(fallback, SafetyNetFallback::Strict) {
                assert!(matches!(
                    result,
                    Err(gaze::Error::SafetyNetFallback(
                        gaze::FallbackReason::OverlapConflict
                    ))
                ));
            } else {
                let (CleanDocument::Text(text), _, _) = result.unwrap() else {
                    panic!("text")
                };
                let replacement = if action == Action::Redact {
                    "[REDACTED]"
                } else {
                    "[EMAIL]"
                };
                assert_eq!(
                    text,
                    format!(
                        "{}{replacement} 尾",
                        if matches!(fallback, SafetyNetFallback::Redact) {
                            gaze::redaction_marker(&gaze::PiiClass::Email)
                        } else {
                            "pré ".to_string()
                        }
                    )
                );
            }
            assert!(session.tokens().is_empty());
        }
    }
}

#[test]
fn explicit_redact_covers_both_exposed_gaps_but_preserves_token() {
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(WholeParent { seen: seen.clone() })
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (CleanDocument::Text(text), spans, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Redact, SafetyNetFallback::Strict),
        )
        .unwrap()
    else {
        panic!("text")
    };
    let marker = gaze::redaction_marker(&gaze::PiiClass::Email);
    let before = seen.lock().unwrap()[0].clone();
    // Both exposed gaps are redacted. The owned token between them remains restorable.
    assert_eq!(
        stable_scan(&text),
        format!("{marker}{}{marker}", &before[5..before.len() - 4])
    );
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].raw_span, 0..5);
    assert_eq!(spans[1].raw_span, 5..26);
    assert_eq!(spans[2].raw_span, 26..30);
    assert_eq!(
        session.restore_strict_text(&text).unwrap(),
        format!("{marker}{}{marker}", &RAW[5..26])
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn full_parent_mixed_class_findings_resolve_only_exposed_gaps() {
    for fallback in [
        SafetyNetFallback::Strict,
        SafetyNetFallback::Tolerant,
        SafetyNetFallback::Redact,
    ] {
        let seen = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .detector(Primary)
            .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(Scripted {
                seen: seen.clone(),
                followup: Followup::Whole,
                parent_class: PiiClass::Name,
                wrong_first: false,
            })
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let result = pipeline.clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, fallback),
        );
        let (CleanDocument::Text(text), spans, report) = result.unwrap() else {
            panic!("text")
        };
        assert_eq!(spans.len(), 3);
        assert_eq!(report.suspects.len(), 2);
        assert_eq!(spans[0].class, PiiClass::Name);
        assert_eq!(spans[1].class, PiiClass::Email);
        assert_eq!(spans[2].class, PiiClass::Name);
        assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
        assert_eq!(seen.lock().unwrap().len(), 2);
    }
}

#[test]
fn post_multigap_net_errors_malformed_and_raw_residuals_stay_enforcing_and_staged() {
    for followup in [Followup::Error, Followup::Malformed, Followup::Raw] {
        for staged in [false, true] {
            let seen = Arc::new(Mutex::new(vec![]));
            let raw = format!("{RAW}|residual");
            let pipeline = Pipeline::builder()
                .detector(Primary)
                .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(Scripted {
                    seen: seen.clone(),
                    followup,
                    parent_class: PiiClass::Email,
                    wrong_first: false,
                })
                .build()
                .unwrap();
            let session = Session::new(Scope::Ephemeral).unwrap();
            let mut tx = session.begin_transaction();
            let policy = SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict);
            let result = if staged {
                pipeline.clean_transaction_with_safety_net_policy_detect_context(
                    &mut tx,
                    RawDocument::Text(raw.clone()),
                    &[LocaleTag::Global],
                    &DictionaryBundle::default(),
                    policy,
                )
            } else {
                pipeline.clean_with_safety_net_policy_detect_context(
                    &session,
                    RawDocument::Text(raw.clone()),
                    &[LocaleTag::Global],
                    &DictionaryBundle::default(),
                    policy,
                )
            };
            match followup {
                Followup::Error => assert!(matches!(
                    result,
                    Err(gaze::Error::SafetyNet(SafetyNetError::Runtime { .. }))
                )),
                _ => assert!(matches!(
                    result,
                    Err(gaze::Error::SafetyNetFallback(
                        gaze::FallbackReason::ResidualSuspect
                    ))
                )),
            }
            assert_eq!(seen.lock().unwrap().len(), 2);
            assert_eq!(
                if staged {
                    tx.restore_strict_text(&emitted_text(&seen.lock().unwrap()[1].0, &tx.tokens()))
                } else {
                    session.restore_strict_text(&emitted_text(
                        &seen.lock().unwrap()[1].0,
                        &session.tokens(),
                    ))
                }
                .unwrap(),
                raw
            );
            if staged {
                assert_eq!(tx.tokens().len(), 3);
                assert!(session.tokens().is_empty());
                drop(tx);
                assert!(session.tokens().is_empty());
            } else {
                assert_eq!(session.tokens().len(), 3);
            }
        }
    }
}
