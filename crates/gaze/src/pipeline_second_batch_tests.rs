//! Corrupt-state and ownership controls unavailable through primary production constructors.
use super::*;
use crate::Scope;
use std::sync::{Arc, Mutex};

fn suspect(span: Range<usize>, kind: LeakKind) -> LeakSuspect {
    LeakSuspect::new(
        span,
        PiiClass::Name,
        "second.fixture",
        Some(0.9),
        kind,
        "synthetic",
        Some("field".into()),
    )
}
fn mismatch(span: Range<usize>) -> LeakSuspect {
    suspect(
        span,
        LeakKind::ClassMismatch {
            pipeline_class: PiiClass::Email,
            safety_net_class: PiiClass::Name,
        },
    )
}
fn report(items: Vec<LeakSuspect>) -> LeakReport {
    LeakReport::from_parts(items, vec![])
}
fn fixture(format: bool) -> (Session, CleanText, String) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = if format {
        session
            // fixture-cited(crates/gaze/src/pipeline_second_batch_tests.rs:pipeline::second_batch_tests::second_batch_single_and_multigap_owned_format_preserve_restore_trace_and_parent_metadata)
            .format_preserving_fake(&PiiClass::Email, "alice@example.invalid")
            .unwrap()
    } else {
        session
            // fixture-cited(crates/gaze/src/pipeline_second_batch_tests.rs:pipeline::second_batch_tests::second_batch_single_and_multigap_owned_format_preserve_restore_trace_and_parent_metadata)
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .unwrap()
    };
    let clean = CleanText {
        text: format!("pré {token} 尾"),
        manifest: vec![EmittedTokenSpan::new(
            5..5 + token.len(),
            5..26,
            PiiClass::Email,
        )],
    };
    // fixture-cited(crates/gaze/src/pipeline_second_batch_tests.rs:pipeline::second_batch_tests::second_batch_single_and_multigap_owned_format_preserve_restore_trace_and_parent_metadata)
    (session, clean, "pré alice@example.invalid 尾".into())
}
struct Capture(Arc<Mutex<Vec<RedactionEntry>>>);
impl RedactionLogger for Capture {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), crate::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

#[test]
fn second_batch_single_and_multigap_owned_format_preserve_restore_trace_and_parent_metadata() {
    for format in [false, true] {
        for multigap in [false, true] {
            let (session, mut clean, original) = fixture(format);
            let owned = clean.manifest[0].clone();
            let token = clean.text[owned.clean_span.clone()].to_owned();
            let parent = suspect(
                0..if multigap {
                    clean.text.len()
                } else {
                    owned.clean_span.end
                },
                LeakKind::PartialBleed { uncovered: 0..5 },
            );
            let report = report(vec![parent, mismatch(owned.clean_span.clone())]);
            let before = session.tokens();
            let plan = plan_followup_resolutions(
                &ProtectionTarget::Live(&session),
                &clean,
                &report,
                Some(&original),
            )
            .unwrap();
            assert_eq!(session.tokens(), before);
            let FollowupResolution::Ready(plan) = plan else {
                panic!("complete owned gaps")
            };
            assert_eq!(plan.parents.len(), 1);
            assert_eq!(
                plan.gaps
                    .iter()
                    .map(|g| g.raw_span.clone())
                    .collect::<Vec<_>>(),
                if multigap {
                    vec![0..5, 26..30]
                } else {
                    std::iter::once(0..5).collect::<Vec<_>>()
                }
            );
            assert!(plan
                .gaps
                .iter()
                .all(|g| std::ptr::eq(g.suspect, &report.suspects[0])));
            let rows = Arc::new(Mutex::new(vec![]));
            let pipeline = Pipeline::builder()
                .redaction_logger(Capture(rows.clone()))
                .build()
                .unwrap();
            let mut trace = ProtectionTraceCollector::new(&original);
            trace
                .record(
                    owned.raw_span,
                    PiiClass::Email,
                    GazeLocalProtectionTraceKind::PrimaryPolicyTokenize,
                    vec!["primary.fixture".into()],
                )
                .unwrap();
            pipeline
                .apply_followup_resolutions(
                    &mut ProtectionTarget::Live(&session),
                    &mut clean,
                    plan,
                    DocumentKind::Text,
                    Some("field"),
                    Some(&mut trace),
                )
                .unwrap();
            assert_eq!(session.restore_strict_text(&clean.text).unwrap(), original);
            assert_eq!(&clean.text[clean.manifest[1].clean_span.clone()], token);
            let trace = trace.finish(&clean.manifest).unwrap();
            assert_eq!(trace.len(), if multigap { 3 } else { 2 });
            for item in trace.iter().filter(|t| t.class == PiiClass::Name) {
                assert_eq!(item.source_ids, ["second.fixture"]);
            }
            let rows = rows.lock().unwrap();
            assert_eq!(rows[0].action, Action::Preserve);
            assert_eq!(rows.len(), if multigap { 3 } else { 2 });
            assert!(rows[1..].iter().all(|r| r.action == Action::Tokenize
                && r.class == PiiClass::Name
                && r.source == "safety_net.second.fixture"));
        }
    }
}

