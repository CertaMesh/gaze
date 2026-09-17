//! Private geometry and validation-atomicity controls; synthetic values only.
use super::*;
use crate::Scope;
use std::sync::{Arc, Mutex};

struct Capture(Arc<Mutex<Vec<RedactionEntry>>>);
impl RedactionLogger for Capture {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), crate::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

fn fixture() -> (Session, CleanText, String) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let x = session.tokenize(&PiiClass::Email, "X").unwrap();
    let y = session.tokenize(&PiiClass::Email, "Y").unwrap();
    let clean = CleanText {
        text: format!("aa{x}bb{y}cc"),
        manifest: vec![
            EmittedTokenSpan::new(2..2 + x.len(), 2..3, PiiClass::Email),
            EmittedTokenSpan::new(4 + x.len()..4 + x.len() + y.len(), 5..6, PiiClass::Email),
        ]
        .into(),
    };
    (session, clean, "aaXbbYcc".into())
}
fn parent(clean: &CleanText) -> LeakSuspect {
    suspect(
        0..clean.text.len(),
        LeakKind::PartialBleed { uncovered: 0..2 },
    )
}
fn suspect(span: Range<usize>, kind: LeakKind) -> LeakSuspect {
    LeakSuspect::new(
        span,
        PiiClass::Name,
        "parent.fixture",
        Some(1.0),
        kind,
        "synthetic",
        Some("field".into()),
    )
}
fn report(suspects: Vec<LeakSuspect>) -> LeakReport {
    LeakReport::from_parts(suspects, vec![])
}

#[test]
fn multiple_gap_plan_freezes_raw_bytes_and_parent_linkage_without_mutation() {
    let (session, clean, raw) = fixture();
    let report = report(vec![parent(&clean)]);
    let before = session.tokens();
    let baseline = session.restore_strict_text(&clean.text).unwrap();
    let plans = plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(&raw),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        plans.iter().map(|p| p.raw_span.clone()).collect::<Vec<_>>(),
        [0..2, 3..5, 6..8]
    );
    assert_eq!(
        plans.iter().map(|p| p.raw.as_str()).collect::<Vec<_>>(),
        ["aa", "bb", "cc"]
    );
    assert!(plans
        .iter()
        .all(|p| std::ptr::eq(p.suspect, &report.suspects[0])));
    assert_eq!(session.tokens(), before);
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), baseline);
}

#[test]
fn multiple_gap_parent_overlap_on_owned_bytes_is_permutation_deterministic() {
    // Reuse one session so ephemeral token prefixes are identical across permutations.
    let (session, clean, raw) = fixture();
    let first = clean.manifest[0].clone();
    let second = clean.manifest[1].clone();
    let left = suspect(
        0..second.clean_span.start + 1,
        LeakKind::PartialBleed { uncovered: 0..2 },
    );
    let right = suspect(
        second.clean_span.start..clean.text.len(),
        LeakKind::PartialBleed {
            uncovered: second.clean_span.end..clean.text.len(),
        },
    );
    let owned = suspect(
        first.clean_span.clone(),
        LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Email,
            safety_net_class: PiiClass::Name,
        },
    );
    let mut outputs = vec![];
    for order in [
        [0, 1, 2],
        [2, 1, 0],
        [1, 0, 2],
        [1, 2, 0],
        [0, 2, 1],
        [2, 0, 1],
    ] {
        let suspects = [left.clone(), right.clone(), owned.clone()];
        let report = report(order.into_iter().map(|i| suspects[i].clone()).collect());
        let mut tx = session.begin_transaction();
        let baseline = tx.restore_strict_text(&clean.text).unwrap();
        let mut copy = CleanText {
            text: clean.text.clone(),
            manifest: clean.manifest.clone(),
        };
        let logs = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .redaction_logger(Capture(logs.clone()))
            .build()
            .unwrap();
        let mut trace = ProtectionTraceCollector::new(&raw);
        for emitted in &clean.manifest {
            trace
                .record(
                    emitted.raw_span.clone(),
                    emitted.class.clone(),
                    GazeLocalProtectionTraceKind::PrimaryPolicyTokenize,
                    vec!["primary.fixture".into()],
                )
                .unwrap();
        }
        let reason = pipeline
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Staged(&mut tx),
                &mut copy,
                &report,
                DocumentKind::Text,
                None,
                Some(&mut trace),
            )
            .unwrap();
        assert_eq!(reason, None);
        assert_eq!(tx.restore_strict_text(&copy.text).unwrap(), baseline);
        let trace = trace.finish(&copy.manifest.projection().spans).unwrap();
        assert_eq!(
            trace.iter().map(|t| t.raw_span.clone()).collect::<Vec<_>>(),
            [0..2, 2..3, 3..5, 5..6, 6..8]
        );
        // Right-to-left assignment is observable in the exact output, not only geometry.
        let suffix = &copy.text[copy.manifest[4].clean_span.clone()];
        assert!(suffix.ends_with("Name_1>"));
        let rows = logs
            .lock()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e.source.clone(),
                    e.class.clone(),
                    e.action,
                    e.field_name.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 4);
        outputs.push((copy.text, copy.manifest, trace, rows));
        drop(tx);
    }
    assert!(outputs.windows(2).all(|p| p[0] == p[1]));
    assert_eq!(session.tokens().len(), 2);
}

