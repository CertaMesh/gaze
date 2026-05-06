use std::ops::Range;

use crate::{Candidate, ConflictTier, PiiClass};

pub fn resolve_candidates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
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
    for candidate in candidates {
        insert_candidate(&mut resolved, candidate);
    }
    resolved.sort_by_key(|a| a.span.start);
    resolved
}

fn insert_candidate(resolved: &mut Vec<Candidate>, candidate: Candidate) {
    let mut index = 0;
    while index < resolved.len() {
        if !overlaps(&resolved[index].span, &candidate.span) {
            index += 1;
            continue;
        }

        if resolved[index].span == candidate.span {
            if resolved[index].class == candidate.class {
                merge_same_span_same_class(&mut resolved[index], candidate);
                return;
            }
            if let Some(tier) = should_replace_same_span_class(&candidate, &resolved[index]) {
                let mut candidate = candidate;
                candidate.decided_by = tier;
                candidate
                    .merged_sources
                    .push(resolved[index].source.clone());
                resolved[index] = candidate;
            } else {
                if let Some(tier) = should_replace_same_span_class(&resolved[index], &candidate) {
                    resolved[index].decided_by = tier;
                }
                resolved[index].merged_sources.push(candidate.source);
            }
            return;
        }

        if contains(&resolved[index].span, &candidate.span)
            || contains(&candidate.span, &resolved[index].span)
        {
            if let Some(tier) = should_replace_containment(&candidate, &resolved[index]) {
                let mut candidate = candidate;
                candidate.decided_by = tier;
                candidate
                    .merged_sources
                    .push(resolved[index].source.clone());
                resolved[index] = candidate;
                remove_overlaps(resolved, index, tier);
            } else {
                if let Some(tier) = should_replace_containment(&resolved[index], &candidate) {
                    resolved[index].decided_by = tier;
                }
                resolved[index].merged_sources.push(candidate.source);
            }
            return;
        }

        if let Some(tier) = should_replace_partial_overlap(&candidate, &resolved[index]) {
            let mut candidate = candidate;
            candidate.decided_by = tier;
            candidate
                .merged_sources
                .push(resolved[index].source.clone());
            resolved[index] = candidate;
            remove_overlaps(resolved, index, tier);
        } else {
            if let Some(tier) = should_replace_partial_overlap(&resolved[index], &candidate) {
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
) -> Option<ConflictTier> {
    compare_by_spec(candidate, existing)
}

fn should_replace_containment(candidate: &Candidate, existing: &Candidate) -> Option<ConflictTier> {
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

    compare_by_spec(candidate, existing)
}

fn should_replace_partial_overlap(
    candidate: &Candidate,
    existing: &Candidate,
) -> Option<ConflictTier> {
    compare_by_spec(candidate, existing)
}

fn compare_by_spec(candidate: &Candidate, existing: &Candidate) -> Option<ConflictTier> {
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
        Candidate {
            span,
            class,
            recognizer_id: id.to_string(),
            score,
            priority: 0,
            canonical_form: None,
            token_family: "counter".to_string(),
            source: id.to_string(),
            decided_by: ConflictTier::None,
            merged_sources: Vec::new(),
        }
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