#[test]
fn second_batch_unowned_intersections_decline_but_nonintersecting_primary_is_eligible() {
    for replacement in ["[REDACTED]", "[EMAIL]", "<foreign:Name_1>"] {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let clean = CleanText {
            text: format!("aa{replacement}bb"),
            manifest: vec![EmittedTokenSpan::new(
                2..2 + replacement.len(),
                2..3,
                PiiClass::Email,
            )],
        };
        let before = session.tokens();
        for partial in [false, true] {
            let s = suspect(
                0..clean.text.len(),
                if partial {
                    LeakKind::PartialBleed { uncovered: 0..2 }
                } else {
                    LeakKind::Uncovered
                },
            );
            assert!(matches!(
                plan_followup_resolutions(
                    &ProtectionTarget::Live(&session),
                    &clean,
                    &report(vec![s]),
                    Some("aaXbb")
                )
                .unwrap(),
                FollowupResolution::NotApplicable
            ));
        }
        let report = report(vec![suspect(0..2, LeakKind::Uncovered)]);
        let FollowupResolution::Ready(plan) = plan_followup_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report,
            Some("aaXbb"),
        )
        .unwrap() else {
            panic!("unrelated primary entry")
        };
        assert_eq!(plan.gaps[0].raw_span, 0..2);
        assert_eq!(session.tokens(), before);
    }
}

#[test]
fn second_batch_adjacent_owned_union_and_duplicate_claims_are_not_protection() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let x = session.tokenize(&PiiClass::Email, "X").unwrap();
    let y = session.tokenize(&PiiClass::Email, "Y").unwrap();
    let clean = CleanText {
        text: format!("{x}{y}aa"),
        manifest: vec![
            EmittedTokenSpan::new(0..x.len(), 0..1, PiiClass::Email),
            EmittedTokenSpan::new(x.len()..x.len() + y.len(), 1..2, PiiClass::Email),
        ],
    };
    let union = mismatch(0..x.len() + y.len());
    assert!(!suspect_is_inside_live_token(
        &ProtectionTarget::Live(&session),
        &clean,
        &union
    ));
    assert!(matches!(
        plan_followup_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report(vec![union]),
            Some("XYaa")
        )
        .unwrap(),
        FollowupResolution::NotApplicable
    ));
    let tail = x.len() + y.len();
    for duplicate in [false, true] {
        let a = suspect(tail..tail + 2, LeakKind::Uncovered);
        let b = suspect(
            if duplicate {
                tail..tail + 2
            } else {
                tail + 1..tail + 2
            },
            LeakKind::Uncovered,
        );
        for order in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
            assert!(matches!(
                plan_followup_resolutions(
                    &ProtectionTarget::Live(&session),
                    &clean,
                    &report(order),
                    Some("XYaa")
                )
                .unwrap(),
                FollowupResolution::NotApplicable
            ));
        }
    }
}

