//! Synthetic conformance only: no vendor API, accuracy, throughput, or model claim.
//! See support header for trusted synchronous callback and test-only cap limits.

#[path = "support/span_batch_probe.rs"]
mod support;
use gaze::*;
use gaze_types::DetectError;
use std::result::Result;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use support::*;

const EMAIL: &str = "alice@example.invalid";
const PERSON: &str = "Dr. Schmidt";
fn raw(start: i64, end: i64, label: &str) -> RawSpanV1 {
    RawSpanV1 {
        start_utf16: start,
        end_utf16: end,
        label: label.into(),
        score: 0.9,
        origin: "neural".into(),
    }
}
fn surface(input: &str, needle: &str, label: &str) -> RawBatchV1 {
    input
        .match_indices(needle)
        .map(|(start, matched)| {
            raw(
                i64::try_from(input[..start].encode_utf16().count()).unwrap(),
                i64::try_from(input[..start + matched.len()].encode_utf16().count()).unwrap(),
                label,
            )
        })
        .collect()
}
fn detect(probe: &ProbeRecognizer, input: &str) -> Result<Vec<Candidate>, DetectError> {
    probe.detect(
        input,
        &DetectContext::new(&[LocaleTag::Global], &DictionaryBundle::default()),
    )
}
fn check(net: &ProbeSafetyNet, input: &str) -> Result<Vec<LeakSuspect>, SafetyNetError> {
    net.check(
        input,
        SafetyNetContext::new(
            &Manifest::default(),
            &[LocaleTag::Global],
            DocumentKind::Text,
            None,
            None,
        ),
    )
}
fn constant(batch: RawBatchV1) -> Backend {
    Backend::new(move |_| Ok(batch.clone()))
}
fn counted(f: impl Fn(&str) -> RawBatchV1 + Send + Sync + 'static) -> (Backend, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    (
        Backend::new(move |input| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(f(input))
        }),
        count,
    )
}

