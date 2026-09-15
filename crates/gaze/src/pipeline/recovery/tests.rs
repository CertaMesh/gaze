use super::*;
use crate::{ConflictTier, PiiClass};

#[allow(dead_code)]
#[path = "legacy.rs"]
mod legacy;

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
fn custom() -> PiiClass {
    PiiClass::custom("password").unwrap()
}
fn run(candidates: Vec<Candidate>, registry: &RecognizerRegistry, raw: &str) -> WholePlan {
    plan(
        CandidatePool::new(candidates),
        registry,
        &crate::normalize::normalize(raw),
        raw,
        &[LocaleTag::Global],
    )
    .unwrap()
}
fn triple() -> Vec<Candidate> {
    vec![
        candidate(11..15, PiiClass::Name, "a"),
        candidate(11..21, custom(), "f"),
        candidate(16..29, PiiClass::Name, "b"),
    ]
}
fn spans(nodes: &[Candidate]) -> Vec<Range<usize>> {
    nodes.iter().map(|n| n.span.clone()).collect()
}

#[test]
fn six_permutations_recover_original_payload_and_keep_primary() {
    let registry = RecognizerRegistry::builder().build();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut input = triple();
        input[0].canonical_form = Some("original-left".into());
        input[0].token_family = "original-family".into();
        let input = order.map(|i| input[i].clone()).to_vec();
        let old = legacy::resolve_candidates(input.clone());
        let result = run(input, &registry, "password: \"left right\"\nmarker");
        assert_eq!(result.primary, old);
        assert_eq!(spans(&result.primary), vec![16..29]);
        assert_eq!(spans(&result.recovered), vec![11..15]);
        assert_eq!(
            result.recovered[0].canonical_form.as_deref(),
            Some("original-left")
        );
        assert_eq!(result.recovered[0].token_family, "original-family");
        assert_eq!(result.recovered[0].decided_by, ConflictTier::None);
        assert!(result.events.iter().any(|e| matches!(e, ResolutionEvent::Recovery { normalized, raw, .. } if *normalized== (11..15) && *raw==(11..15))));
    }
}

