use gaze_types::{AmbiguityReason, AmbiguityRecord, Candidate, LosingCandidate, PiiClass};

use crate::resolver::{FamilyEmission, FamilyEmissionReason};

#[derive(Clone)]
pub(crate) struct HybridFallbackRecord {
    pub(crate) collision_family: String,
    pub(crate) ambiguity_record: AmbiguityRecord,
}

pub(crate) fn emit(emission: &FamilyEmission, originals: &[Candidate]) -> HybridFallbackRecord {
    let family = emission.family.clone();
    let reason = match emission.reason {
        FamilyEmissionReason::PrecedenceTie => AmbiguityReason::PrecedenceTie,
        FamilyEmissionReason::NoAnchor => AmbiguityReason::NoAnchor,
    };
    let class = PiiClass::family(&family);
    let mut losing_candidates = emission
        .participants
        .iter()
        .map(|&id| {
            let original = &originals[id];
            LosingCandidate::new(original.class.clone(), original.recognizer_id.clone())
        })
        .collect::<Vec<_>>();
    losing_candidates.sort_by(|a, b| {
        a.recognizer_id
            .cmp(&b.recognizer_id)
            .then_with(|| a.class.cmp(&b.class))
    });
    losing_candidates.dedup_by(|a, b| a.recognizer_id == b.recognizer_id && a.class == b.class);
    HybridFallbackRecord {
        collision_family: family,
        ambiguity_record: AmbiguityRecord::new(class, losing_candidates, reason),
    }
}