#[test]
fn utf16_surrogate_interior_rejects_and_astral_offsets_map_to_bytes() {
    let input = "😀 Dr. Schmidt";
    assert_eq!(input.len(), 16);
    assert_eq!(input.encode_utf16().count(), 14);
    assert_eq!(
        validate(input, vec![raw(3, 14, "PERSON")]).unwrap()[0].span,
        5..16
    );
    assert_eq!(
        validate(input, vec![raw(1, 2, "PERSON")]),
        Err(ProbeError::SplitSurrogate),
        "surrogate-interior must fail"
    );
    assert_eq!(
        validate(input, vec![raw(0, 1, "PERSON")]),
        Err(ProbeError::SplitSurrogate)
    );
}
#[test]
fn negative_empty_reversed_overflow_and_past_end_bounds_reject() {
    for (start, end) in [
        (-1, 1),
        (0, 0),
        (2, 1),
        (0, 4),
        (0, i64::MAX),
        (i64::MIN, 1),
    ] {
        assert_eq!(
            validate("abc", vec![raw(start, end, "PERSON")]),
            Err(ProbeError::InvalidBounds)
        );
    }
    assert_eq!(
        validate("abc", vec![raw(0, 3, "PERSON")]).unwrap()[0].span,
        0..3
    );
    assert!(validate("", vec![]).unwrap().is_empty());
}
#[test]
fn nonfinite_and_out_of_range_scores_fail_before_threshold() {
    for score in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
        let mut item = raw(0, 1, "PERSON");
        item.score = score;
        assert_eq!(
            validate("a", vec![item.clone()]),
            Err(ProbeError::InvalidScore),
            "nonfinite-score must fail"
        );
        assert!(detect(&ProbeRecognizer::new(constant(vec![item])), "a").is_err());
    }
    let nan = f64::NAN;
    assert!([nan].into_iter().any(|score| !score.is_finite()));
    assert_eq!([nan].into_iter().filter(|score| *score >= 0.5).count(), 0);
    for score in [0.0, 1.0] {
        let mut item = raw(0, 1, "PERSON");
        item.score = score;
        assert!(validate("a", vec![item]).is_ok());
    }
}
#[test]
fn excluded_and_below_threshold_malformed_entries_fail_whole_batch() {
    let good = raw(0, 1, "PERSON");
    for mut bad in [raw(2, 9, "EMAIL"), raw(2, 3, "EMAIL"), raw(0, 1, "EMAIL")] {
        if bad.end_utf16 == 3 {
            bad.score = f64::NAN;
        }
        let probe = ProbeRecognizer::new(constant(vec![good.clone(), bad]));
        assert!(
            detect(&probe, "abc").is_err(),
            "excluded-malformed must fail whole batch"
        );
    }
    let mut below = raw(1, 99, "PERSON");
    below.score = 0.1;
    assert!(detect(&ProbeRecognizer::new(constant(vec![good, below])), "abc").is_err());
}
#[test]
fn flat_nested_street_address_house_number_is_explicitly_rejected() {
    let input = "Example Street 42";
    assert_eq!(input.len(), 17);
    assert_eq!(
        validate(
            input,
            vec![raw(0, 17, "STREET_ADDRESS"), raw(15, 17, "HOUSE_NUMBER")]
        ),
        Err(ProbeError::Overlap)
    );
}
#[test]
fn sorting_is_deterministic_adjacency_allowed_duplicates_and_overlap_reject() {
    let batch = vec![raw(2, 3, "EMAIL"), raw(0, 2, "PERSON")];
    let expected = validate("abc", batch.clone()).unwrap();
    assert_eq!(expected[0].span, 0..2);
    assert_eq!(expected[1].span, 2..3);
    assert_eq!(validate("abc", batch).unwrap(), expected);
    for batch in [
        vec![raw(0, 2, "PERSON"), raw(0, 2, "PERSON")],
        vec![raw(0, 2, "PERSON"), raw(1, 3, "EMAIL")],
    ] {
        assert_eq!(validate("abc", batch), Err(ProbeError::Overlap));
    }
}
#[test]
fn cap_overflow_rejects_instead_of_truncating_valid_spans() {
    let input = "a".repeat(BATCH_LIMIT + 1);
    let batch: Vec<_> = (0..=BATCH_LIMIT)
        .map(|n| raw(n.try_into().unwrap(), (n + 1).try_into().unwrap(), "PERSON"))
        .collect();
    assert_eq!(
        validate(&input, batch.clone()),
        Err(ProbeError::BatchTooLarge),
        "cap-overflow must reject"
    );
    assert_eq!(
        validate(&input, batch[..BATCH_LIMIT].to_vec())
            .unwrap()
            .len(),
        BATCH_LIMIT
    );
    assert!(matches!(
        check(&ProbeSafetyNet(constant(batch)), &input),
        Err(SafetyNetError::InvalidOutput { .. })
    ));
}
#[test]
fn input_cap_prevents_callback_and_metadata_caps_precede_parsing() {
    let (backend, calls) = counted(|_| vec![]);
    let probe = ProbeRecognizer::new(backend);
    assert!(detect(&probe, &"a".repeat(INPUT_LIMIT)).unwrap().is_empty());
    assert!(detect(&probe, &"a".repeat(INPUT_LIMIT + 1)).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (backend, calls) = counted(|_| panic!("oversized input reached backend"));
    assert!(
        matches!(check(&ProbeSafetyNet(backend), &"a".repeat(INPUT_LIMIT + 1)), Err(SafetyNetError::InputTooLarge { limit: INPUT_LIMIT, actual }) if actual == INPUT_LIMIT + 1)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for (label, origin, expected) in [
        (
            "x".repeat(LABEL_LIMIT + 1),
            "neural".into(),
            ProbeError::MetadataTooLarge,
        ),
        (
            "PERSON".into(),
            "x".repeat(ORIGIN_LIMIT + 1),
            ProbeError::MetadataTooLarge,
        ),
        (
            "x".repeat(LABEL_LIMIT),
            "neural".into(),
            ProbeError::UnknownLabel,
        ),
        (
            "PERSON".into(),
            "x".repeat(ORIGIN_LIMIT),
            ProbeError::UnknownOrigin,
        ),
    ] {
        let mut item = raw(0, 1, &label);
        item.origin = origin;
        assert_eq!(validate("a", vec![item]), Err(expected));
    }
}
#[test]
fn error_diagnostics_and_output_provenance_never_include_unchecked_metadata() {
    let canary = "SYNTHETIC-SOURCE-CANARY";
    for label_field in [true, false] {
        let mut item = raw(0, 1, "PERSON");
        if label_field {
            item.label = canary.into();
        } else {
            item.origin = canary.into();
        }
        let error = validate(canary, vec![item.clone()]).unwrap_err();
        assert_eq!(
            error,
            if label_field {
                ProbeError::UnknownLabel
            } else {
                ProbeError::UnknownOrigin
            }
        );
        let primary =
            detect(&ProbeRecognizer::new(constant(vec![item.clone()])), canary).unwrap_err();
        let net = check(&ProbeSafetyNet(constant(vec![item])), canary).unwrap_err();
        for diagnostic in [
            format!("{error} {error:?}"),
            format!("{primary} {primary:?}"),
            format!("{net} {net:?}"),
        ] {
            assert!(!diagnostic.contains(canary));
        }
    }
    for error in [
        ProbeError::InputTooLarge,
        ProbeError::BatchTooLarge,
        ProbeError::MetadataTooLarge,
        ProbeError::InvalidBounds,
        ProbeError::SplitSurrogate,
        ProbeError::InvalidScore,
        ProbeError::UnknownLabel,
        ProbeError::UnknownOrigin,
        ProbeError::Overlap,
        ProbeError::BackendFailure,
    ] {
        assert!(!format!("{error} {error:?}").contains(canary));
    }
    let candidate = detect(
        &ProbeRecognizer::new(constant(vec![raw(0, 1, "PERSON")])),
        canary,
    )
    .unwrap()
    .remove(0);
    assert_eq!(candidate.source, "probe-span-v1/neural/PERSON");
    assert_eq!(
        candidate.recognizer_version_id.as_deref(),
        Some("probe-span-v1")
    );
    assert_eq!(candidate.token_family, "counter");
    assert_eq!(candidate.canonical_form, None);
    assert_eq!(candidate.priority, 0);
    assert_eq!(candidate.decided_by, ConflictTier::None);
    assert!(!format!("{candidate:?}").contains(canary));
}
#[test]
fn one_callback_validates_multiple_classes_but_primary_emits_only_name() {
    let labels = ["PERSON", "ORG", "STREET_ADDRESS", "HOUSE_NUMBER", "EMAIL"];
    let batch: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mut item = raw(i.try_into().unwrap(), (i + 1).try_into().unwrap(), label);
            item.origin = ["neural", "rule", "context"][i % 3].into();
            item
        })
        .collect();
    let (backend, calls) = counted({
        let batch = batch.clone();
        move |_| batch.clone()
    });
    let probe = ProbeRecognizer::new(backend);
    let first = detect(&probe, "abcde").unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.len(), 1);
    assert!(first.iter().all(|c| &c.class == probe.supported_class()));
    assert_eq!(detect(&probe, "abcde").unwrap(), first);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let net = check(&ProbeSafetyNet(constant(batch)), "abcde").unwrap();
    assert_eq!(
        net.iter().map(|s| s.class.clone()).collect::<Vec<_>>(),
        vec![
            PiiClass::Name,
            PiiClass::Custom("organization".into()),
            PiiClass::Location,
            PiiClass::Custom("house_number".into()),
            PiiClass::Email
        ]
    );
    for (i, suspect) in net.iter().enumerate() {
        assert_eq!(
            suspect.raw_label,
            format!(
                "probe-span-v1/{}/{}",
                ["neural", "rule", "context"][i % 3],
                labels[i]
            )
        );
        assert_eq!(suspect.safety_net_id, "probe-span-v1/net");
        assert_eq!(suspect.field_path, None);
    }
}
#[test]
fn registry_document_locale_fallback_repeats_empty_and_excluded_results() {
    for excluded in [false, true] {
        let (backend, calls) = counted(move |_| {
            if excluded {
                vec![raw(0, 1, "EMAIL")]
            } else {
                vec![]
            }
        });
        let probe = ProbeRecognizer::new(backend);
        assert_eq!(probe.locale_basis(), LocaleBasis::Document);
        let registry = RecognizerRegistry::builder().register(probe).build();
        let dictionaries = DictionaryBundle::default();
        let context = DetectContext::new(
            &[LocaleTag::EnUs, LocaleTag::DeDe, LocaleTag::Global],
            &dictionaries,
        );
        assert!(registry
            .detect_all_resolved("a", &context)
            .unwrap()
            .0
            .is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    let (backend, calls) = counted(|_| vec![raw(0, 1, "PERSON")]);
    let registry = RecognizerRegistry::builder()
        .register(ProbeRecognizer::new(backend))
        .build();
    assert_eq!(
        registry
            .detect_all_resolved(
                "a",
                &DetectContext::new(
                    &[LocaleTag::EnUs, LocaleTag::Global],
                    &DictionaryBundle::default()
                )
            )
            .unwrap()
            .0
            .len(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[derive(Clone)]
struct EmailFloor;
impl Detector for EmailFloor {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .match_indices(EMAIL)
            .map(|(start, _)| {
                Detection::new(
                    start..start + EMAIL.len(),
                    PiiClass::Email,
                    "synthetic-email-floor",
                )
            })
            .collect()
    }
}
fn pipeline(primary: Option<Backend>, net: Backend) -> Pipeline {
    let builder = Pipeline::builder()
        .detector(EmailFloor)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(ProbeSafetyNet(net));
    match primary {
        Some(backend) => builder.recognizer(ProbeRecognizer::new(backend)),
        None => builder,
    }
    .build()
    .unwrap()
}
fn protect(
    p: &Pipeline,
    tx: &mut SessionTransaction<'_>,
    input: &str,
) -> Result<String, ProtectionError> {
    p.protect_text_transaction(
        tx,
        input,
        ProtectionContext::strict(&[LocaleTag::Global], &DictionaryBundle::default()),
    )
}
#[test]
fn strict_primary_plus_net_completes_but_net_only_rejects_without_publication() {
    let input = format!("{EMAIL} {PERSON}");
    let mut completed = 0;
    let mut rejected = 0;
    for with_primary in [true, false] {
        let (primary, primary_calls) = counted(|input| surface(input, PERSON, "PERSON"));
        let (net, net_calls) = counted(|clean| {
            assert!(!clean.contains(EMAIL));
            surface(clean, PERSON, "PERSON")
        });
        let p = pipeline(with_primary.then_some(primary), net);
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let result = protect(&p, &mut tx, &input);
        assert_eq!(net_calls.load(Ordering::SeqCst), 1);
        assert!(!tx.snapshot_entries().is_empty());
        assert_eq!(
            primary_calls.load(Ordering::SeqCst),
            usize::from(with_primary)
        );
        assert!(session.tokens().is_empty());
        if with_primary {
            let clean = result.unwrap();
            assert!(!clean.contains(PERSON));
            assert_eq!(tx.restore_strict_text(&clean).unwrap(), input.as_str());
            tx.commit().unwrap();
            assert!(!session.tokens().is_empty());
            completed += 1;
        } else {
            assert_eq!(result, Err(ProtectionError::Residual));
            drop(tx);
            assert!(session.tokens().is_empty());
            rejected += 1;
        }
    }
    assert_eq!((completed, rejected), (1, 1));
}
#[test]
fn normalized_primary_and_final_net_coordinates_restore_original_bytes() {
    for (owner, normalized, needle, prefix, suffix) in [
        (
            "Ａlice Example x",
            "Alice Example x",
            "Alice Example",
            "",
            " x",
        ),
        ("👨‍👩‍👧 Alice", "👨👩👧 Alice", "Alice", "👨‍👩‍👧 ", ""),
        (
            "D\u{200c}r. Sch\u{200d}midt",
            "Dr. Schmidt",
            "Dr. Schmidt",
            "",
            "",
        ),
        (
            "Jose\u{0301} Example",
            "Jose\u{0301} Example",
            "Jose\u{0301}",
            "",
            " Example",
        ),
    ] {
        let p = pipeline(
            Some(Backend::new(move |input| {
                assert_eq!(input, normalized);
                Ok(surface(input, needle, "PERSON"))
            })),
            Backend::new(move |clean| {
                assert!(clean.starts_with(prefix));
                assert!(clean.ends_with(suffix));
                assert!(!clean.contains(needle));
                Ok(vec![])
            }),
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let clean = protect(&p, &mut tx, owner).unwrap();
        assert_eq!(tx.restore_strict_text(&clean).unwrap(), owner);
        assert!(session.tokens().is_empty());
    }
}
#[test]
fn geometrically_valid_wrong_fullwidth_coordinates_swallow_suffix_counterfixture() {
    let owner = "Ａlice Example x";
    assert_eq!(owner.len(), 17);
    assert_eq!("Alice Example x".len(), 15);
    let run = |end| {
        let p = pipeline(
            Some(Backend::new(move |input| {
                assert_eq!(input, "Alice Example x");
                Ok(vec![raw(0, end, "PERSON")])
            })),
            constant(vec![]),
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let clean = protect(&p, &mut tx, owner).unwrap();
        assert_eq!(tx.restore_strict_text(&clean).unwrap(), owner);
        clean
    };
    let correct = run(13);
    let wrong = run(15);
    assert!(correct.ends_with(" x"));
    assert!(!wrong.ends_with(" x"));
    assert_ne!(correct, wrong);
    // Both ranges are geometric successes. Geometry cannot establish inference fidelity.
}
#[test]
fn scalar_boundary_inside_grapheme_is_accepted_and_restores_exactly() {
    let owner = "Jose\u{0301} Example";
    assert!(owner.is_char_boundary(4));
    assert_eq!(
        validate(owner, vec![raw(0, 4, "PERSON")]).unwrap()[0].span,
        0..4
    );
    let p = pipeline(Some(constant(vec![raw(0, 4, "PERSON")])), constant(vec![]));
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let clean = protect(&p, &mut tx, owner).unwrap();
    assert!(clean.ends_with("\u{0301} Example"));
    assert_eq!(tx.restore_strict_text(&clean).unwrap(), owner);
}
#[test]
fn repeated_surfaces_have_distinct_coordinates_and_session_value_stability() {
    let owner = "Dr. Schmidt met Dr. Schmidt.";
    let mapped = validate(owner, surface(owner, PERSON, "PERSON")).unwrap();
    assert_eq!(
        mapped.iter().map(|s| s.span.clone()).collect::<Vec<_>>(),
        vec![0..11, 16..27]
    );
    let p = pipeline(
        Some(Backend::new(|input| Ok(surface(input, PERSON, "PERSON")))),
        constant(vec![]),
    );
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    let token = tx.tokenize(&PiiClass::Name, PERSON).unwrap();
    let clean = protect(&p, &mut tx, owner).unwrap();
    assert_eq!(clean, format!("{token} met {token}."));
    assert_eq!(tx.restore_strict_text(&clean).unwrap(), owner);
}
#[test]
fn token_containment_class_disagreement_and_crossing_gap_use_live_token() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Email, EMAIL).unwrap();
    assert!(token.is_ascii());
    for crossing in [false, true] {
        let expected = token.clone();
        let p = pipeline(
            None,
            Backend::new(move |clean| {
                assert!(clean.starts_with(&expected));
                let end = if crossing {
                    clean.encode_utf16().count()
                } else {
                    expected.encode_utf16().count()
                };
                Ok(vec![raw(0, end.try_into().unwrap(), "PERSON")])
            }),
        );
        let input = format!("{token} raw gap");
        let mut tx = session.begin_transaction();
        let result = protect(&p, &mut tx, &input);
        if crossing {
            assert_eq!(result, Err(ProtectionError::Residual));
        } else {
            assert_eq!(result.unwrap(), input);
        }
    }
}
#[test]
fn stale_owner_net_coordinates_are_not_clean_coordinates_counterfixture() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Name, PERSON).unwrap();
    assert!(token.is_ascii());
    // Choose independent raw bytes longer than the actual token, so the stale
    // owner endpoint crosses a known raw suffix instead of relying on token shape.
    let raw_owner = "Synthetic Example ".repeat(token.len() + 1);
    let owned = session.tokenize(&PiiClass::Name, &raw_owner).unwrap();
    assert!(raw_owner.len() > owned.len());
    let input = format!("{owned}{}", " x".repeat(raw_owner.len()));
    for stale in [false, true] {
        let end = if stale { raw_owner.len() } else { owned.len() };
        let p = pipeline(
            None,
            Backend::new(move |clean| {
                assert!(end <= clean.len());
                Ok(vec![raw(0, end.try_into().unwrap(), "PERSON")])
            }),
        );
        let result = protect(&p, &mut session.begin_transaction(), &input);
        if stale {
            assert_eq!(result, Err(ProtectionError::Residual));
        } else {
            assert_eq!(result.unwrap(), input);
        }
    }
}
#[test]
fn malformed_owned_token_span_fails_before_manifest_filter_and_errors_drop_staging() {
    for primary_failure in [true, false] {
        let bad = Backend::new(|_| Ok(vec![raw(0, 0, "PERSON")]));
        let p = if primary_failure {
            pipeline(Some(bad), constant(vec![]))
        } else {
            pipeline(None, bad)
        };
        let session = Session::new(Scope::Ephemeral).unwrap();
        let mut tx = session.begin_transaction();
        let token = tx.tokenize(&PiiClass::Email, EMAIL).unwrap();
        let input = if primary_failure {
            format!("{token} {PERSON}")
        } else {
            token
        };
        assert_eq!(
            protect(&p, &mut tx, &input),
            Err(if primary_failure {
                ProtectionError::Primary
            } else {
                ProtectionError::SafetyNet
            })
        );
        drop(tx);
        assert!(session.tokens().is_empty());
    }
    let p = pipeline(None, Backend::new(|_| Err(ProbeError::BackendFailure)));
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut tx = session.begin_transaction();
    assert_eq!(protect(&p, &mut tx, EMAIL), Err(ProtectionError::SafetyNet));
    drop(tx);
    assert!(session.tokens().is_empty());
    assert!(matches!(
        check(
            &ProbeSafetyNet(Backend::new(|_| Err(ProbeError::BackendFailure))),
            "a"
        ),
        Err(SafetyNetError::Runtime { .. })
    ));
}
#[test]
fn owned_tokens_split_primary_into_gaps_and_net_scans_once() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session.tokenize(&PiiClass::Email, EMAIL).unwrap();
    let input = format!("left{token}right");
    let (primary, primary_calls) = counted(|gap| {
        assert!(matches!(gap, "left" | "right"));
        vec![]
    });
    let expected = input.clone();
    let (net, net_calls) = counted(move |clean| {
        assert_eq!(clean, expected);
        vec![]
    });
    let p = pipeline(Some(primary), net);
    assert_eq!(
        protect(&p, &mut session.begin_transaction(), &input).unwrap(),
        input
    );
    assert_eq!(primary_calls.load(Ordering::SeqCst), 2);
    assert_eq!(net_calls.load(Ordering::SeqCst), 1);
}
#[test]
fn literal_collision_rejects_and_foreign_token_does_not_grant_ownership() {
    let p = pipeline(None, constant(vec![]));
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut prediction = session.begin_transaction();
    let predicted = prediction.tokenize(&PiiClass::Email, EMAIL).unwrap();
    drop(prediction);
    let mut tx = session.begin_transaction();
    assert_eq!(
        protect(&p, &mut tx, &format!("{predicted} {EMAIL}")),
        Err(ProtectionError::Provenance)
    );
    drop(tx);
    assert!(session.tokens().is_empty());
    let foreign = Session::new(Scope::Ephemeral)
        .unwrap()
        .tokenize(&PiiClass::Name, PERSON)
        .unwrap();
    let p = pipeline(
        None,
        Backend::new(|input| {
            Ok(vec![raw(
                0,
                input.encode_utf16().count().try_into().unwrap(),
                "PERSON",
            )])
        }),
    );
    assert_eq!(
        protect(&p, &mut session.begin_transaction(), &foreign),
        Err(ProtectionError::Residual)
    );
    assert!(session.tokens().is_empty());
}