#[test]
fn reverse_geometry_and_two_productive_recovery_rounds() {
    let registry = RecognizerRegistry::builder().build();
    let reverse = vec![
        candidate(10..20, custom(), "f"),
        candidate(0..12, PiiClass::Name, "a"),
        candidate(15..18, PiiClass::Name, "b"),
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let result = run(
            order.map(|i| reverse[i].clone()).to_vec(),
            &registry,
            &"x".repeat(40),
        );
        assert_eq!(spans(&result.primary), vec![0..12, 15..18]);
    }
    let mut outer = candidate(0..30, custom(), "outer");
    outer.priority = 100;
    let result = run(
        vec![
            outer,
            candidate(0..10, custom(), "f"),
            candidate(0..4, PiiClass::Name, "a"),
            candidate(5..14, PiiClass::Name, "b"),
            candidate(25..40, PiiClass::Name, "last"),
        ],
        &registry,
        &"x".repeat(40),
    );
    assert_eq!(spans(&result.primary), vec![25..40]);
    assert_eq!(spans(&result.recovered), vec![0..4, 5..14]);
    let events = result
        .events
        .iter()
        .filter_map(|e| {
            if let ResolutionEvent::Recovery { raw, .. } = e {
                Some(raw.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(events, vec![5..14, 0..4]);
}

#[test]
fn pending_pool_uses_legacy_merge_validator_family_and_anchor_rules() {
    for mode in 0..6 {
        let mut builder = RecognizerRegistry::builder();
        if mode >= 3 {
            builder = builder
                .register_collision(
                    "a",
                    crate::CollisionMembership::new("document", "a", 10, Some("cue".into())),
                )
                .register_collision(
                    "b",
                    crate::CollisionMembership::new(
                        "document",
                        "b",
                        if mode == 3 { 10 } else { 20 },
                        None,
                    ),
                );
            if mode == 5 {
                builder = builder.register_anchor_cue_bundle(
                    LocaleTag::Global,
                    "cue",
                    vec!["cue".into()],
                    None,
                );
            }
        }
        let registry = builder.build();
        let (mut a, b) = match mode {
            0 => (
                candidate(0..5, PiiClass::Email, "a"),
                candidate(3..8, PiiClass::Name, "b"),
            ),
            1 => (
                candidate(0..5, PiiClass::Name, "a"),
                candidate(0..8, PiiClass::Name, "b"),
            ),
            2 => (
                candidate(0..5, PiiClass::Name, "a"),
                candidate(0..5, PiiClass::Name, "b"),
            ),
            _ => (
                candidate(0..5, custom(), "a"),
                candidate(3..8, custom(), "b"),
            ),
        };
        if mode == 1 {
            a.canonical_form = Some("valid".into());
        }
        let pending = vec![a, b];
        let raw = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx cue";
        let mut anchors = crate::anchor_resolver::AnchorResolver::default();
        if mode == 5 {
            anchors.register(LocaleTag::Global, "cue", vec!["cue".into()], None);
        }
        let expected = legacy::resolve_candidates_with_policy_and_anchors(
            pending.clone(),
            registry.family_policy(),
            &anchors,
            raw,
            &[LocaleTag::Global],
        );
        let mut outer = candidate(0..30, custom(), "outer");
        outer.priority = 100;
        let mut all = vec![outer];
        all.extend(pending);
        all.push(candidate(25..40, PiiClass::Email, "last"));
        let result = run(all, &registry, raw);
        assert_eq!(result.recovered, expected);
    }
}

#[test]
fn stable_ties_preserve_canonical_confidence_multiplicity_and_primary_order() {
    let registry = RecognizerRegistry::builder().build();
    let mut a = candidate(0..5, PiiClass::Name, "same");
    a.canonical_form = Some("first".into());
    let mut b = a.clone();
    b.canonical_form = Some("second".into());
    b.source = "different".into();
    b.token_family = "different".into();
    b.recognizer_version_id = Some("v2".into());
    for input in [
        vec![a.clone(), b.clone(), a.clone()],
        vec![b.clone(), a.clone(), b.clone()],
    ] {
        let expected = legacy::resolve_candidates(input.clone());
        let result = run(input.clone(), &registry, "xxxxx");
        assert_eq!(result.primary, expected);
        assert_eq!(result.primary[0].canonical_form, input[0].canonical_form);
        assert!(result.recovered.is_empty());
    }
}

#[test]
fn structural_members_do_not_include_source_label_losers() {
    let mut pool = CandidatePool::new(triple());
    let order = pool.order.clone();
    let selected = pool.resolve(&order, &crate::FamilyPolicyTable::EMPTY, None);
    assert_eq!(selected[0].members, vec![2]);
    assert_eq!(pool.originals()[0].decided_by, ConflictTier::None);
    let mut pool = CandidatePool::new(vec![
        candidate(0..5, PiiClass::Name, "a"),
        candidate(0..5, PiiClass::Name, "b"),
    ]);
    let order = pool.order.clone();
    let selected = pool.resolve(&order, &crate::FamilyPolicyTable::EMPTY, None);
    assert_eq!(selected[0].members, vec![0, 1]);
    let registry = RecognizerRegistry::builder()
        .register_collision(
            "a",
            crate::CollisionMembership::new("document", "a", 10, None),
        )
        .register_collision(
            "b",
            crate::CollisionMembership::new("document", "b", 10, None),
        )
        .build();
    let mut pool = CandidatePool::new(vec![
        candidate(0..5, custom(), "a"),
        candidate(3..8, custom(), "b"),
    ]);
    let order = pool.order.clone();
    let selected = registry.resolve_pool(&mut pool, &order, "xxxxxxxx", &[LocaleTag::Global]);
    assert_eq!(selected[0].members, vec![0, 1]);
    assert_eq!(selected[0].candidate.span, 0..8);
}

#[test]
fn raw_scalar_expansion_fails_closed_and_joiners_use_original_geometry() {
    let registry = RecognizerRegistry::builder().build();
    let raw = "\u{0344}";
    let normalized = crate::normalize::normalize(raw);
    assert_eq!(normalized.text, "\u{0308}\u{0301}");
    let input = vec![
        candidate(0..2, PiiClass::Name, "a"),
        candidate(2..4, PiiClass::Name, "b"),
    ];
    assert_eq!(legacy::resolve_candidates(input.clone()).len(), 2);
    assert!(matches!(
        plan(
            CandidatePool::new(input),
            &registry,
            &normalized,
            raw,
            &[LocaleTag::Global]
        ),
        Err(crate::Error::SafetyNet(
            crate::SafetyNetError::InvalidOutput { .. }
        ))
    ));
    let raw = "ｐassword: \"le\u{200d}ft right\"\nmarker";
    let result = run(triple(), &registry, raw);
    assert_eq!(&raw[result.recovered[0].span.clone()], "le\u{200d}ft");
    assert_eq!(&raw[result.primary[0].span.clone()], "right\"\nmarker");
}

#[test]
fn indexed_primary_matches_frozen_legacy_and_recovery_terminates() {
    let registry = RecognizerRegistry::builder().build();
    let mut seed = 17u64;
    for _ in 0..2000 {
        let mut input = Vec::new();
        for index in 0..8 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = (seed as usize) % 24;
            let end = start + 1 + ((seed >> 32) as usize) % 12;
            let class = match (seed >> 48) % 3 {
                0 => PiiClass::Name,
                1 => PiiClass::Email,
                _ => custom(),
            };
            input.push(candidate(start..end, class, &format!("r{index}")));
        }
        let result = run(input.clone(), &registry, &"x".repeat(40));
        let reversed = run(
            input.iter().cloned().rev().collect(),
            &registry,
            &"x".repeat(40),
        );
        assert_eq!(result.primary, reversed.primary);
        assert_eq!(result.recovered, reversed.recovered);
        assert_eq!(result.primary, legacy::resolve_candidates(input.clone()));
        let mut all = result.primary;
        all.extend(result.recovered);
        all.sort_by_key(|n| n.span.start);
        assert!(all.len() <= input.len());
        assert!(all.windows(2).all(|n| n[0].span.end <= n[1].span.start));
        for original in input {
            assert!(all
                .iter()
                .any(|n| n.span.start < original.span.end && original.span.start < n.span.end));
        }
    }
}

#[test]
fn invalid_boundaries_are_rejected_before_anchor_lookup() {
    let registry = RecognizerRegistry::builder()
        .register_collision(
            "a",
            crate::CollisionMembership::new("document", "a", 10, Some("cue".into())),
        )
        .register_anchor_cue_bundle(LocaleTag::Global, "cue", vec!["x".into()], None)
        .build();
    let raw = "é";
    let result = plan(
        CandidatePool::new(vec![candidate(1..2, custom(), "a")]),
        &registry,
        &crate::normalize::normalize(raw),
        raw,
        &[LocaleTag::Global],
    );
    assert!(result.is_err());
}

#[test]
fn recovered_anchor_fallback_uses_original_context_and_collision_bypasses_it() {
    for found in [false, true] {
        let mut builder = RecognizerRegistry::builder().register_collision(
            "a",
            crate::CollisionMembership::new("document", "a", 10, Some("cue".into())),
        );
        if found {
            builder = builder.register_anchor_cue_bundle(
                LocaleTag::Global,
                "cue",
                vec!["cue".into()],
                None,
            );
        }
        let registry = builder.build();
        let mut outer = candidate(0..30, custom(), "outer");
        outer.priority = 100;
        let result = run(
            vec![
                outer,
                candidate(0..4, custom(), "a"),
                candidate(25..40, PiiClass::Email, "last"),
            ],
            &registry,
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx cue",
        );
        assert_eq!(result.recovered.len(), 1);
        assert_eq!(
            result.recovered[0].class,
            if found {
                custom()
            } else {
                PiiClass::Custom("family:document".into())
            }
        );
        assert_eq!(result.recovered[0].span, 0..4);
    }
    let registry = RecognizerRegistry::builder()
        .register_collision(
            "a",
            crate::CollisionMembership::new("document", "a", 10, Some("cue".into())),
        )
        .register_collision(
            "b",
            crate::CollisionMembership::new("document", "b", 20, None),
        )
        .build();
    let result = run(
        vec![
            candidate(0..5, custom(), "a"),
            candidate(3..8, custom(), "b"),
        ],
        &registry,
        "xxxxxxxx",
    );
    assert_eq!(result.primary[0].class, custom());
    assert_eq!(result.primary[0].decided_by, ConflictTier::CollisionPolicy);
}

#[test]
fn vanished_family_hull_does_not_consume_its_original_members() {
    let registry = RecognizerRegistry::builder()
        .register_collision(
            "a",
            crate::CollisionMembership::new("document", "a", 10, None),
        )
        .register_collision(
            "b",
            crate::CollisionMembership::new("document", "b", 10, None),
        )
        .build();
    let mut a = candidate(0..5, custom(), "a");
    a.canonical_form = Some("whole-a".into());
    let result = run(
        vec![
            a.clone(),
            candidate(3..8, custom(), "b"),
            candidate(7..12, PiiClass::Email, "last"),
        ],
        &registry,
        "xxxxxxxxxxxx",
    );
    assert_eq!(spans(&result.primary), vec![7..12]);
    assert_eq!(result.recovered, vec![a]);
}

#[test]
fn work_counts_show_disjoint_gaps_are_not_replayed() {
    let registry = RecognizerRegistry::builder().build();
    for count in [10, 100, 500] {
        let disjoint = (0..count)
            .map(|i| candidate(40 + 2 * i..41 + 2 * i, PiiClass::Name, &format!("d{i}")))
            .collect::<Vec<_>>();
        let baseline = run(disjoint.clone(), &registry, &"x".repeat(42 + 2 * count));
        assert_eq!(baseline.work.pools, 1);
        assert_eq!(baseline.work.candidates, count);
        assert_eq!(baseline.work.overlap_probes, count * (count - 1) / 2);
        let mut input = triple();
        input.extend(disjoint);
        let result = run(input, &registry, &"x".repeat(42 + 2 * count));
        assert_eq!(result.work.pools, 2);
        assert_eq!(result.work.candidates, count + 4);
        assert_eq!(spans(&result.recovered), vec![11..15]);
    }
}

#[test]
fn equal_legacy_keys_across_custom_classes_keep_input_order() {
    let registry = RecognizerRegistry::builder().build();
    let a = candidate(0..4, PiiClass::custom("alpha").unwrap(), "same");
    let b = candidate(0..4, PiiClass::custom("beta").unwrap(), "same");
    for input in [vec![a.clone(), b.clone()], vec![b, a]] {
        let expected = legacy::resolve_candidates(input.clone());
        let first = input[0].class.clone();
        let result = run(input, &registry, "xxxx");
        assert_eq!(result.primary, expected);
        assert_eq!(result.primary[0].class, first);
    }
}

#[test]
fn recovered_pool_raw_collision_is_rejected_even_with_valid_primary() {
    let registry = RecognizerRegistry::builder().build();
    let raw = format!("\u{0344}{}", "x".repeat(38));
    let input = vec![
        candidate(0..30, custom(), "outer"),
        candidate(0..2, PiiClass::Name, "a"),
        candidate(2..4, PiiClass::Name, "b"),
        candidate(25..40, PiiClass::Name, "last"),
    ];
    assert_eq!(
        spans(&legacy::resolve_candidates(input.clone())),
        vec![25..40]
    );
    assert!(matches!(
        plan(
            CandidatePool::new(input),
            &registry,
            &crate::normalize::normalize(&raw),
            &raw,
            &[LocaleTag::Global]
        ),
        Err(crate::Error::SafetyNet(
            crate::SafetyNetError::InvalidOutput { .. }
        ))
    ));
}