#[test]
fn multiple_gap_refused_mixed_batches_never_publish_or_audit_a_partial_plan() {
    for case in 0..8 {
        let (session, mut clean, raw) = fixture();
        let primary = parent(&clean);
        let mut extra = match case {
            0 => primary.clone(),                    // duplicate, no dedup admission
            1 => suspect(0..2, LeakKind::Uncovered), // overlapping raw gap (word-aligned)
            2 => suspect(clean.text.len()..clean.text.len() + 1, LeakKind::Uncovered),
            3 => suspect(0..clean.manifest[0].clean_span.end, LeakKind::Uncovered),
            4 => suspect(
                0..clean.text.len(),
                LeakKind::ClassMismatch {
                    pipeline_class: PiiClass::Email,
                    safety_net_class: PiiClass::Name,
                },
            ),
            5 => suspect(0..0, LeakKind::Uncovered),
            6 => suspect(
                0..clean.text.len(),
                LeakKind::PartialBleed {
                    uncovered: clean.manifest[1].clean_span.end..clean.text.len(),
                },
            ),
            _ => suspect(clean.text.len()..clean.text.len(), LeakKind::Uncovered),
        };
        extra.safety_net_id = "other.fixture".into();
        let protected = suspect(clean.manifest[0].clean_span.clone(), LeakKind::Uncovered);
        let report = report(vec![primary, extra, protected]);
        let before = (clean.text.clone(), clean.manifest.clone(), session.tokens());
        assert!(plan_multiple_gap_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report,
            Some(&raw)
        )
        .unwrap()
        .is_none());
        let logs = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .redaction_logger(Capture(logs.clone()))
            .build()
            .unwrap();
        let reason = pipeline
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None,
            )
            .unwrap();
        assert_eq!(reason, Some(FallbackReason::OverlapConflict));
        assert_eq!((clean.text, clean.manifest, session.tokens()), before);
        assert!(logs.lock().unwrap().is_empty());
    }
}

