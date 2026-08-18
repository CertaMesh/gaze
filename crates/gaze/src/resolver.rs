use std::ops::Range;

use crate::anchor_resolver::{AnchorOutcome, AnchorResolver};
use crate::LocaleTag;
use crate::{Candidate, ConflictTier, FamilyPolicyTable, PiiClass};

pub fn resolve_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    resolve_candidates_with_policy(candidates, &FamilyPolicyTable::EMPTY)
}

pub fn resolve_candidates_with_policy(
    mut candidates: Vec<Candidate>,
    policy: &FamilyPolicyTable,
) -> Vec<Candidate> {
    resolve_candidates_inner(&mut candidates, policy, None)
}

pub(crate) fn resolve_candidates_with_policy_and_anchors(
    mut candidates: Vec<Candidate>,
    policy: &FamilyPolicyTable,
    anchor_resolver: &AnchorResolver,
    input: &str,
    locale_chain: &[LocaleTag],
) -> Vec<Candidate> {
    resolve_candidates_inner(
        &mut candidates,
        policy,
        Some(AnchorContext {
            resolver: anchor_resolver,
            input,
            locale_chain,
        }),
    )
}

#[derive(Clone, Copy)]
struct AnchorContext<'a> {
    resolver: &'a AnchorResolver,
    input: &'a str,
    locale_chain: &'a [LocaleTag],
}

fn resolve_candidates_inner(
    candidates: &mut Vec<Candidate>,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| b.span.end.cmp(&a.span.end))
            .then_with(|| class_priority(&b.class).cmp(&class_priority(&a.class)))
            .then_with(|| b.priority.cmp(&a.priority))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.recognizer_id.cmp(&b.recognizer_id))
    });

    let mut resolved: Vec<Candidate> = Vec::new();
    for candidate in std::mem::take(candidates) {
        insert_candidate(&mut resolved, candidate, policy, anchor_ctx);
    }
    if let Some(anchor_ctx) = anchor_ctx {
        resolved = resolved
            .into_iter()
            .map(|candidate| apply_missing_anchor_fallback(candidate, policy, anchor_ctx))
            .collect();
    }
    resolved.sort_by_key(|candidate| candidate.span.start);
    resolved
}

fn insert_candidate(
    resolved: &mut Vec<Candidate>,
    candidate: Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) {
    let mut index = 0;
    while index < resolved.len() {
        let Some(overlap) = Overlap::classify(&resolved[index].span, &candidate.span) else {
            index += 1;
            continue;
        };

        match arbitrate(&resolved[index], &candidate, overlap, policy, anchor_ctx) {
            Arbitration::Merge => merge_same_span_same_class(&mut resolved[index], candidate),
            Arbitration::Family(tie) => {
                resolved[index] = tie;
                if overlap != Overlap::Exact {
                    remove_overlaps(resolved, index, ConflictTier::CollisionPolicy);
                }
            }
            Arbitration::CandidateWins(tier) => {
                let mut candidate = candidate;
                candidate.decided_by = tier;
                candidate
                    .merged_sources
                    .push(resolved[index].source.clone());
                resolved[index] = candidate;
                if overlap != Overlap::Exact {
                    remove_overlaps(resolved, index, tier);
                }
            }
            Arbitration::ExistingWins(tier) => {
                resolved[index].decided_by = tier;
                resolved[index].merged_sources.push(candidate.source);
            }
        }
        return;
    }
    resolved.push(candidate);
}

/// Geometric relation between an already-resolved span and an incoming one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlap {
    /// Identical spans: the winner keeps the slot; nothing else can overlap.
    Exact,
    /// One span fully covers the other (same-class containment gets the
    /// validator-preference rule).
    Containment,
    /// Spans overlap without either covering the other.
    Partial,
}

impl Overlap {
    fn classify(existing: &Range<usize>, candidate: &Range<usize>) -> Option<Self> {
        if !overlaps(existing, candidate) {
            return None;
        }
        if existing == candidate {
            Some(Self::Exact)
        } else if contains(existing, candidate) || contains(candidate, existing) {
            Some(Self::Containment)
        } else {
            Some(Self::Partial)
        }
    }
}

