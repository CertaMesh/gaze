use super::*;

const RAW: &str = "password: \"left right\"\nmarker";

#[derive(Clone)]
struct Fixed(Vec<Candidate>);
impl crate::Recognizer for Fixed {
    fn id(&self) -> &str {
        "synthetic.fixed"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        _: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self.0.clone())
    }
}
fn candidate(span: Range<usize>, class: PiiClass, id: &str) -> Candidate {
    Candidate::new(
        span,
        class,
        id,
        0.9,
        0,
        None,
        "counter",
        id,
        ConflictTier::None,
        vec![],
    )
}
fn field() -> PiiClass {
    PiiClass::custom("password").unwrap()
}
fn pair() -> Vec<Candidate> {
    vec![
        candidate(0..15, PiiClass::Name, "synthetic.name"),
        candidate(11..21, field(), "synthetic.field"),
    ]
}
fn pipeline(candidates: Vec<Candidate>, enabled: bool) -> Pipeline {
    let mut p = Pipeline::builder()
        .recognizer(Fixed(candidates))
        .rule(crate::rule::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    p.residual_coverage = enabled;
    p
}
fn clean(p: &Pipeline, session: &Session, raw: &str) -> Result<CleanText> {
    p.redact_text_with_manifest_uncached(
        &mut ProtectionTarget::Live(session),
        raw,
        None,
        DocumentKind::Text,
        &[crate::LocaleTag::Global],
        &DictionaryBundle::default(),
        None,
    )
}
#[test]
fn pair_residual_preserves_whole_token_and_exact_raw_union() {
    let old_session = Session::new(crate::Scope::Ephemeral).unwrap();
    let new_session = &old_session;
    let old = clean(&pipeline(pair(), false), &old_session, RAW).unwrap();
    let new = clean(&pipeline(pair(), true), new_session, RAW).unwrap();
    assert_eq!(
        old.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15]
    );
    assert_eq!(
        new.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15, 15..21]
    );
    assert_eq!(
        &new.text[new.manifest[0].clean_span.clone()],
        &old.text[old.manifest[0].clean_span.clone()]
    );
    assert_eq!(&new.text[new.manifest[1].clean_span.end..], &RAW[21..]);
    assert_eq!(new_session.restore_strict_text(&new.text).unwrap(), RAW);
}

fn triple() -> Vec<Candidate> {
    vec![
        candidate(11..15, PiiClass::Name, "synthetic.left"),
        candidate(11..21, field(), "synthetic.field"),
        candidate(16..29, PiiClass::Name, "synthetic.right"),
    ]
}
fn text(document: CleanDocument) -> String {
    let CleanDocument::Text(value) = document else {
        panic!("text result")
    };
    value
}
#[test]
fn pair_and_triple_use_existing_traced_live_staged_and_strict_calls() {
    for (input, expected) in [
        (pair(), vec![0..15, 15..21]),
        (triple(), vec![11..15, 15..16, 16..29]),
    ] {
        let p = pipeline(input, true);
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let locales = [crate::LocaleTag::Global];
        let dictionaries = DictionaryBundle::default();
        let (output, manifest, _, trace) = p
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                RAW,
                &locales,
                &dictionaries,
                SafetyNetPolicy::default(),
            )
            .unwrap();
        assert_eq!(
            manifest
                .iter()
                .map(|s| s.raw_span.clone())
                .collect::<Vec<_>>(),
            expected
        );
        wire_fixture(RAW, &manifest, &trace);
        assert_eq!(trace.len(), manifest.len());
        for (item, span) in trace.iter().zip(&manifest) {
            assert_eq!(
                (item.raw_start(), item.raw_end(), item.class()),
                (span.raw_span.start, span.raw_span.end, &span.class)
            );
            assert_eq!(
                (item.stage(), item.decision(), item.action()),
                ("primary_pipeline", "policy", "tokenize")
            );
        }
        assert_eq!(session.restore_strict_text(&text(output)).unwrap(), RAW);
        let staged_session = Session::new(crate::Scope::Ephemeral).unwrap();
        let mut tx = staged_session.begin_transaction();
        let (output, staged, _) = p
            .clean_transaction_with_safety_net_policy_detect_context(
                &mut tx,
                RawDocument::Text(RAW.into()),
                &locales,
                &dictionaries,
                SafetyNetPolicy::default(),
            )
            .unwrap();
        assert!(staged_session.tokens().is_empty());
        assert_eq!(
            staged
                .iter()
                .map(|s| s.raw_span.clone())
                .collect::<Vec<_>>(),
            expected
        );
        let output = text(output);
        tx.commit().unwrap();
        assert_eq!(staged_session.restore_strict_text(&output).unwrap(), RAW);
        let strict_session = Session::new(crate::Scope::Ephemeral).unwrap();
        let mut tx = strict_session.begin_transaction();
        let protected = p
            .protect_text_transaction(
                &mut tx,
                RAW,
                ProtectionContext::strict(&locales, &dictionaries),
            )
            .unwrap();
        tx.commit().unwrap();
        assert_eq!(strict_session.restore_strict_text(&protected).unwrap(), RAW);
        assert_eq!(strict_session.tokens().len(), expected.len());
    }
}

#[test]
fn all_twenty_five_action_pairs_admit_only_both_tokenize() {
    let actions = [
        Action::Tokenize,
        Action::Preserve,
        Action::Redact,
        Action::Generalize,
        Action::FormatPreserve,
    ];
    for a in actions {
        for b in actions {
            let mut p = pipeline(pair(), true);
            p.rules = vec![
                crate::rule::RuleEntry::new(crate::rule::ClassRule::new(PiiClass::Name, a)),
                crate::rule::RuleEntry::new(crate::rule::DefaultRule::new(b)),
            ];
            let normalized = normalize(RAW);
            let (pool, _) = p
                .registry
                .detect_candidate_pool(
                    &normalized.text,
                    &DetectContext::new(&[crate::LocaleTag::Global], &DictionaryBundle::default()),
                )
                .unwrap();
            let whole = recovery::plan(
                pool,
                &p.registry,
                &normalized,
                RAW,
                &[crate::LocaleTag::Global],
            )
            .unwrap();
            let selected = whole
                .primary
                .iter()
                .chain(&whole.recovered)
                .collect::<Vec<_>>();
            let plan = residual::plan(
                &p,
                &whole.evidence,
                &whole.order,
                &selected,
                &normalized.text,
                RAW,
                &RuleContext::default(),
                &[crate::LocaleTag::Global],
            )
            .unwrap();
            assert_eq!(
                plan.cells.len(),
                usize::from(a == Action::Tokenize && b == Action::Tokenize),
                "{a:?}/{b:?}"
            );
            if a != Action::Tokenize || b != Action::Tokenize {
                let session = Session::new(crate::Scope::Ephemeral).unwrap();
                let new = clean(&p, &session, RAW);
                p.residual_coverage = false;
                let old = clean(&p, &session, RAW);
                match (new, old) {
                    (Ok(new), Ok(old)) => {
                        assert_eq!(new.text, old.text);
                        assert_eq!(new.manifest, old.manifest);
                    }
                    (Err(new), Err(old)) => assert_eq!(new.to_string(), old.to_string()),
                    _ => panic!("ineligible behavior drift"),
                }
            }
        }
    }
}

