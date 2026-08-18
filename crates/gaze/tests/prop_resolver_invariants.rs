//! Resolver-boundary invariants for `resolve_candidates` /
//! `resolve_candidates_with_policy`.
//!
//! These are properties of the public conflict-resolution boundary, not of any
//! particular internal shape, so they must hold before and after any refactor
//! of `crates/gaze/src/resolver.rs`:
//!
//! 1. The resolved set is pairwise disjoint (the pipeline tokenizes each
//!    resolved span once; an overlap would double-tokenize or drop bytes).
//! 2. The result does not depend on the order candidates arrive in. Every
//!    generated candidate has a distinct sort key unless it is identical, so
//!    the resolver's internal sort is total and the outcome is a pure function
//!    of the multiset of candidates.
//! 3. Every resolved span boundary comes from an input span boundary (family
//!    tokens take the union of their rivals; nothing invents new offsets).
//!
//! Defaults to 128 cases for local speed. Set `PROPTEST_CASES=<n>` to broaden
//! the run in CI or during investigation.

use gaze::{
    resolve_candidates, resolve_candidates_with_policy, Candidate, CollisionMembership,
    ConflictTier, PiiClass, RecognizerRegistry,
};
use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

/// Number of synthetic recognizers. Each index maps to one recognizer id, one
/// class, and one validator flavour, so equal sort keys imply equal candidates.
const RECOGNIZER_COUNT: usize = 6;

#[derive(Debug, Clone, PartialEq)]
struct Spec {
    start: usize,
    len: usize,
    recognizer: usize,
    score: f32,
    priority: i32,
}

fn prop_config() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(128),
        ..Default::default()
    }
}

fn recognizer_id(index: usize) -> String {
    format!("rec{index}")
}

fn class_for(index: usize) -> PiiClass {
    match index {
        0 => PiiClass::Email,
        1 => PiiClass::Name,
        2 => PiiClass::Organization,
        3 => PiiClass::Location,
        4 => PiiClass::custom("iban"),
        _ => PiiClass::custom("pan"),
    }
}

/// Even-indexed recognizers are validator-backed so same-class containment
/// exercises the validator-preference branch in both directions.
fn canonical_form_for(index: usize) -> Option<String> {
    index.is_multiple_of(2).then(|| format!("canon{index}"))
}

fn candidate_from(spec: &Spec) -> Candidate {
    Candidate::new(
        spec.start..spec.start + spec.len,
        class_for(spec.recognizer),
        recognizer_id(spec.recognizer),
        spec.score,
        spec.priority,
        canonical_form_for(spec.recognizer),
        "prop",
        recognizer_id(spec.recognizer),
        ConflictTier::None,
        Vec::new(),
    )
}

fn spec_strategy() -> impl Strategy<Value = Spec> {
    (
        0usize..24,
        1usize..=8,
        0usize..RECOGNIZER_COUNT,
        prop::sample::select(vec![0.30f32, 0.50, 0.70, 0.90]),
        0i32..3,
    )
        .prop_map(|(start, len, recognizer, score, priority)| Spec {
            start,
            len,
            recognizer,
            score,
            priority,
        })
}

/// A candidate multiset plus one permutation of it.
fn specs_and_permutation() -> impl Strategy<Value = (Vec<Spec>, Vec<Spec>)> {
    prop::collection::vec(spec_strategy(), 0..=8)
        .prop_flat_map(|specs| (Just(specs.clone()), Just(specs).prop_shuffle()))
}

/// Family policy that exercises every collision path the resolver has:
/// `rec4`/`rec5` share a family with distinct precedence (policy decides),
/// `rec2`/`rec3` share a family with equal precedence (precedence tie emits a
/// family-level token).
fn family_registry() -> RecognizerRegistry {
    RecognizerRegistry::builder()
        .register_collision(
            recognizer_id(4),
            CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
        )
        .register_collision(
            recognizer_id(5),
            CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
        )
        .register_collision(
            recognizer_id(2),
            CollisionMembership::new("tenant-document", "alpha", 10, None),
        )
        .register_collision(
            recognizer_id(3),
            CollisionMembership::new("tenant-document", "beta", 10, None),
        )
        .build()
}

fn assert_disjoint_and_sorted(resolved: &[Candidate]) {
    for window in resolved.windows(2) {
        let (left, right) = (&window[0], &window[1]);
        assert!(
            left.span.start <= right.span.start,
            "resolved output not sorted by start: {left:?} then {right:?}"
        );
        assert!(
            left.span.end <= right.span.start,
            "resolved spans overlap: {left:?} and {right:?}"
        );
    }
}

fn assert_boundaries_from_inputs(specs: &[Spec], resolved: &[Candidate]) {
    for candidate in resolved {
        assert!(
            specs.iter().any(|spec| spec.start == candidate.span.start),
            "resolved start {} not an input start: {candidate:?}",
            candidate.span.start
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.start + spec.len == candidate.span.end),
            "resolved end {} not an input end: {candidate:?}",
            candidate.span.end
        );
    }
}

fn check_invariants(
    specs: &[Spec],
    permuted: &[Spec],
    resolve: impl Fn(Vec<Candidate>) -> Vec<Candidate>,
) {
    let resolved = resolve(specs.iter().map(candidate_from).collect());
    let resolved_permuted = resolve(permuted.iter().map(candidate_from).collect());

    assert_disjoint_and_sorted(&resolved);
    assert_boundaries_from_inputs(specs, &resolved);
    assert!(
        resolved.len() <= specs.len(),
        "resolver produced more candidates than it received"
    );
    assert_eq!(
        resolved, resolved_permuted,
        "resolver output depends on candidate arrival order"
    );
}

proptest! {
    #![proptest_config(prop_config())]

    #[test]
    fn resolved_set_is_disjoint_and_permutation_invariant(
        (specs, permuted) in specs_and_permutation()
    ) {
        check_invariants(&specs, &permuted, resolve_candidates);
    }

    #[test]
    fn resolved_set_with_family_policy_is_disjoint_and_permutation_invariant(
        (specs, permuted) in specs_and_permutation()
    ) {
        let registry = family_registry();
        check_invariants(&specs, &permuted, |candidates| {
            resolve_candidates_with_policy(candidates, registry.family_policy())
        });
    }
}