#[test]
fn second_batch_parent_overlap_only_on_owned_bytes_has_unique_disjoint_provenance() {
    let (session, clean, original) = fixture(false);
    let token = &clean.manifest[0].clean_span;
    let left = suspect(0..token.end, LeakKind::PartialBleed { uncovered: 0..5 });
    let right = suspect(
        token.start..clean.text.len(),
        LeakKind::PartialBleed {
            uncovered: token.end..clean.text.len(),
        },
    );
    for order in [vec![left.clone(), right.clone()], vec![right, left]] {
        let report = report(order);
        let FollowupResolution::Ready(plan) = plan_followup_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report,
            Some(&original),
        )
        .unwrap() else {
            panic!("unique disjoint gaps")
        };
        assert_eq!(
            plan.gaps
                .iter()
                .map(|g| g.raw_span.clone())
                .collect::<Vec<_>>(),
            [0..5, 26..30]
        );
        assert_eq!(plan.parents.len(), 2);
    }
}

#[test]
fn second_batch_source_and_affine_corruption_are_fatal_before_any_effect() {
    for corruption in 0..5 {
        for reverse in [false, true] {
            let (session, mut clean, mut original) = fixture(false);
            let parent = suspect(
                0..clean.text.len(),
                LeakKind::PartialBleed { uncovered: 0..5 },
            );
            let mut items = vec![mismatch(0..5), parent];
            match corruption {
                0 => original.replace_range(5..10, "xxxxx"), // Same-length wrong owned source.
                1 => original.replace_range(0..5, "xxxxx"),  // Wrong raw-gap source.
                2 => clean.manifest[0].raw_span.start += 1,
                3 => clean.manifest[0].raw_span.end += 1,
                _ => items[1].safety_net_id = " \t".into(),
            }
            if reverse {
                items.reverse();
            }
            let before = session.tokens();
            let original_text = clean.text.clone();
            let report = report(items);
            assert!(plan_followup_resolutions(
                &ProtectionTarget::Live(&session),
                &clean,
                &report,
                Some(&original)
            )
            .is_err());
            assert_eq!(session.tokens(), before);
            assert_eq!(clean.text, original_text);
        }
    }
}

#[test]
fn second_batch_untraced_does_not_claim_original_source_or_require_trace_id() {
    let (session, clean, _) = fixture(false);
    let mut s = suspect(0..5, LeakKind::Uncovered);
    s.safety_net_id = " ".into();
    assert!(matches!(
        plan_followup_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report(vec![s]),
            None
        )
        .unwrap(),
        FollowupResolution::Ready(_)
    ));
}

struct Reports(Mutex<Vec<LeakReport>>);
impl SafetyNet for Reports {
    fn id(&self) -> &str {
        "reports.fixture"
    }
    fn supported_locales(&self) -> &[crate::LocaleTag] {
        &[crate::LocaleTag::Global]
    }
    fn check(
        &self,
        _: &str,
        _: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        Ok(self.0.lock().unwrap().remove(0).suspects)
    }
}