struct Unknown {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
impl Rule for Unknown {
    fn action(&self, _: &PiiClass, _: &RuleContext) -> Option<Action> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        None
    }
}
struct Wrapped(crate::rule::DefaultRule);
impl Rule for Wrapped {
    fn action(&self, c: &PiiClass, x: &RuleContext) -> Option<Action> {
        self.0.action(c, x)
    }
}
#[test]
fn unknown_order_wrappers_nonmatches_and_clone_preserve_runtime_calls() {
    use crate::rule::{preview, ClassRule, ColumnRule, DefaultRule, RuleEntry};
    for mode in 0..6 {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let unknown = RuleEntry::new(Unknown {
            calls: calls.clone(),
        });
        let default = RuleEntry::new(DefaultRule::new(Action::Tokenize));
        let rules = match mode {
            0 => vec![unknown, default],
            1 => vec![default, unknown],
            2 => vec![
                RuleEntry::new(ClassRule::new(PiiClass::Email, Action::Tokenize)),
                unknown,
                default,
            ],
            3 => vec![RuleEntry::new(Wrapped(DefaultRule::new(Action::Tokenize)))],
            4 => vec![
                RuleEntry::new(ColumnRule::new("secret", Action::Tokenize)),
                unknown,
                default,
            ],
            _ => vec![
                RuleEntry::new(ClassRule::new(PiiClass::Name, Action::Preserve)),
                default,
            ],
        };
        let expected = match mode {
            1 => Some(Action::Tokenize),
            5 => Some(Action::Preserve),
            _ => None,
        };
        assert_eq!(
            preview(&rules, &PiiClass::Name, &RuleContext::default(), |_| {
                Vec::new()
            }),
            expected
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let mut p = pipeline(pair(), true);
        p.rules = rules.clone();
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let output = clean(&p, &session, RAW).unwrap();
        assert_eq!(
            output.manifest.len(),
            if mode == 1 {
                2
            } else if mode == 5 {
                0
            } else {
                1
            }
        );
        let invoked = calls.swap(0, std::sync::atomic::Ordering::SeqCst);
        p.residual_coverage = false;
        clean(&p.clone(), &session, RAW).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), invoked);
    }
}

#[test]
fn builtin_preview_matches_runtime_for_actual_class_and_field_context() {
    use crate::rule::{preview, ClassRule, ColumnRule, DefaultRule, RuleEntry};
    for action in [
        Action::Tokenize,
        Action::Preserve,
        Action::Redact,
        Action::Generalize,
        Action::FormatPreserve,
    ] {
        for class in [
            PiiClass::Name,
            PiiClass::Email,
            field(),
            PiiClass::family("document"),
        ] {
            for field_name in [None, Some("secret"), Some("different")] {
                let context = build_context(field_name);
                let rules = vec![
                    RuleEntry::new(ClassRule::new(PiiClass::Name, action)),
                    RuleEntry::new(ColumnRule::new("secret", action)),
                    RuleEntry::new(DefaultRule::new(Action::Tokenize)),
                ];
                assert_eq!(
                    preview(&rules, &class, &context, |_| Vec::new()),
                    Some(
                        rules
                            .iter()
                            .find_map(|r| r.action(&class, &context))
                            .unwrap()
                    )
                );
            }
        }
    }
    assert_eq!(
        preview(&[], &PiiClass::Name, &RuleContext::default(), |_| Vec::new(
        )),
        Some(Action::Preserve)
    );
}

#[test]
fn duplicate_nested_and_touching_evidence_retains_all_parent_ids_without_hulls() {
    let mut input = pair();
    input.push(input[1].clone());
    input.push(candidate(17..23, field(), "synthetic.nested"));
    input.push(candidate(23..26, PiiClass::Name, "synthetic.touch"));
    let p = pipeline(input, true);
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, RAW).unwrap();
    assert_eq!(session.restore_strict_text(&output.text).unwrap(), RAW);
    let cells = &output.manifest.segment().residuals;
    assert_eq!(
        cells.iter().map(|c| c.raw.clone()).collect::<Vec<_>>(),
        vec![15..17]
    );
    assert_eq!(cells[0].parents.len(), 2);
    assert_ne!(cells[0].parents[0], cells[0].parents[1]);
    // A touching Preserve component does not disable the pair's residual.
    let mut p = pipeline(
        vec![
            pair()[0].clone(),
            pair()[1].clone(),
            candidate(21..22, PiiClass::Email, "synthetic.touch"),
        ],
        true,
    );
    p.rules = vec![
        crate::rule::RuleEntry::new(crate::rule::ClassRule::new(
            PiiClass::Email,
            Action::Preserve,
        )),
        crate::rule::RuleEntry::new(crate::rule::DefaultRule::new(Action::Tokenize)),
    ];
    assert_eq!(
        clean(&p, &session, RAW)
            .unwrap()
            .manifest
            .segment()
            .residuals[0]
            .raw,
        15..21
    );
}