/// Outcome of arbitrating one overlapping pair.
enum Arbitration {
    /// Exact span, same class: provenance and confidence merge into `existing`.
    Merge,
    /// Precedence tie inside one collision family: emit the family-level
    /// candidate in place of both rivals.
    Family(Candidate),
    /// `candidate` replaces `existing`; the tier names what decided it.
    CandidateWins(ConflictTier),
    /// `existing` keeps the slot; the tier names the rung that separated the
    /// pair (never a label left over from an earlier overlap).
    ExistingWins(ConflictTier),
}

fn arbitrate(
    existing: &Candidate,
    candidate: &Candidate,
    overlap: Overlap,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Arbitration {
    // Family tie must be checked before the same-class merge and before any
    // ladder: two equal-precedence variants collapse into one family token even
    // when they share a class.
    if let Some(tie) = family_tie_candidate(candidate, existing, policy) {
        return Arbitration::Family(tie);
    }
    if overlap == Overlap::Exact && existing.class == candidate.class {
        return Arbitration::Merge;
    }

    // Same-class containment prefers the validator-backed span, then the base
    // ladder; policy and anchors do not apply inside one class.
    if overlap == Overlap::Containment && existing.class == candidate.class {
        let candidate_validated = candidate.canonical_form.is_some();
        let existing_validated = existing.canonical_form.is_some();
        if candidate_validated != existing_validated {
            return if candidate_validated {
                Arbitration::CandidateWins(ConflictTier::Validator)
            } else {
                Arbitration::ExistingWins(ConflictTier::Validator)
            };
        }
        return ladder_verdict(existing, candidate);
    }

    if let Some(candidate_wins) = policy.compare(&candidate.recognizer_id, &existing.recognizer_id)
    {
        return if candidate_wins {
            Arbitration::CandidateWins(ConflictTier::CollisionPolicy)
        } else {
            Arbitration::ExistingWins(ConflictTier::CollisionPolicy)
        };
    }

    // Anchor rung, consulted once per pair: an anchored *incoming* candidate
    // takes the slot from a rival outside its family and defers the
    // found/missing verdict to `apply_missing_anchor_fallback`. An anchored
    // incumbent gets no short-circuit; the ladder decides and names the tier,
    // so `AnchoredContext` is never stamped on a ladder-decided overlap.
    if let Some(anchor_ctx) = anchor_ctx {
        if requires_anchor(candidate, policy, anchor_ctx) {
            return Arbitration::CandidateWins(ConflictTier::AnchoredContext);
        }
    }

    // Structured-containment rung: a builtin-class span strictly inside a
    // custom-class structured span never evicts its container. Without it the
    // base ladder's class priority (Email/Name/Organization/Location above
    // every `Custom`) let an NER sub-token split a URL, IBAN or credential
    // around a mid-word token and leave the rest raw (todo #3025).
    if let Some(container_is_candidate) = structured_containment(existing, candidate, overlap) {
        return if container_is_candidate {
            Arbitration::CandidateWins(ConflictTier::StructuredContainment)
        } else {
            Arbitration::ExistingWins(ConflictTier::StructuredContainment)
        };
    }

    ladder_verdict(existing, candidate)
}

/// Detects the structured-containment shape and says which side is the
/// container: `Some(true)` when `candidate` encloses a builtin-class
/// `existing`, `Some(false)` when `existing` encloses a builtin-class
/// `candidate`, `None` when the pair is not a custom-class span enclosing a
/// builtin-class span. Geometry decides, never arrival order, so the verdict
/// is permutation-invariant. Same-class pairs, custom-inside-custom,
/// builtin-inside-builtin, and builtin containers over custom spans are all
/// left to the existing rungs.
fn structured_containment(
    existing: &Candidate,
    candidate: &Candidate,
    overlap: Overlap,
) -> Option<bool> {
    if overlap != Overlap::Containment {
        return None;
    }
    let candidate_encloses = contains(&candidate.span, &existing.span);
    let (container, enclosed) = if candidate_encloses {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    let structured_container = matches!(container.class, PiiClass::Custom(_));
    let builtin_enclosed = !matches!(enclosed.class, PiiClass::Custom(_));
    (structured_container && builtin_enclosed).then_some(candidate_encloses)
}

fn requires_anchor(
    candidate: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: AnchorContext<'_>,
) -> bool {
    match anchor_ctx
        .resolver
        .resolve(candidate, anchor_ctx.input, policy, anchor_ctx.locale_chain)
    {
        AnchorOutcome::Found | AnchorOutcome::Missing { .. } => true,
        AnchorOutcome::NotRequired => false,
    }
}

/// Runs the base ladder in both directions and labels the winner with the rung
/// that separated the pair. When every rung ties (same recognizer id, class
/// priority, rule priority, score, and length) the incumbent keeps the slot
/// and the row carries the terminal `RecognizerId` rung rather than a stale
/// tier from an earlier overlap.
fn ladder_verdict(existing: &Candidate, candidate: &Candidate) -> Arbitration {
    if let Some(tier) = compare_base_ladder(candidate, existing) {
        return Arbitration::CandidateWins(tier);
    }
    Arbitration::ExistingWins(
        compare_base_ladder(existing, candidate).unwrap_or(ConflictTier::RecognizerId),
    )
}

fn merge_same_span_same_class(existing: &mut Candidate, candidate: Candidate) {
    existing.score = combine_confidence(existing.score, candidate.score);
    append_unique(&mut existing.recognizer_id, &candidate.recognizer_id);
    append_unique(&mut existing.source, &candidate.source);
    if existing.canonical_form.is_none() {
        existing.canonical_form = candidate.canonical_form;
    }
    existing.decided_by = ConflictTier::Merged;
    existing.merged_sources.push(candidate.source);
}

fn combine_confidence(left: f32, right: f32) -> f32 {
    1.0 - (1.0 - left.clamp(0.0, 1.0)) * (1.0 - right.clamp(0.0, 1.0))
}

fn append_unique(existing: &mut String, next: &str) {
    if existing.split('+').any(|part| part == next) {
        return;
    }
    if !existing.is_empty() {
        existing.push('+');
    }
    existing.push_str(next);
}

/// The base conflict ladder: class-priority > rule-priority > score >
/// span-length > recognizer-id. Returns the tier at which `candidate` beats
/// `existing`, or `None` when it does not.
fn compare_base_ladder(candidate: &Candidate, existing: &Candidate) -> Option<ConflictTier> {
    if class_priority(&candidate.class) != class_priority(&existing.class) {
        return (class_priority(&candidate.class) > class_priority(&existing.class))
            .then_some(ConflictTier::ClassPriority);
    }
    if candidate.priority != existing.priority {
        return (candidate.priority > existing.priority).then_some(ConflictTier::RulePriority);
    }
    if candidate.score != existing.score {
        return candidate
            .score
            .total_cmp(&existing.score)
            .is_gt()
            .then_some(ConflictTier::Score);
    }
    let candidate_len = candidate.span.end - candidate.span.start;
    let existing_len = existing.span.end - existing.span.start;
    if candidate_len != existing_len {
        return (candidate_len > existing_len).then_some(ConflictTier::SpanLength);
    }
    (candidate.recognizer_id < existing.recognizer_id).then_some(ConflictTier::RecognizerId)
}

fn family_tie_candidate(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
) -> Option<Candidate> {
    let family = policy.precedence_tie_family(&candidate.recognizer_id, &existing.recognizer_id)?;
    let mut merged_sources = vec![
        existing.recognizer_id.clone(),
        candidate.recognizer_id.clone(),
    ];
    merged_sources.sort();
    merged_sources.dedup();
    Some(Candidate::new(
        candidate.span.start.min(existing.span.start)..candidate.span.end.max(existing.span.end),
        PiiClass::family(family),
        format!("collision-family:{family}"),
        candidate.score.max(existing.score),
        candidate.priority.max(existing.priority),
        None,
        "collision-family",
        format!("collision-family:{family}"),
        ConflictTier::CollisionPolicy,
        merged_sources,
    ))
}

fn apply_missing_anchor_fallback(
    candidate: Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: AnchorContext<'_>,
) -> Candidate {
    if candidate.decided_by == ConflictTier::CollisionPolicy {
        return candidate;
    }
    match anchor_ctx.resolver.resolve(
        &candidate,
        anchor_ctx.input,
        policy,
        anchor_ctx.locale_chain,
    ) {
        AnchorOutcome::Missing { family, .. } => {
            family_fallback_candidate(candidate, family, ConflictTier::AnchoredContext)
        }
        AnchorOutcome::Found | AnchorOutcome::NotRequired => candidate,
    }
}

fn family_fallback_candidate(
    candidate: Candidate,
    family: String,
    decided_by: ConflictTier,
) -> Candidate {
    let original_recognizer_id = candidate.recognizer_id.clone();
    Candidate::new(
        candidate.span,
        PiiClass::family(&family),
        format!("collision-family:{family}"),
        candidate.score,
        candidate.priority,
        None,
        format!("collision-family:{family}"),
        candidate.source,
        decided_by,
        vec![original_recognizer_id],
    )
}

#[cfg(test)]
thread_local! {
    /// Test-only count of `remove_overlaps` entries on the current thread.
    ///
    /// The two exact-span short-circuits in `insert_candidate` cannot be
    /// observed through the resolver's output: an exact-overlap winner keeps
    /// the very span it replaced, and the resolved set is pairwise disjoint
    /// (property 1 in `tests/prop_resolver_invariants.rs`), so overlap removal
    /// would find nothing to remove and return an identical set. Dropping both
    /// guards is therefore output-identical, and only the entry count can lock
    /// them; `non_exact_winner_enters_overlap_removal` is the positive control
    /// that keeps that count honest. Thread-local because the harness runs
    /// tests in parallel.
    static REMOVE_OVERLAPS_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn remove_overlaps(resolved: &mut Vec<Candidate>, winner_index: usize, tier: ConflictTier) {
    #[cfg(test)]
    REMOVE_OVERLAPS_CALLS.with(|calls| calls.set(calls.get() + 1));

    let winner_span = resolved[winner_index].span.clone();
    let mut index = 0;
    while index < resolved.len() {
        if index != winner_index && overlaps(&resolved[index].span, &winner_span) {
            let loser = resolved.remove(index);
            let target = if index < winner_index {
                winner_index - 1
            } else {
                winner_index
            };
            resolved[target].merged_sources.push(loser.source);
            resolved[target].decided_by = tier;
            continue;
        }
        index += 1;
    }
}

fn class_priority(class: &PiiClass) -> u8 {
    match class {
        PiiClass::Email => 90,
        PiiClass::Name => 80,
        PiiClass::Organization => 70,
        PiiClass::Location => 60,
        PiiClass::Custom(_) => 50,
    }
}

fn contains(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start <= right.start && left.end >= right.end
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(span: Range<usize>, class: PiiClass, score: f32, id: &str) -> Candidate {
        Candidate::new(
            span,
            class,
            id,
            score,
            0,
            None,
            "counter",
            id,
            ConflictTier::None,
            Vec::new(),
        )
    }

    /// Two variants of one collision family at equal precedence: the shape
    /// `family_tie_candidate` recognises as a precedence tie.
    fn tenant_document_registry() -> crate::RecognizerRegistry {
        crate::RecognizerRegistry::builder()
            .register_collision(
                "doc.alpha",
                crate::CollisionMembership::new("tenant-document", "alpha", 10, None),
            )
            .register_collision(
                "doc.beta",
                crate::CollisionMembership::new("tenant-document", "beta", 10, None),
            )
            .build()
    }

    /// Resolves and reports how many times the resolver entered
    /// `remove_overlaps`, so the exact-span short-circuits can be asserted
    /// directly rather than through output that does not change.
    fn counting_removals<F>(resolve: F) -> (Vec<Candidate>, usize)
    where
        F: FnOnce() -> Vec<Candidate>,
    {
        REMOVE_OVERLAPS_CALLS.with(|calls| calls.set(0));
        let resolved = resolve();
        (resolved, REMOVE_OVERLAPS_CALLS.with(std::cell::Cell::get))
    }

    #[test]
    fn exact_span_same_class_merges_provenance_and_confidence() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Email, 0.70, "regex"),
            candidate(0..5, PiiClass::Email, 0.50, "dict"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "regex+dict");
        assert!((resolved[0].score - 0.85).abs() < 0.0001);
    }

    #[test]
    fn exact_span_different_class_uses_class_priority_then_score() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Name, 0.99, "ner"),
            candidate(0..5, PiiClass::Email, 0.70, "regex"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    #[test]
    fn collision_policy_precedes_class_priority() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban",
                crate::CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
            )
            .build();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(0..5, PiiClass::Email, 0.70, "pan"),
                candidate(0..5, PiiClass::custom("iban"), 0.70, "iban"),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban");
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
    }

    #[test]
    fn family_policy_arbitrates_before_mandatory_anchor_resolution() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan.structural",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(0..5, PiiClass::Email, 0.70, "pan.structural"),
                candidate(0..5, PiiClass::custom("iban"), 0.70, "iban.structural"),
            ],
            registry.family_policy(),
            &AnchorResolver::default(),
            "DE893",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].class, PiiClass::Custom("iban".to_string()));
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
    }

    #[test]
    fn precedence_tie_emits_family_level_candidate() {
        let registry = tenant_document_registry();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(0..5, PiiClass::custom("alpha"), 0.70, "doc.alpha"),
                candidate(0..5, PiiClass::custom("beta"), 0.70, "doc.beta"),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].class,
            PiiClass::Custom("family:tenant-document".to_string())
        );
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            resolved[0].merged_sources,
            vec!["doc.alpha".to_string(), "doc.beta".to_string()]
        );
    }

    /// `arbitrate` probes the family tie *before* the exact same-class merge.
    /// Both rivals here report the same class, so a merge that ran first would
    /// swallow the tie and emit an ordinary merged candidate instead of the
    /// family token. The other precedence-tie fixtures pair different classes
    /// and cannot fail when that ordering regresses.
    #[test]
    fn exact_span_same_class_precedence_tie_emits_family_candidate_not_merge() {
        let registry = tenant_document_registry();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(0..5, PiiClass::custom("tenant-doc"), 0.70, "doc.alpha"),
                candidate(0..5, PiiClass::custom("tenant-doc"), 0.70, "doc.beta"),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].class,
            PiiClass::Custom("family:tenant-document".to_string()),
            "an exact same-class overlap must not merge ahead of the family tie"
        );
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            resolved[0].merged_sources,
            vec!["doc.alpha".to_string(), "doc.beta".to_string()]
        );
    }

    /// Positive control for `counting_removals`. A non-exact winner widens the
    /// slot it took, so it must enter overlap removal; without this, the two
    /// zero-call assertions below would also pass on a counter that never
    /// increments.
    #[test]
    fn non_exact_winner_enters_overlap_removal() {
        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates(vec![
                candidate(0..6, PiiClass::Name, 0.70, "ner"),
                candidate(3..12, PiiClass::Email, 0.80, "regex"),
            ])
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 3..12);
        assert_eq!(removal_calls, 1);
    }

    /// An exact-span family tie installs a span identical to the one it
    /// evicted, so no other resolved candidate can overlap it and overlap
    /// removal is dead work. Skipping it is invisible in the output, so assert
    /// on the entry count itself (see `REMOVE_OVERLAPS_CALLS`).
    #[test]
    fn exact_span_family_tie_skips_overlap_removal() {
        let registry = tenant_document_registry();

        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates_with_policy(
                vec![
                    candidate(0..5, PiiClass::custom("tenant-doc"), 0.70, "doc.alpha"),
                    candidate(0..5, PiiClass::custom("tenant-doc"), 0.70, "doc.beta"),
                ],
                registry.family_policy(),
            )
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            removal_calls, 0,
            "an exact-span family tie must not enter overlap removal"
        );
    }

    /// The same guard on the other exact-span path: a collision-policy win
    /// replaces the slot span-for-span, so it must not enter overlap removal
    /// either.
    #[test]
    fn exact_span_candidate_win_skips_overlap_removal() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban",
                crate::CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
            )
            .build();

        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates_with_policy(
                vec![
                    candidate(0..5, PiiClass::Email, 0.70, "pan"),
                    candidate(0..5, PiiClass::custom("iban"), 0.70, "iban"),
                ],
                registry.family_policy(),
            )
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban");
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            removal_calls, 0,
            "an exact-span collision-policy win must not enter overlap removal"
        );
    }

    #[test]
    fn rule_priority_beats_score_when_class_ties() {
        let mut low_priority = candidate(0..5, PiiClass::Email, 0.99, "low");
        low_priority.priority = 1;
        let mut high_priority = candidate(0..5, PiiClass::Email, 0.70, "high");
        high_priority.priority = 2;

        let resolved = resolve_candidates(vec![low_priority, high_priority]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "high+low");
    }

    #[test]
    fn same_class_containment_prefers_validator_backed_candidate() {
        let mut validated = candidate(0..10, PiiClass::Email, 0.50, "validator");
        validated.canonical_form = Some("canonical".to_string());
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Email, 0.95, "regex"),
            validated,
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "validator");
    }

    #[test]
    fn partial_overlap_prefers_higher_confidence() {
        let resolved = resolve_candidates(vec![
            candidate(0..6, PiiClass::Name, 0.70, "ner"),
            candidate(3..12, PiiClass::Email, 0.80, "regex"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    #[test]
    fn multi_overlap_replacement_leaves_disjoint_set() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Location, 0.70, "a"),
            candidate(3..8, PiiClass::Name, 0.70, "b"),
            candidate(0..10, PiiClass::Email, 0.70, "c"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..10);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    /// The anchor short-circuit must not label an overlap the base ladder
    /// decided. Here the anchored incumbent wins on Score; the audit row has to
    /// say Score, not AnchoredContext.
    #[test]
    fn anchored_incumbent_that_wins_on_score_is_labelled_score() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(LocaleTag::DeDe, "iban", vec!["IBAN".to_string()], None);

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(6..10, PiiClass::custom("iban"), 0.90, "iban.structural"),
                candidate(6..10, PiiClass::custom("digits"), 0.50, "digits.generic"),
            ],
            registry.family_policy(),
            &anchors,
            "IBAN: DE89",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].class, PiiClass::custom("iban"));
        assert_eq!(
            resolved[0].merged_sources,
            vec!["digits.generic".to_string()]
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::Score);
    }

    /// Same mislabel through the partial-overlap path.
    #[test]
    fn anchored_incumbent_that_wins_partial_overlap_on_score_is_labelled_score() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(LocaleTag::DeDe, "iban", vec!["IBAN".to_string()], None);

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(6..10, PiiClass::custom("iban"), 0.90, "iban.structural"),
                candidate(8..12, PiiClass::custom("digits"), 0.50, "digits.generic"),
            ],
            registry.family_policy(),
            &anchors,
            "IBAN: DE8912",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].span, 6..10);
        assert_eq!(resolved[0].decided_by, ConflictTier::Score);
    }

    /// When the ladder is exhausted (identical recognizer id, class, priority,
    /// score, and length) the incumbent keeps the slot; the row must carry the
    /// terminal rung, not whatever tier an earlier overlap stamped.
    #[test]
    fn fully_tied_partial_overlap_never_keeps_stale_tier() {
        let resolved = resolve_candidates(vec![
            candidate(0..4, PiiClass::Name, 0.70, "dict"),
            candidate(2..6, PiiClass::Name, 0.70, "dict"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..4);
        assert_eq!(resolved[0].merged_sources, vec!["dict".to_string()]);
        assert_eq!(resolved[0].decided_by, ConflictTier::RecognizerId);
    }

    fn prioritized(mut candidate: Candidate, priority: i32) -> Candidate {
        candidate.priority = priority;
        candidate
    }

    /// An NER organisation token strictly inside a rule-recognised URL: the
    /// structured container keeps the slot whichever candidate arrives first,
    /// the enclosed span is recorded as a merged source, and the rung that
    /// decided it is named truthfully. Before this rung existed the enclosed
    /// builtin span won on `ClassPriority` and `remove_overlaps` dropped the
    /// whole URL, leaving its head and tail raw around a mid-word token.
    #[test]
    fn builtin_sub_span_does_not_evict_custom_container() {
        for container_first in [true, false] {
            let container = prioritized(
                candidate(0..24, PiiClass::custom("url"), 0.80, "url.anchored"),
                90,
            );
            let enclosed = candidate(12..16, PiiClass::Organization, 0.99, "ner");
            let input = if container_first {
                vec![container, enclosed]
            } else {
                vec![enclosed, container]
            };

            let resolved = resolve_candidates(input);

            assert_eq!(resolved.len(), 1, "container_first={container_first}");
            assert_eq!(resolved[0].span, 0..24);
            assert_eq!(resolved[0].class, PiiClass::custom("url"));
            assert_eq!(resolved[0].recognizer_id, "url.anchored");
            assert_eq!(resolved[0].decided_by, ConflictTier::StructuredContainment);
            assert_eq!(resolved[0].merged_sources, vec!["ner".to_string()]);
        }
    }

    /// Two builtin sub-spans of different classes inside one structured span
    /// converge to the container alone (multi-overlap fixed point).
    #[test]
    fn several_builtin_sub_spans_collapse_into_the_custom_container() {
        let resolved = resolve_candidates(vec![
            candidate(12..16, PiiClass::Organization, 0.99, "ner"),
            prioritized(
                candidate(0..30, PiiClass::custom("url"), 0.80, "url.anchored"),
                90,
            ),
            candidate(20..26, PiiClass::Name, 0.97, "ner"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..30);
        assert_eq!(resolved[0].class, PiiClass::custom("url"));
        assert_eq!(resolved[0].decided_by, ConflictTier::StructuredContainment);
    }

    /// Scope pin: the rung is containment-only. A builtin span that merely
    /// straddles the structured span's edge is still decided by the base
    /// ladder (class priority), exactly as before.
    #[test]
    fn partial_overlap_with_a_custom_span_still_uses_class_priority() {
        let resolved = resolve_candidates(vec![
            prioritized(
                candidate(0..24, PiiClass::custom("url"), 0.80, "url.anchored"),
                90,
            ),
            candidate(20..30, PiiClass::Organization, 0.99, "ner"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Organization);
        assert_eq!(resolved[0].decided_by, ConflictTier::ClassPriority);
    }

    /// Scope pin: containment between two custom-class spans is untouched and
    /// keeps going through the base ladder (here rule priority).
    #[test]
    fn custom_inside_custom_containment_still_uses_the_base_ladder() {
        let resolved = resolve_candidates(vec![
            prioritized(
                candidate(
                    0..13,
                    PiiClass::custom("tax_number"),
                    0.85,
                    "tax_number.cue_anchored",
                ),
                84,
            ),
            prioritized(
                candidate(8..13, PiiClass::custom("postal_code"), 0.80, "postal.de"),
                70,
            ),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "tax_number.cue_anchored");
        assert_eq!(resolved[0].decided_by, ConflictTier::RulePriority);
    }

    /// Scope pin: a builtin container over a custom sub-span already won on
    /// class priority; the new rung must not relabel that decision.
    #[test]
    fn builtin_container_over_custom_sub_span_is_still_class_priority() {
        let resolved = resolve_candidates(vec![
            candidate(0..20, PiiClass::Location, 0.95, "ner"),
            prioritized(
                candidate(10..15, PiiClass::custom("postal_code"), 0.80, "postal.de"),
                70,
            ),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Location);
        assert_eq!(resolved[0].decided_by, ConflictTier::ClassPriority);
    }
}
