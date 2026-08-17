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
        if !overlaps(&resolved[index].span, &candidate.span) {
            index += 1;
            continue;
        }

        if resolved[index].span == candidate.span {
            if let Some(tie) = family_tie_candidate(&candidate, &resolved[index], policy) {
                resolved[index] = tie;
                return;
            }
            if resolved[index].class == candidate.class {
                merge_same_span_same_class(&mut resolved[index], candidate);
                return;
            }
            if let Some(tier) =
                should_replace_same_span_class(&candidate, &resolved[index], policy, anchor_ctx)
            {
                let mut candidate = candidate;
                candidate.decided_by = tier;
                candidate
                    .merged_sources
                    .push(resolved[index].source.clone());
                resolved[index] = candidate;
            } else {
                if let Some(tier) =
                    should_replace_same_span_class(&resolved[index], &candidate, policy, anchor_ctx)
                {
                    resolved[index].decided_by = tier;
                }
                resolved[index].merged_sources.push(candidate.source);
            }
            return;
        }

        if contains(&resolved[index].span, &candidate.span)
            || contains(&candidate.span, &resolved[index].span)
        {
            if let Some(tie) = family_tie_candidate(&candidate, &resolved[index], policy) {
                resolved[index] = tie;
                remove_overlaps(resolved, index, ConflictTier::CollisionPolicy);
            } else if let Some(tier) =
                should_replace_containment(&candidate, &resolved[index], policy, anchor_ctx)
            {
                let mut candidate = candidate;
                candidate.decided_by = tier;
                candidate
                    .merged_sources
                    .push(resolved[index].source.clone());
                resolved[index] = candidate;
                remove_overlaps(resolved, index, tier);
            } else {
                if let Some(tier) =
                    should_replace_containment(&resolved[index], &candidate, policy, anchor_ctx)
                {
                    resolved[index].decided_by = tier;
                }
                resolved[index].merged_sources.push(candidate.source);
            }
            return;
        }

        if let Some(tie) = family_tie_candidate(&candidate, &resolved[index], policy) {
            resolved[index] = tie;
            remove_overlaps(resolved, index, ConflictTier::CollisionPolicy);
        } else if let Some(tier) =
            should_replace_partial_overlap(&candidate, &resolved[index], policy, anchor_ctx)
        {
            let mut candidate = candidate;
            candidate.decided_by = tier;
            candidate
                .merged_sources
                .push(resolved[index].source.clone());
            resolved[index] = candidate;
            remove_overlaps(resolved, index, tier);
        } else {
            if let Some(tier) =
                should_replace_partial_overlap(&resolved[index], &candidate, policy, anchor_ctx)
            {
                resolved[index].decided_by = tier;
            }
            resolved[index].merged_sources.push(candidate.source);
        }
        return;
    }
    resolved.push(candidate);
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

fn should_replace_same_span_class(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Option<ConflictTier> {
    compare_by_spec(candidate, existing, policy, anchor_ctx)
}

fn should_replace_containment(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Option<ConflictTier> {
    if candidate.class == existing.class {
        let candidate_validated = candidate.canonical_form.is_some();
        let existing_validated = existing.canonical_form.is_some();
        if candidate_validated != existing_validated {
            return candidate_validated.then_some(ConflictTier::Validator);
        }

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

        return (candidate.recognizer_id < existing.recognizer_id)
            .then_some(ConflictTier::RecognizerId);
    }

    compare_by_spec(candidate, existing, policy, anchor_ctx)
}

fn should_replace_partial_overlap(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Option<ConflictTier> {
    compare_by_spec(candidate, existing, policy, anchor_ctx)
}

fn compare_by_spec(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Option<ConflictTier> {
    if let Some(candidate_wins) = policy.compare(&candidate.recognizer_id, &existing.recognizer_id)
    {
        return candidate_wins.then_some(ConflictTier::CollisionPolicy);
    }
    if let Some(anchor_ctx) = anchor_ctx {
        match anchor_ctx.resolver.resolve(
            candidate,
            anchor_ctx.input,
            policy,
            anchor_ctx.locale_chain,
        ) {
            AnchorOutcome::Found | AnchorOutcome::Missing { .. } => {
                return Some(ConflictTier::AnchoredContext);
            }
            AnchorOutcome::NotRequired => {}
        }
    }
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

fn remove_overlaps(resolved: &mut Vec<Candidate>, winner_index: usize, tier: ConflictTier) {
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
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "doc.alpha",
                crate::CollisionMembership::new("tenant-document", "alpha", 10, None),
            )
            .register_collision(
                "doc.beta",
                crate::CollisionMembership::new("tenant-document", "beta", 10, None),
            )
            .build();

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
}