#[test]
fn normalization_keeps_source_scalars_joiners_and_the_original_collision_guard() {
    let raw = "ｐassword: \"le\u{200d}ft right\"\nmarker\u{200d}";
    let p = pipeline(triple(), true);
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, raw).unwrap();
    assert_eq!(session.restore_strict_text(&output.text).unwrap(), raw);
    assert_eq!(
        &raw[output.manifest.segment().residuals[0].raw.clone()],
        " "
    );
    assert!(output.text.ends_with('\u{200d}')); // Outside admitted evidence.
    let raw = "\u{0344}";
    let p = pipeline(
        vec![
            candidate(0..2, PiiClass::Name, "synthetic.a"),
            candidate(2..4, PiiClass::Name, "synthetic.b"),
        ],
        true,
    );
    let empty = Session::new(crate::Scope::Ephemeral).unwrap();
    assert!(clean(&p, &empty, raw).is_err());
    assert!(empty.tokens().is_empty());
    for span in [0..0, 1..2, 0..8] {
        let p = pipeline(vec![candidate(span, PiiClass::Name, "synthetic.bad")], true);
        assert!(clean(&p, &empty, "é").is_err());
    }
}

#[test]
fn actual_collision_bypass_and_conservative_original_fallback_are_distinct() {
    let a = PiiClass::custom("alpha").unwrap();
    let b = PiiClass::custom("beta").unwrap();
    let mut p = Pipeline::builder()
        .recognizer(Fixed(vec![
            candidate(0..5, a.clone(), "synthetic.a"),
            candidate(3..8, b, "synthetic.b"),
        ]))
        .register_collision(
            "synthetic.a",
            crate::CollisionMembership::new("document", "a", 10, Some("cue".into())),
        )
        .register_collision(
            "synthetic.b",
            crate::CollisionMembership::new("document", "b", 20, None),
        )
        .rule(crate::rule::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    p.residual_coverage = true;
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, "xxxxxxxx").unwrap();
    assert_eq!(output.manifest[0].class, a);
    assert_eq!(output.manifest[0].raw_span, 0..5);
    assert_eq!(output.manifest.segment().residuals[0].raw, 5..8);
    // Actual selected CollisionPolicy bypass stays alpha; the conservative original
    // check still requires its standalone family policy and can exclude coverage.
    p.rules.insert(
        0,
        crate::rule::RuleEntry::new(crate::rule::ClassRule::new(
            PiiClass::family("document"),
            Action::Preserve,
        )),
    );
    let output = clean(&p, &session, "xxxxxxxx").unwrap();
    assert_eq!(output.manifest[0].class, a);
    assert!(output.manifest.segment().residuals.is_empty());
    // A family class that is never an actual fallback does not exclude the pair.
    p.registry = pipeline(pair(), true).registry;
    assert_eq!(
        clean(&p, &session, RAW)
            .unwrap()
            .manifest
            .segment()
            .residuals
            .len(),
        1
    );
}

#[test]
fn same_value_whole_and_fragment_keep_distinct_occurrences() {
    let p = pipeline(
        vec![
            candidate(0..3, PiiClass::Email, "synthetic.email"),
            candidate(2..4, PiiClass::Name, "synthetic.partial"),
            candidate(5..6, PiiClass::Name, "synthetic.whole"),
        ],
        true,
    );
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, "xxaa a").unwrap();
    assert_eq!(
        output
            .manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..3, 3..4, 5..6]
    );
    assert_eq!(
        &output.text[output.manifest[1].clean_span.clone()],
        &output.text[output.manifest[2].clean_span.clone()]
    );
    assert!(matches!(
        output.manifest.records()[1].origin,
        Origin::Residual { .. }
    ));
    assert!(matches!(
        output.manifest.records()[2].origin,
        Origin::Selection { .. }
    ));
    assert_ne!(
        output.manifest.records()[1].id,
        output.manifest.records()[2].id
    );
    assert_eq!(session.restore_strict_text(&output.text).unwrap(), "xxaa a");
}

type AuditCalls = Arc<std::sync::Mutex<Vec<(bool, usize)>>>;
struct Audit {
    session: Arc<Session>,
    calls: AuditCalls,
    fail: Option<usize>,
}
impl RedactionLogger for Audit {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), crate::RedactionLogError> {
        let mut calls = self.calls.lock().unwrap();
        let index = calls.len();
        calls.push((
            entry.provenance_stage.as_deref() == Some("primary_pipeline.residual"),
            self.session.tokens().len(),
        ));
        if self.fail == Some(index) {
            return Err(crate::RedactionLogError::Backend(
                "synthetic audit failure".into(),
            ));
        }
        Ok(())
    }
}
#[test]
fn audit_failures_preserve_original_order_and_residual_allocate_before_log() {
    let raw = "xxxxxxxxxxxxxx";
    let input = vec![
        candidate(0..11, field(), "synthetic.parent"),
        candidate(1..2, PiiClass::Name, "synthetic.a"),
        candidate(4..5, PiiClass::Name, "synthetic.b"),
        candidate(7..8, PiiClass::Name, "synthetic.c"),
        candidate(10..14, PiiClass::Name, "synthetic.last"),
    ];
    let baseline_session = Arc::new(Session::new(crate::Scope::Ephemeral).unwrap());
    let baseline_calls = AuditCalls::default();
    let mut baseline = pipeline(input.clone(), true);
    baseline.redaction_loggers.push(Arc::new(Audit {
        session: baseline_session.clone(),
        calls: baseline_calls.clone(),
        fail: None,
    }));
    let out = clean(&baseline, &baseline_session, raw).unwrap();
    assert_eq!(out.manifest.segment().residuals.len(), 4);
    let expected = baseline_calls.lock().unwrap().clone();
    let first = expected.iter().position(|(r, _)| *r).unwrap();
    assert!(expected[first..].iter().all(|(r, _)| *r));
    assert!(expected[first].1 > expected[first - 1].1);
    for fail in 0..expected.len() {
        let session = Arc::new(Session::new(crate::Scope::Ephemeral).unwrap());
        let calls = AuditCalls::default();
        let mut p = pipeline(input.clone(), true);
        p.redaction_loggers.push(Arc::new(Audit {
            session: session.clone(),
            calls: calls.clone(),
            fail: Some(fail),
        }));
        assert!(clean(&p, &session, raw).is_err());
        assert_eq!(*calls.lock().unwrap(), expected[..=fail]);
        assert_eq!(session.tokens().len(), expected[fail].1);
        let staged_session = Arc::new(Session::new(crate::Scope::Ephemeral).unwrap());
        let staged_calls = AuditCalls::default();
        p.redaction_loggers = vec![Arc::new(Audit {
            session: staged_session.clone(),
            calls: staged_calls.clone(),
            fail: Some(fail),
        })];
        let mut tx = staged_session.begin_transaction();
        assert!(p
            .redact_text_with_manifest_uncached(
                &mut ProtectionTarget::Staged(&mut tx),
                raw,
                None,
                DocumentKind::Text,
                &[crate::LocaleTag::Global],
                &DictionaryBundle::default(),
                None
            )
            .is_err());
        drop(tx);
        assert!(staged_session.tokens().is_empty());
        assert_eq!(staged_calls.lock().unwrap().len(), fail + 1); // Audit attempts are not rolled back.
    }
}