#[test]
fn second_batch_history_counts_parent_once_and_preserve_once_per_nonterminal_phase() {
    let (session, mut clean, original) = fixture(false);
    let token = clean.manifest[0].clean_span.clone();
    let parent = suspect(
        0..clean.text.len(),
        LeakKind::PartialBleed { uncovered: 0..5 },
    );
    let protected = mismatch(token.clone());
    let rows = Arc::new(Mutex::new(vec![]));
    // Two Name replacements are equally long; middle Email's shifted position is fixed by token grammar.
    let name_len = session
        .tokenize_with_family("safety_net", &PiiClass::Name, "尾")
        .unwrap()
        .len();
    let third = mismatch(name_len..name_len + token.len());
    let pipeline = Pipeline::builder()
        .register_safety_net(Reports(Mutex::new(vec![
            report(vec![parent.clone(), protected]),
            report(vec![third]),
        ])))
        .redaction_logger(Capture(rows.clone()))
        .build()
        .unwrap();
    let mut history = report(vec![]);
    history.replay_hash = Some("old-hash".into());
    history.stats.suspect_count = 999; // Admission and final counters must ignore stale aggregate fields.
    let telemetry = LeakReportTelemetry::LocaleSkipped {
        safety_net_id: "other.fixture".into(),
        document_kind: DocumentKind::Text,
        field_path: None,
    };
    history.telemetry.push(telemetry.clone());
    let mut trace = ProtectionTraceCollector::new(&original);
    trace
        .record(
            5..26,
            PiiClass::Email,
            GazeLocalProtectionTraceKind::PrimaryPolicyTokenize,
            vec!["primary.fixture".into()],
        )
        .unwrap();
    pipeline
        .apply_safety_net_policy(
            &mut ProtectionTarget::Live(&session),
            &mut clean,
            &mut history,
            DocumentKind::Text,
            &[crate::LocaleTag::Global],
            Some("field"),
            SafetyNetPolicy::default().decision(),
            Some(&mut trace),
        )
        .unwrap();
    assert_eq!(history.suspects.len(), 1);
    assert_eq!(history.suspects[0].span, parent.span);
    assert_eq!(history.suspects[0].field_path, parent.field_path);
    assert_eq!(history.stats.suspect_count, 1);
    assert_eq!(history.stats.partial_bleed_count, 1);
    assert_eq!(history.replay_hash, None);
    assert_eq!(history.telemetry, [telemetry]);
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), original);
    assert_eq!(trace.finish(&clean.manifest).unwrap().len(), 3);
    assert_eq!(
        rows.lock()
            .unwrap()
            .iter()
            .map(|r| r.action)
            .collect::<Vec<_>>(),
        [
            Action::Preserve,
            Action::Tokenize,
            Action::Tokenize,
            Action::Preserve
        ]
    );
}

#[test]
fn second_batch_utf8_before_between_after_two_tokens_keeps_exact_source_and_audit_order() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let x = session.tokenize(&PiiClass::Email, "X").unwrap();
    let y = session.tokenize(&PiiClass::Email, "Y").unwrap();
    let original = "éX中Y尾";
    let mut clean = CleanText {
        text: format!("é{x}中{y}尾"),
        manifest: vec![
            EmittedTokenSpan::new(2..2 + x.len(), 2..3, PiiClass::Email),
            EmittedTokenSpan::new(5 + x.len()..5 + x.len() + y.len(), 6..7, PiiClass::Email),
        ],
    };
    let report = report(vec![suspect(
        0..clean.text.len(),
        LeakKind::PartialBleed { uncovered: 0..2 },
    )]);
    let FollowupResolution::Ready(plan) = plan_followup_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(original),
    )
    .unwrap() else {
        panic!("complete UTF8 gaps")
    };
    assert_eq!(
        plan.gaps
            .iter()
            .map(|g| (g.raw_span.clone(), g.raw.as_str()))
            .collect::<Vec<_>>(),
        [(0..2, "é"), (3..6, "中"), (7..10, "尾")]
    );
    let mut trace = ProtectionTraceCollector::new(original);
    for emitted in &clean.manifest {
        trace
            .record(
                emitted.raw_span.clone(),
                PiiClass::Email,
                GazeLocalProtectionTraceKind::PrimaryPolicyTokenize,
                vec!["primary.fixture".into()],
            )
            .unwrap();
    }
    Pipeline::builder()
        .build()
        .unwrap()
        .apply_followup_resolutions(
            &mut ProtectionTarget::Live(&session),
            &mut clean,
            plan,
            DocumentKind::Text,
            None,
            Some(&mut trace),
        )
        .unwrap();
    assert_eq!(session.restore_strict_text(&clean.text).unwrap(), original);
    let trace = trace.finish(&clean.manifest).unwrap();
    assert_eq!(
        trace.iter().map(|t| t.raw_span.clone()).collect::<Vec<_>>(),
        [0..2, 2..3, 3..6, 6..7, 7..10]
    );
    assert_eq!(&clean.text[clean.manifest[1].clean_span.clone()], x);
    assert_eq!(&clean.text[clean.manifest[3].clean_span.clone()], y);
    assert!(
        clean.text[clean.manifest[4].clean_span.clone()].ends_with("Name_1>"),
        "apply right to left"
    );
    assert!(clean.text[clean.manifest[0].clean_span.clone()].ends_with("Name_3>"));
}