#[test]
fn multiple_gap_invalid_geometry_ownership_and_affine_mapping_never_plan() {
    for case in 0..13 {
        let (session, mut clean, raw) = fixture();
        let mut item = parent(&clean);
        match case {
            0 => item.kind = LeakKind::PartialBleed { uncovered: 1..2 },
            1 => {
                item.kind = LeakKind::PartialBleed {
                    uncovered: 0..clean.text.len() + 1,
                }
            }
            2 => item.span = 0..0,
            3 => {
                let end = clean.text.len();
                item.span = end..end - 1;
            }
            4 => item.span.end += 1,
            5 => {
                clean.text.replace_range(0..2, "é");
                item.kind = LeakKind::PartialBleed { uncovered: 1..2 };
            }
            6 => {
                clean.text.replace_range(0..2, "é");
                item.span.start = 1;
            }
            7 => {
                // Same-shaped foreign literal, never an owned mapping.
                let span = clean.manifest[0].clean_span.clone();
                clean
                    .text
                    .replace_range(span.start + 1..span.start + 2, "z");
            }
            8 => {
                clean.manifest[0].raw_span.end += 1;
                clean.manifest[1].raw_span.start += 1;
                clean.manifest[1].raw_span.end += 1;
            }
            9 => clean.manifest.swap(0, 1),
            10 => clean.manifest.push(clean.manifest[0].clone()),
            11 => clean.manifest[1].clean_span.start -= 1,
            _ => clean.manifest[0].clean_span.end = clean.text.len() + 1,
        }
        let report = report(vec![item]);
        let result = plan_multiple_gap_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report,
            Some(&raw),
        );
        if case >= 9 {
            assert!(matches!(
                result,
                Err(Error::SafetyNet(SafetyNetError::InvalidOutput { .. }))
            ));
        } else {
            assert!(result.unwrap().is_none(), "case {case}");
        }
        assert_eq!(session.tokens().len(), 2);
    }
}

#[test]
fn multiple_gap_trace_requires_owned_value_equality_not_just_length() {
    let (session, clean, _) = fixture();
    let report = report(vec![parent(&clean)]);
    assert!(plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some("aaZbbYcc")
    )
    .unwrap()
    .is_none());
}

#[test]
fn multiple_gap_nonintersecting_unowned_entry_does_not_globally_reject() {
    let (session, mut clean, raw) = fixture();
    let item = parent(&clean);
    let end = clean.text.len();
    clean.text.push_str(" [REDACTED]");
    clean.manifest.push(EmittedTokenSpan::new(
        end + 1..end + 11,
        raw.len() + 1..raw.len() + 2,
        PiiClass::Location,
    ));
    let report = report(vec![item]);
    assert!(plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        None
    )
    .unwrap()
    .is_some());
    let baseline = session.restore_strict_text(&clean.text).unwrap();
    assert_eq!(
        Pipeline::builder()
            .build()
            .unwrap()
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None
            )
            .unwrap(),
        None
    );
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), baseline);
    assert!(clean.text.ends_with(" [REDACTED]"));
    assert_eq!(clean.manifest.last().unwrap().raw_span, 9..10);
}

#[test]
fn multiple_gap_reused_values_stay_owned_and_original_entries_unchanged() {
    let (session, mut clean, _) = fixture();
    let item = parent(&clean);
    let reused_token = clean.text[clean.manifest[0].clean_span.clone()].to_owned();
    clean
        .text
        .replace_range(clean.manifest[1].clean_span.clone(), &reused_token);
    let raw = "aaXbbXcc";
    let report = report(vec![item]);
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), raw);
    assert_eq!(
        Pipeline::builder()
            .build()
            .unwrap()
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None
            )
            .unwrap(),
        None
    );
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), raw);
    assert_eq!(clean.manifest[1].class, PiiClass::Email);
    assert_eq!(clean.manifest[3].class, PiiClass::Email);
    assert_eq!(
        &clean.text[clean.manifest[1].clean_span.clone()],
        &clean.text[clean.manifest[3].clean_span.clone()]
    );
}