#[test]
fn many_gaps_and_parent_incidence_retain_payloads_once() {
    for count in [8, 32, 64] {
        let raw = "x".repeat(4 * count + 1);
        let mut input = (0..count)
            .map(|i| candidate(0..4 * count - i, field(), "synthetic.parent"))
            .collect::<Vec<_>>();
        input.extend(
            (0..count).map(|i| candidate(2 * i + 1..2 * i + 2, PiiClass::Name, "synthetic.whole")),
        );
        input.push(candidate(
            3 * count - 1..4 * count + 1,
            PiiClass::Name,
            "synthetic.last",
        ));
        let p = pipeline(input, true);
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let output = clean(&p, &session, &raw).unwrap();
        let segment = output.manifest.segment();
        assert_eq!(segment.originals.len(), 2 * count + 1);
        let incidence = segment
            .residuals
            .iter()
            .map(|c| c.parents.len())
            .sum::<usize>();
        assert!(
            incidence >= count * count,
            "maximal nested parent incidence is quadratic"
        );
        let payloads = segment.originals.as_ptr();
        for _ in 0..4 {
            output.manifest.projection();
            output.manifest.validate().unwrap();
        }
        assert_eq!(payloads, output.manifest.segment().originals.as_ptr());
        assert_eq!(session.restore_strict_text(&output.text).unwrap(), raw);
        eprintln!(
            "B1 retained originals={} cells={} parent_incidence={} records={} projected_spans={}",
            segment.originals.len(),
            segment.residuals.len(),
            incidence,
            output.manifest.len(),
            output.manifest.projection().spans.len()
        );
    }
}

type NetCalls = Arc<std::sync::Mutex<Vec<(usize, usize, String, usize, usize)>>>;
struct ScriptNet {
    net: usize,
    step: std::sync::Mutex<usize>,
    calls: NetCalls,
}
impl SafetyNet for ScriptNet {
    fn id(&self) -> &str {
        if self.net == 0 {
            "synthetic.net"
        } else {
            "synthetic.observer"
        }
    }
    fn supported_locales(&self) -> &[crate::LocaleTag] {
        &[crate::LocaleTag::Global]
    }
    fn check(
        &self,
        output: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let mut step = self.step.lock().unwrap();
        self.calls.lock().unwrap().push((
            self.net,
            *step,
            output.into(),
            context.manifest as *const Manifest as usize,
            context.manifest.spans.len(),
        ));
        let found = if self.net == 1 {
            vec![]
        } else {
            match *step {
                0 => {
                    assert_eq!(context.manifest.spans.len(), 2);
                    // Identifier class: `er` and `ma` cut `marker`, and the sub-word guard
                    // exempts only identifier classes.
                    vec![LeakSuspect::new(
                        output.len() - 2..output.len(),
                        field(),
                        self.id(),
                        Some(0.99),
                        LeakKind::Uncovered,
                        "synthetic",
                        None,
                    )]
                }
                1 => {
                    let start = output.find("ma").unwrap();
                    vec![LeakSuspect::new(
                        start..start + 2,
                        field(),
                        self.id(),
                        Some(0.99),
                        LeakKind::Uncovered,
                        "synthetic",
                        None,
                    )]
                }
                2 => {
                    let residual = &context.manifest.spans[1];
                    vec![LeakSuspect::new(
                        residual.clean_span.start..residual.clean_span.end + 1,
                        PiiClass::Name,
                        self.id(),
                        Some(0.99),
                        LeakKind::ClassMismatch {
                            pipeline_class: field(),
                            safety_net_class: PiiClass::Name,
                        },
                        "synthetic",
                        None,
                    )]
                }
                3 => vec![],
                _ => panic!("unexpected extra sweep"),
            }
        };
        *step += 1;
        Ok(found)
    }
}
#[test]
fn actual_two_net_sequence_sees_residual_output_and_deletes_its_final_authority() {
    let mut p = pipeline(pair(), true);
    let calls = NetCalls::default();
    for net in 0..2 {
        p.safety_nets.push(Arc::new(ScriptNet {
            net,
            step: std::sync::Mutex::new(0),
            calls: calls.clone(),
        }));
    }
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let (output, manifest, _, trace) = p
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            RAW,
            &[crate::LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    wire_fixture(RAW, &manifest, &trace);
    let observed = calls.lock().unwrap();
    assert_eq!(observed.len(), 8);
    for (phase, pair) in observed.chunks_exact(2).enumerate() {
        assert_eq!(pair[0].1, phase);
        assert_eq!(pair[0].2, pair[1].2);
        assert_eq!(pair[0].3, pair[1].3);
        assert_eq!(pair[0].4, [2, 3, 4, 3][phase]);
        if phase > 0 {
            assert_ne!(pair[0].2, observed[(phase - 1) * 2].2);
        }
    }
    assert!(!observed[0].2.contains(" right"));
    assert_eq!(
        manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15, 23..25, 27..29]
    );
    assert_eq!(
        trace
            .iter()
            .filter(|t| t.stage() == "safety_net" && t.action() == "tokenize")
            .count(),
        2
    );
    assert_eq!(
        trace
            .iter()
            .filter(|t| t.decision() == "fallback_redact")
            .count(),
        1
    );
    assert_eq!(
        trace.iter().filter(|t| t.action() == "tokenize").count(),
        manifest.len()
    );
    assert_eq!(
        session.restore_strict_text(&text(output)).unwrap(),
        format!("{}{}", &RAW[..15], &RAW[22..])
    );
}

