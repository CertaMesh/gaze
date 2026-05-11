use gaze_types::{AmbiguityReason, AmbiguityRecord, Candidate, LosingCandidate, PiiClass};

use crate::registry::RecognizerRegistry;

pub(crate) struct HybridFallbackRecord {
    pub(crate) collision_family: String,
    pub(crate) ambiguity_record: AmbiguityRecord,
}

pub(crate) fn emit(
    candidate: &Candidate,
    registry: &RecognizerRegistry,
    reason: AmbiguityReason,
) -> Option<HybridFallbackRecord> {
    let family = candidate
        .recognizer_id
        .strip_prefix("collision-family:")?
        .to_string();
    let class = PiiClass::Custom(format!("family:{family}"));
    let mut losing_candidates = candidate
        .merged_sources
        .iter()
        .filter_map(|recognizer_id| {
            registry.recognizer(recognizer_id).map(|recognizer| {
                LosingCandidate::new(recognizer.supported_class().clone(), recognizer_id.clone())
            })
        })
        .collect::<Vec<_>>();
    losing_candidates.sort_by(|a, b| a.recognizer_id.cmp(&b.recognizer_id));
    Some(HybridFallbackRecord {
        collision_family: family,
        ambiguity_record: AmbiguityRecord::new(class, losing_candidates, reason),
    })
}
