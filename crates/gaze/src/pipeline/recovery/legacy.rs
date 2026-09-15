// Exact pre-repair resolver, used only for differential tests.
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
    let mut merged_sources = existing.merged_sources.clone();
    merged_sources.extend(candidate.merged_sources.iter().cloned());
    merged_sources.push(existing.recognizer_id.clone());
    merged_sources.push(candidate.recognizer_id.clone());
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
    let mut merged_sources = candidate.merged_sources;
    if !merged_sources
        .iter()
        .any(|source| source == &original_recognizer_id)
    {
        merged_sources.push(original_recognizer_id);
    }
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
        merged_sources,
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