struct RejectNet {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    error: bool,
    inside_owned: bool,
}
impl SafetyNet for RejectNet {
    fn id(&self) -> &str {
        "synthetic.reject"
    }
    fn supported_locales(&self) -> &[crate::LocaleTag] {
        &[crate::LocaleTag::Global]
    }
    fn check(
        &self,
        _: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(context.manifest.spans.len(), 2);
        if self.error {
            Err(SafetyNetError::InvalidOutput {
                message: "synthetic failure".into(),
            })
        } else {
            Ok(vec![LeakSuspect::new(
                if self.inside_owned {
                    context.manifest.spans[1].clean_span.clone()
                } else {
                    let end = context.manifest.spans[1].clean_span.end;
                    end..end + 1
                },
                PiiClass::Name,
                self.id(),
                Some(0.99),
                LeakKind::Uncovered,
                "synthetic",
                None,
            )])
        }
    }
}
#[test]
fn strict_nets_deny_actual_residual_output_and_owned_prefix_composition_restores() {
    for (error, inside_owned) in [(false, false), (true, false), (false, true)] {
        let mut p = pipeline(pair(), true);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        p.safety_nets.push(Arc::new(RejectNet {
            calls: calls.clone(),
            error,
            inside_owned,
        }));
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        assert_eq!(
            p.protect_text_transaction(
                &mut tx,
                RAW,
                ProtectionContext::strict(
                    &[crate::LocaleTag::Global],
                    &DictionaryBundle::default()
                )
            )
            .is_err(),
            error || !inside_owned
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(tx);
        assert!(session.tokens().is_empty());
    }
    let p = pipeline(pair(), true);
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let prefix = session.tokenize(&PiiClass::Name, "seed").unwrap();
    let mut tx = session.begin_transaction();
    let output = p
        .protect_text_transaction(
            &mut tx,
            &format!("{prefix}{RAW}"),
            ProtectionContext::strict(&[crate::LocaleTag::Global], &DictionaryBundle::default()),
        )
        .unwrap();
    tx.commit().unwrap();
    assert_eq!(
        session.restore_strict_text(&output).unwrap(),
        format!("seed{RAW}")
    );
}

#[test]
fn residual_parent_order_cannot_replace_the_legacy_representative() {
    let mut input = pair();
    let mut duplicate = input[1].clone();
    duplicate.recognizer_id = "synthetic.zz".into();
    input.push(duplicate);
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&pipeline(input, true), &session, RAW).unwrap();
    let mut segment = output.manifest.segment().clone();
    let ids = segment.residuals[0].parents.clone();
    assert_eq!(ids.len(), 2);
    let a = segment
        .residual_order
        .iter()
        .position(|id| *id == ids[0])
        .unwrap();
    let b = segment
        .residual_order
        .iter()
        .position(|id| *id == ids[1])
        .unwrap();
    segment.residual_order.swap(a, b);
    segment.residuals[0].parents.swap(0, 1);
    segment.residuals[0].representative = ids[1];
    let mut bad = Ledger::new(segment);
    for record in output.manifest.records() {
        bad.insert(record.clone());
    }
    assert!(
        bad.validate().is_err(),
        "a self-consistent replacement order must not forge representative authority"
    );
}

/// Everything else in this file reaches the engine through the `pipeline`
/// helper, which sets `residual_coverage` explicitly. That is fine for proving
/// behavior and useless for proving activation: a feature that only the tests
/// switch on protects nothing.
///
/// So build the pipeline the way an adopter does, touch no internal field, and
/// assert both that the default is on and that it actually covers. If someone
/// flips the default back, this is the test that says so.
#[test]
fn residual_coverage_is_on_by_default_with_no_test_only_switch() {
    let p = Pipeline::builder()
        .recognizer(Fixed(pair()))
        .rule(crate::rule::DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    assert!(
        p.residual_coverage,
        "residual coverage must be the shipped default, not an opt-in"
    );

    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, RAW).unwrap();
    assert_eq!(
        output
            .manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15, 15..21],
        "the default build must emit the residual, not just admit it"
    );
    assert_eq!(session.restore_strict_text(&output.text).unwrap(), RAW);
}