#[test]
fn multiple_gap_foreign_literal_keeps_undefined_strict_restore_baseline() {
    let (session, mut clean, _) = fixture();
    let item = parent(&clean);
    clean.text.push_str(" <deadbeef:Name_999>");
    let report = report(vec![item]);
    assert!(session.restore_strict_text(&clean.text).is_err());
    assert_eq!(
        Pipeline::builder()
            .build()
            .unwrap()
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None
            )
            .unwrap(),
        None
    );
    assert!(session.restore_strict_text(&clean.text).is_err());
    assert!(clean.text.ends_with(" <deadbeef:Name_999>"));
}

#[test]
fn multiple_gap_source_ids_keep_existing_live_and_trace_validation_boundary() {
    let (session, clean, raw) = fixture();
    let mut item = parent(&clean);
    item.safety_net_id = String::new();
    let report = report(vec![item]);
    assert!(plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(&raw)
    )
    .unwrap()
    .is_some());
    for traced in [false, true] {
        let mut tx = session.begin_transaction();
        let mut copy = CleanText {
            text: clean.text.clone(),
            manifest: clean.manifest.clone(),
        };
        let mut trace = ProtectionTraceCollector::new(&raw);
        let result = Pipeline::builder()
            .build()
            .unwrap()
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Staged(&mut tx),
                &mut copy,
                &report,
                DocumentKind::Text,
                None,
                if traced { Some(&mut trace) } else { None },
            );
        if traced {
            assert!(
                matches!(result,Err(Error::SafetyNet(SafetyNetError::InvalidOutput{message})) if message=="empty protection source id")
            );
        } else {
            assert_eq!(result.unwrap(), None);
        }
        drop(tx);
        assert_eq!(session.tokens().len(), 2);
    }
}

#[test]
fn multiple_gap_invalid_class_in_mixed_batch_never_plans() {
    let (session, mut clean, raw) = fixture();
    let mut extra = suspect(
        clean.manifest[1].clean_span.end..clean.text.len(),
        LeakKind::Uncovered,
    );
    extra.class = PiiClass::Custom(String::new());
    let mut multi = parent(&clean);
    multi.span.end = clean.manifest[1].clean_span.start + 1;
    let report = report(vec![multi, extra]);
    assert!(plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(&raw)
    )
    .unwrap()
    .is_none());
    let before = (clean.text.clone(), clean.manifest.clone(), session.tokens());
    let logs = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .redaction_logger(Capture(logs.clone()))
        .build()
        .unwrap();
    assert_eq!(
        pipeline
            .resolve_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report,
                DocumentKind::Text,
                None,
                None
            )
            .unwrap(),
        Some(FallbackReason::OverlapConflict)
    );
    assert_eq!((clean.text, clean.manifest, session.tokens()), before);
    assert!(logs.lock().unwrap().is_empty());
}

#[test]
fn multiple_gap_adjacent_half_open_plans_remain_separate() {
    let (session, clean, raw) = fixture();
    let mut multi = parent(&clean);
    multi.span.start = 1;
    multi.kind = LeakKind::PartialBleed { uncovered: 1..2 };
    let report = report(vec![multi, suspect(0..1, LeakKind::Uncovered)]);
    let plans = plan_multiple_gap_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(&raw),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        plans.iter().map(|p| p.raw_span.clone()).collect::<Vec<_>>(),
        [0..1, 1..2, 3..5, 6..8]
    );
}

#[test]
fn multiple_gap_unowned_replacement_and_format_lookalikes_never_plan() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for replacement in [
        "[REDACTED]",
        "[EMAIL]",
        "<deadbeef:Email_1>",
        "email1.deadbeef@gaze-fake.invalid",
    ] {
        let clean = CleanText {
            text: format!("aa{replacement}cc"),
            manifest: vec![EmittedTokenSpan::new(
                2..2 + replacement.len(),
                2..3,
                PiiClass::Email,
            )]
            .into(),
        };
        let report = report(vec![parent(&clean)]);
        assert!(!session.contains_token(replacement));
        assert!(plan_multiple_gap_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report,
            None
        )
        .unwrap()
        .is_none());
        assert!(session.tokens().is_empty());
    }
}
