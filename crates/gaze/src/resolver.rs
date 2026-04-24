use std::ops::Range;

use crate::{Candidate, PiiClass};

pub fn resolve_candidates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| b.span.end.cmp(&a.span.end))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.recognizer_id.cmp(&b.recognizer_id))
    });

    let mut resolved: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        insert_candidate(&mut resolved, candidate);
    }
    resolved.sort_by(|a, b| a.span.start.cmp(&b.span.start));
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
            if should_replace_same_span_class(&candidate, &resolved[index]) {
                resolved[index] = candidate;
            }
            return;
        }

        if contains(&resolved[index].span, &candidate.span)
            || contains(&candidate.span, &resolved[index].span)
        {
            if should_replace_containment(&candidate, &resolved[index]) {
                resolved[index] = candidate;
            }
            return;
        }

        if should_replace_partial_overlap(&candidate, &resolved[index]) {
            resolved[index] = candidate;
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

fn should_replace_same_span_class(candidate: &Candidate, existing: &Candidate) -> bool {
    candidate
        .score
        .total_cmp(&existing.score)
        .is_gt()
        || (candidate.score == existing.score
            && class_priority(&candidate.class) > class_priority(&existing.class))
}

fn should_replace_containment(candidate: &Candidate, existing: &Candidate) -> bool {
    let candidate_validated = candidate.canonical_form.is_some();
    let existing_validated = existing.canonical_form.is_some();
    if candidate_validated != existing_validated {
        return candidate_validated;
    }

    class_priority(&candidate.class) > class_priority(&existing.class)
        || (class_priority(&candidate.class) == class_priority(&existing.class)
            && candidate.score.total_cmp(&existing.score).is_gt())
}

fn should_replace_partial_overlap(candidate: &Candidate, existing: &Candidate) -> bool {
    candidate.score.total_cmp(&existing.score).is_gt()
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
            canonical_form: None,
            token_family: "counter".to_string(),
            source: id.to_string(),
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
    fn exact_span_different_class_uses_score_then_class_priority() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Name, 0.90, "ner"),
            candidate(0..5, PiiClass::Email, 0.90, "regex"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    #[test]
    fn containment_prefers_validator_backed_candidate() {
        let mut validated = candidate(0..10, PiiClass::Name, 0.50, "validator");
        validated.canonical_form = Some("canonical".to_string());
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Email, 0.95, "regex"),
            validated,
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Name);
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
}