/// Success measured only over eligible components is not coverage accounting.
/// A real document mixes them, so pin both sides of the ledger on one input:
/// what the admitted component gained, and exactly which admitted-union bytes
/// the excluded component still ships in the clear.
///
/// The exclusion is the honest one from the conservative contract -- component
/// B's winner is tokenized exactly as before, but a suppressed member of the
/// same component previews Preserve, so the component is ineligible and its gap
/// keeps Stage A byte for byte. Those gap bytes are a disclosed B1 limitation,
/// not something it quietly protects. The test fails in both directions: if
/// coverage silently widens to the excluded component, or silently narrows on
/// the admitted one.
#[test]
fn coverage_accounting_names_the_excluded_component_and_its_uncovered_bytes() {
    use crate::rule::{ClassRule, DefaultRule};

    let head = "password: \"left right\"\nmarker ";
    let tail = "password: \"left right\"";
    let raw = format!("{head}{tail}");
    let base = head.len();
    let preserved = PiiClass::custom("apikey").unwrap();

    // A: Name 0..15 over custom("password") 11..21, both known-Tokenize.
    // B: the same geometry, but its suppressed member previews Preserve.
    let input = vec![
        candidate(0..15, PiiClass::Name, "synthetic.name"),
        candidate(11..21, field(), "synthetic.field"),
        candidate(base..base + 15, PiiClass::Name, "synthetic.name.b"),
        candidate(base + 11..base + 21, preserved.clone(), "synthetic.apikey"),
    ];
    let build = |enabled: bool| {
        let mut p = Pipeline::builder()
            .recognizer(Fixed(input.clone()))
            .rule(ClassRule::new(preserved.clone(), Action::Preserve))
            .rule(DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        p.residual_coverage = enabled;
        p
    };

    let stage_a_session = Session::new(crate::Scope::Ephemeral).unwrap();
    let stage_a = clean(&build(false), &stage_a_session, &raw).unwrap();
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let covered = clean(&build(true), &session, &raw).unwrap();

    let spans = |c: &CleanText| {
        c.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>()
    };
    let (before, after) = (spans(&stage_a), spans(&covered));
    assert_eq!(
        before,
        vec![0..15, base..base + 15],
        "both components must select their whole exactly as Stage A does"
    );

    // Exactly one new protected interval, and it is component A's gap.
    let gained = after
        .iter()
        .filter(|span| !before.contains(span))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(gained, vec![15..21], "coverage changed outside component A");
    assert!(
        before.iter().all(|span| after.contains(span)),
        "an admitted component must never lose a Stage A selection: {before:?} -> {after:?}"
    );

    // Name the admitted-union bytes component B still ships in the clear.
    let uncovered = base + 15..base + 21;
    assert!(
        covered.text.contains(&raw[uncovered.clone()]),
        "excluded gap {uncovered:?} ({:?}) must stay visibly uncovered, not quietly protected",
        &raw[uncovered.clone()]
    );
    assert!(
        !after
            .iter()
            .any(|span| span.start < uncovered.end && uncovered.start < span.end),
        "excluded gap {uncovered:?} must not be claimed by any emitted span"
    );
    eprintln!(
        "B1 accounting admitted=0..21 covered={after:?} excluded={:?} uncovered={uncovered:?}",
        base..base + 21
    );

    assert_eq!(session.restore_strict_text(&covered.text).unwrap(), raw);
}

/// The benchmark reaches this engine through production assembly, not through a
/// hand-built `RuleEntry`: scripts/bench/run_no_opf_benchmark.py:474-486 runs
/// clean_for_bench, which registers exactly `ClassRule` / `ColumnRule` /
/// `DefaultRule` values (crates/gaze-recognizers/examples/clean_for_bench.rs:775-812)
/// through `gaze_assembly` (crates/gaze-assembly/src/lib.rs:114-129), whose
/// `AssemblyBuilder::rule` forwards them as a generic `R: Rule + 'static` to
/// `PipelineBuilder::rule` (crates/gaze-assembly/src/registration.rs:61-67).
/// `PipelineBuilder::rule` is the concrete-to-erased boundary where the
/// immutable preview is captured.
///
/// That forwarding is the single point of silent failure for the whole feature.
/// If it ever stops preserving the preview -- boxing the rule, wrapping it,
/// taking `Arc<dyn Rule>` -- `rule::preview` answers Unknown for every class,
/// the planner admits nothing, and B1 protects zero bytes in production while
/// every other test in this file still passes. Reproduce the generic hop here,
/// where preview is visible, and pin the coverage it is supposed to produce.
#[test]
fn generic_production_registration_keeps_builtins_previewable_and_covering() {
    use crate::rule::{preview, ClassRule, ColumnRule, DefaultRule};

    // Same shape as AssemblyBuilder::rule: the concrete type survives only
    // because the parameter stays generic all the way to PipelineBuilder::rule.
    fn register<R: Rule + 'static>(
        builder: crate::PipelineBuilder,
        rule: R,
    ) -> crate::PipelineBuilder {
        builder.rule(rule)
    }

    let mut builder = Pipeline::builder().recognizer(Fixed(pair()));
    builder = register(builder, ClassRule::new(PiiClass::Name, Action::Tokenize));
    builder = register(builder, ColumnRule::new("password", Action::Tokenize));
    builder = register(builder, DefaultRule::new(Action::Tokenize));
    let mut p = builder.build().unwrap();

    // Every effective class these fixtures can present stays known-Tokenize, in
    // and out of a field context. A hypothetical unused class must not silently
    // disable an otherwise fully admitted component.
    for class in [
        PiiClass::Name,
        PiiClass::Email,
        field(),
        PiiClass::family("document"),
    ] {
        for field_name in [None, Some("password"), Some("other")] {
            assert_eq!(
                preview(&p.rules, &class, &build_context(field_name), |family| {
                    p.registry.family_member_classes(family)
                }),
                Some(Action::Tokenize),
                "{class:?} with field {field_name:?} lost static recognizability \
                 across the generic production registration hop"
            );
        }
    }

    // The consequence that actually matters: real coverage, not just a preview.
    p.residual_coverage = true;
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let covered = clean(&p, &session, RAW).unwrap();
    assert_eq!(
        covered
            .manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..15, 15..21],
        "production-shaped registration must still admit the residual"
    );
    assert_eq!(session.restore_strict_text(&covered.text).unwrap(), RAW);
}

/// `SOURCE_ID_PATTERN` from scripts/bench/gaze_bench_score.py:397, spelled out:
/// a first `[a-z][a-z0-9]*` part, then `[._:/-]`-joined `[a-z0-9]+` parts, at
/// most 128 characters. The pattern only admits ASCII, so byte length is
/// character length here.
fn is_metadata_only_source_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 128 {
        return false;
    }
    let alnum = |part: &str| {
        part.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    let mut parts = value.split(['.', '_', ':', '/', '-']);
    let first = parts.next().expect("split always yields one part");
    first.starts_with(|c: char| c.is_ascii_lowercase())
        && alnum(first)
        && parts.all(|part| !part.is_empty() && alnum(part))
}