#[test]
fn second_batch_false_single_gap_and_protected_source_mismatch_are_fatal() {
    let (session, clean, original) = fixture(false);
    let token = &clean.manifest[0].clean_span;
    let cases = [
        suspect(0..token.end, LeakKind::Uncovered),
        suspect(0..token.end, LeakKind::PartialBleed { uncovered: 0..2 }),
        suspect(
            0..token.end,
            LeakKind::PartialBleed {
                uncovered: 0..token.end + 1,
            },
        ),
        suspect(0..token.end, LeakKind::PartialBleed { uncovered: 0..0 }),
    ];
    for item in cases {
        let before = session.tokens();
        assert!(plan_followup_resolutions(
            &ProtectionTarget::Live(&session),
            &clean,
            &report(vec![item]),
            Some(&original)
        )
        .is_err());
        assert_eq!(session.tokens(), before);
    }
    let report = report(vec![
        mismatch(token.clone()),
        suspect(0..5, LeakKind::Uncovered),
    ]);
    let wrong_source = original.replacen("alice", "xxxxx", 1);
    assert!(plan_followup_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report,
        Some(&wrong_source)
    )
    .is_err());
}

#[test]
fn second_batch_unowned_neighbor_cannot_hide_false_uncovered_owned_intersection() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let owned = session.tokenize(&PiiClass::Email, "X").unwrap();
    let clean = CleanText {
        text: format!("a{owned}[REDACTED]z"),
        manifest: vec![
            EmittedTokenSpan::new(1..1 + owned.len(), 1..2, PiiClass::Email),
            EmittedTokenSpan::new(1 + owned.len()..11 + owned.len(), 2..3, PiiClass::Email),
        ],
    };
    let before = session.tokens();
    assert!(plan_followup_resolutions(
        &ProtectionTarget::Live(&session),
        &clean,
        &report(vec![suspect(0..clean.text.len(), LeakKind::Uncovered)]),
        Some("aXYz")
    )
    .is_err());
    assert_eq!(session.tokens(), before);
}

#[test]
fn second_batch_preflight_failure_keeps_existing_trace_manifest_and_audit_untouched() {
    for invalid_source in [false, true] {
        let (session, mut clean, mut original) = fixture(false);
        let mut item = suspect(0..5, LeakKind::Uncovered);
        if invalid_source {
            original.replace_range(0..5, "xxxxx");
        } else {
            item.safety_net_id = " \t".into();
        }
        let rows = Arc::new(Mutex::new(vec![]));
        let pipeline = Pipeline::builder()
            .register_safety_net(Reports(Mutex::new(vec![report(vec![
                mismatch(0..5),
                item,
            ])])))
            .redaction_logger(Capture(rows.clone()))
            .build()
            .unwrap();
        let mut trace = ProtectionTraceCollector::new(&original);
        trace
            .record(
                5..26,
                PiiClass::Email,
                GazeLocalProtectionTraceKind::PrimaryPolicyTokenize,
                vec!["primary.fixture".into()],
            )
            .unwrap();
        let before_text = clean.text.clone();
        let before_tokens = session.tokens();
        let before_span = clean.manifest[0].clone();
        assert!(pipeline
            .apply_safety_net_policy(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &mut report(vec![]),
                DocumentKind::Text,
                &[crate::LocaleTag::Global],
                None,
                SafetyNetPolicy::default().decision(),
                Some(&mut trace)
            )
            .is_err());
        assert_eq!(session.tokens(), before_tokens);
        assert_eq!(clean.text, before_text);
        assert_eq!(clean.manifest, [before_span]);
        assert_eq!(trace.items.len(), 1);
        assert_eq!(trace.items[0].raw_span, 5..26);
        assert!(rows.lock().unwrap().is_empty());
    }
}