/// Assert the unchanged benchmark validator's closed contract, clause for
/// clause, against a real emission: scripts/bench/gaze_bench_score.py:381-396
/// (exact key sets, the four allowed stage/decision/action tuples, the
/// source-ID shape) and :663-749 (canonical class, non-empty sorted
/// duplicate-free source IDs, in-bounds char-boundary geometry, sorted
/// disjoint trace spans, and tokenize/manifest agreement by multiplicity).
///
/// B1's whole compatibility claim is that a residual emission survives that
/// untouched scorer, so this fixture has to fail when the projection drifts.
/// Serializing and printing it would prove nothing.
fn wire_fixture(raw: &str, manifest: &[EmittedTokenSpan], trace: &[GazeLocalProtectionTraceItem]) {
    let boundaries = raw
        .char_indices()
        .map(|(index, _)| index)
        .chain([raw.len()])
        .collect::<std::collections::BTreeSet<_>>();
    let mut previous_raw_end = 0usize;
    let mut tokenize_items = BTreeMap::<(usize, usize, String), usize>::new();
    let mut protected_raw_values = Vec::new();
    let mut source_identifiers = Vec::<&str>::new();

    for (index, item) in trace.iter().enumerate() {
        let class = item.class().to_canonical_str();
        assert!(
            ["email", "name", "location", "organization"].contains(&class.as_str())
                || class
                    .strip_prefix("custom:")
                    .is_some_and(|rest| !rest.is_empty()),
            "trace[{index}].class: {class} is not a canonical PiiClass representation"
        );
        let projection = (item.stage(), item.decision(), item.action());
        assert!(
            matches!(projection.2, "tokenize" | "redact"),
            "trace[{index}].action: unknown action {}",
            projection.2
        );
        assert!(
            [
                ("primary_pipeline", "policy", "tokenize"),
                ("safety_net", "resolve", "tokenize"),
                ("safety_net", "redact", "redact"),
                ("safety_net", "fallback_redact", "redact"),
            ]
            .contains(&projection),
            "trace[{index}].provenance: {projection:?} is outside the closed scorer set"
        );

        let sources = item.source_ids();
        assert!(
            !sources.is_empty(),
            "trace[{index}].provenance.source_ids: expected non-empty metadata IDs"
        );
        for id in sources {
            assert!(
                is_metadata_only_source_id(id),
                "trace[{index}].provenance.source_ids: {id} is not a metadata-only stable identifier"
            );
        }
        assert!(
            sources.windows(2).all(|pair| pair[0] < pair[1]),
            "trace[{index}].provenance.source_ids: expected sorted duplicate-free IDs, got {sources:?}"
        );

        let (start, end) = (item.raw_start(), item.raw_end());
        assert!(
            start < end && end <= raw.len(),
            "trace[{index}]: invalid original-text bounds {start}:{end}"
        );
        assert!(
            boundaries.contains(&start) && boundaries.contains(&end),
            "trace[{index}]: span endpoints are not original-text UTF-8 char boundaries"
        );
        assert!(
            start >= previous_raw_end,
            "trace[{index}]: trace spans must be sorted and disjoint"
        );
        previous_raw_end = end;
        protected_raw_values.push(&raw[start..end]);
        source_identifiers.extend(sources.iter().map(String::as_str));
        if projection.2 == "tokenize" {
            *tokenize_items.entry((start, end, class)).or_default() += 1;
        }
    }

    let mut manifest_items = BTreeMap::<(usize, usize, String), usize>::new();
    for span in manifest {
        *manifest_items
            .entry((
                span.raw_span.start,
                span.raw_span.end,
                span.class.to_canonical_str(),
            ))
            .or_default() += 1;
    }
    assert_eq!(
        tokenize_items, manifest_items,
        "tokenize trace items must agree 1:1 with the final manifest by multiplicity"
    );

    // The scorer loads its committed vocabulary from the embedded rulepacks;
    // these synthetic recognizer IDs sit outside it on purpose, so the rule that
    // still bites is the leak one: an out-of-vocabulary ID must never be
    // protected raw text.
    for id in source_identifiers {
        assert!(
            !protected_raw_values.contains(&id),
            "source ID {id} leaks protected raw text"
        );
    }
}

#[test]
fn forged_residual_ids_bounds_parent_membership_and_ownership_fail_closed() {
    let mut input = pair();
    input.push(input[1].clone());
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&pipeline(input, true), &session, RAW).unwrap();
    for mode in 0..8 {
        let mut segment = output.manifest.segment().clone();
        let mut records = output.manifest.records().to_vec();
        match mode {
            0 => {
                segment.residuals[0].parents.pop();
            }
            1 => segment.residuals[0].raw.start -= 1,
            2 => segment.residuals[0].raw.end += 1,
            3 => records[1].owned = false,
            4 => {
                records[1].origin = Origin::Residual {
                    segment: 0,
                    residual: 99,
                }
            }
            5 => records[1].action = Some(Action::Preserve),
            // The emitted class is what reaches the public manifest span, the audit
            // row and the scorer, so it has to be pinned to its cell independently
            // of geometry and ownership.
            6 => records[1].emitted.class = PiiClass::Email,
            // Forge the record's raw span while leaving the cell alone, so only the
            // record-to-cell geometry term of the residual disposition can reject it.
            _ => records[1].emitted.raw_span.end += 1,
        }
        let mut bad = Ledger::new(segment);
        for r in records {
            bad.insert(r);
        }
        assert!(bad.validate().is_err(), "corrupt residual mode{mode}");
    }
}

/// The public fragment discriminator and the internal origin are two spellings
/// of one fact, derived at a single site. Consumers decide whether to index a
/// replacement as an entity on the strength of the public one, so a record where
/// they disagree is forged or drifted state and must not validate.
///
/// Both directions matter: a whole relabelled as a fragment loses a searchable
/// entity, and a fragment relabelled as whole is indexed as an entity it is not.
#[test]
fn a_record_whose_two_origins_disagree_fails_closed() {
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&pipeline(pair(), true), &session, RAW).unwrap();
    let records = output.manifest.records().to_vec();
    let fragment = records
        .iter()
        .position(|r| matches!(r.origin, Origin::Residual { .. }))
        .expect("the pair fixture emits one residual");
    let whole = records
        .iter()
        .position(|r| matches!(r.origin, Origin::Selection { .. }))
        .expect("the pair fixture emits one whole");
    assert!(records[fragment].emitted.origin.is_residual_fragment());
    assert!(records[whole].emitted.origin.is_whole());

    for (label, index, forged) in [
        (
            "fragment claiming to be whole",
            fragment,
            gaze_types::EmittedTokenOrigin::Whole,
        ),
        (
            "whole claiming to be a fragment",
            whole,
            gaze_types::EmittedTokenOrigin::ResidualFragment,
        ),
    ] {
        let mut bad = Ledger::new(output.manifest.segment().clone());
        for (position, record) in records.iter().enumerate() {
            let mut record = record.clone();
            if position == index {
                record.emitted.origin = forged;
            }
            bad.insert(record);
        }
        assert!(bad.validate().is_err(), "{label} must not validate");
    }
}

/// Two adjacent residual cells can agree on representative, class and family and
/// still cover different parent sets, so the parent set has to stay in the
/// coalescing key. Arbitration here selects `11..15` and `35..45`, leaving the
/// admitted union of `synthetic.wide` split into `15..25` (one parent) and
/// `25..35` (two, once `synthetic.inner` becomes active). Merging them would
/// hand one of the two ranges a parent list that is not true of it.
#[test]
fn adjacent_cells_sharing_a_representative_keep_their_distinct_parent_sets() {
    let raw = "x".repeat(50);
    let input = vec![
        candidate(11..15, PiiClass::Name, "synthetic.left"),
        candidate(11..40, field(), "synthetic.wide"),
        candidate(25..40, field(), "synthetic.inner"),
        candidate(35..45, PiiClass::Name, "synthetic.right"),
    ];
    let p = pipeline(input, true);
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let output = clean(&p, &session, &raw).unwrap();
    assert_eq!(
        output
            .manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![11..15, 15..25, 25..35, 35..45]
    );
    let cells = &output.manifest.segment().residuals;
    assert_eq!(
        cells.iter().map(|c| c.raw.clone()).collect::<Vec<_>>(),
        vec![15..25, 25..35],
        "a shared representative must not coalesce two different parent sets"
    );
    assert_eq!(cells[0].raw.end, cells[1].raw.start);
    assert_eq!(cells[0].representative, cells[1].representative);
    assert_eq!(cells[0].class, cells[1].class);
    assert_eq!(cells[0].family, cells[1].family);
    assert_eq!(cells[0].parents.len(), 1);
    assert_eq!(cells[1].parents.len(), 2);
    assert_eq!(cells[1].parents[0], cells[0].parents[0]);
    assert_eq!(session.restore_strict_text(&output.text).unwrap(), raw);
}

#[test]
fn preview_disagreement_never_overrides_action_or_allocates_a_residual() {
    struct Fault {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        limit: usize,
    }
    impl Rule for Fault {
        fn action(&self, _: &PiiClass, _: &RuleContext) -> Option<Action> {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(if call < self.limit + 1 {
                Action::Tokenize
            } else {
                Action::Preserve
            })
        }
    }
    for limit in 0..2 {
        let mut p = pipeline(pair(), true);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        p.rules[0].inject_runtime(Fault {
            calls: calls.clone(),
            limit,
        });
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let err = clean(&p, &session, RAW).err().unwrap();
        assert!(err.to_string().contains("preview mismatch"));
        assert_eq!(session.tokens().len(), limit);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), limit + 2);
    }
}

#[test]
fn residual_allocation_failure_occurs_after_original_effects_before_residual_audit() {
    let mut input = pair();
    input[1].class = PiiClass::Custom("___".into());
    let mut p = pipeline(input, true);
    let session = Arc::new(Session::new(crate::Scope::Ephemeral).unwrap());
    let calls = AuditCalls::default();
    p.redaction_loggers.push(Arc::new(Audit {
        session: session.clone(),
        calls: calls.clone(),
        fail: None,
    }));
    assert!(clean(&p, &session, RAW).is_err());
    assert_eq!(session.tokens().len(), 1);
    assert!(calls.lock().unwrap().iter().all(|(residual, _)| !*residual));
}

#[test]
fn duplicate_normalized_parents_consume_one_raw_scalar_and_tied_order_is_real() {
    let raw = "\u{0344}x";
    assert_eq!(normalize(raw).text, "\u{0308}\u{0301}x");
    let parent = candidate(2..5, field(), "synthetic.parent");
    let p = pipeline(
        vec![
            candidate(0..4, PiiClass::Name, "synthetic.name"),
            parent.clone(),
            parent,
        ],
        true,
    );
    let session = Session::new(crate::Scope::Ephemeral).unwrap();
    let out = clean(&p, &session, raw).unwrap();
    assert_eq!(
        out.manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..2, 2..3]
    );
    assert_eq!(out.manifest.segment().residuals[0].parents.len(), 2);
    assert_eq!(session.restore_strict_text(&out.text).unwrap(), raw);
    assert_eq!(normalize("e\u{0301}").text, "e\u{0301}");
    for reverse in [false, true] {
        let a = candidate(2..5, PiiClass::custom("alpha").unwrap(), "synthetic.same");
        let b = candidate(2..5, PiiClass::custom("beta").unwrap(), "synthetic.same");
        let mut parents = if reverse { vec![b, a] } else { vec![a, b] };
        let expected = parents[0].class.clone();
        parents.insert(0, candidate(0..3, PiiClass::Name, "synthetic.name"));
        let out = clean(&pipeline(parents, true), &session, "xxxxx").unwrap();
        assert_eq!(out.manifest.segment().residuals[0].class, expected);
    }
}

#[test]
fn planning_counts_real_preview_and_sweep_operations() {
    let count = 64;
    let raw = "x".repeat(4 * count + 1);
    let mut input = (0..count)
        .map(|i| candidate(0..4 * count - i, field(), "synthetic.parent"))
        .collect::<Vec<_>>();
    input.extend(
        (0..count).map(|i| candidate(2 * i + 1..2 * i + 2, PiiClass::Name, "synthetic.whole")),
    );
    input.push(candidate(
        3 * count - 1..4 * count + 1,
        PiiClass::Name,
        "synthetic.last",
    ));
    let p = pipeline(input, true);
    let normalized = normalize(&raw);
    let dictionaries = DictionaryBundle::default();
    let locales = [crate::LocaleTag::Global];
    let (pool, _) = p
        .registry
        .detect_candidate_pool(
            &normalized.text,
            &DetectContext::new(&locales, &dictionaries),
        )
        .unwrap();
    let whole = recovery::plan(pool, &p.registry, &normalized, &raw, &locales).unwrap();
    let by_span = whole
        .primary
        .iter()
        .chain(&whole.recovered)
        .map(|c| ((c.span.start, c.span.end), c))
        .collect::<BTreeMap<_, _>>();
    let selected = whole
        .evidence
        .selections
        .iter()
        .map(|s| by_span[&(s.raw.start, s.raw.end)])
        .collect::<Vec<_>>();
    let plan = residual::plan(
        &p,
        &whole.evidence,
        &whole.order,
        &selected,
        &normalized.text,
        &raw,
        &RuleContext::default(),
        &locales,
    )
    .unwrap();
    assert_eq!(plan.work.preview_queries, 2);
    assert_eq!(plan.work.active_parent_visits, 4160);
    assert_eq!(
        plan.cells.iter().map(|c| c.parents.len()).sum::<usize>(),
        4160
    );
    eprintln!(
        "B1 work preview_queries={} endpoint_cells={} active_parent_visits={}",
        plan.work.preview_queries, plan.work.endpoint_cells, plan.work.active_parent_visits
    );
}
