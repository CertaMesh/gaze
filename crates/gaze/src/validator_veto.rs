use gaze_types::{Candidate, ValidatorFailReason, ValidatorOutcome};

use crate::registry::RecognizerRegistry;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VetoedCandidate {
    pub candidate: Candidate,
    pub reason: ValidatorFailReason,
}

pub fn apply(
    candidates: Vec<Candidate>,
    registry: &RecognizerRegistry,
    input: &str,
) -> (Vec<Candidate>, Vec<VetoedCandidate>) {
    let mut kept = Vec::with_capacity(candidates.len());
    let mut vetoed = Vec::new();

    for mut candidate in candidates {
        let Some(recognizer) = registry.recognizer(&candidate.recognizer_id) else {
            kept.push(candidate);
            continue;
        };
        let Some(kind) = recognizer.validator_kind() else {
            kept.push(candidate);
            continue;
        };
        let Some(raw) = input.get(candidate.span.clone()) else {
            kept.push(candidate);
            continue;
        };

        match kind.validate(raw) {
            ValidatorOutcome::Pass { canonical_form } => {
                if candidate.canonical_form.is_none() {
                    candidate.canonical_form = canonical_form;
                }
                kept.push(candidate);
            }
            ValidatorOutcome::Fail { reason } => {
                vetoed.push(VetoedCandidate { candidate, reason });
            }
            ValidatorOutcome::NotApplicable => kept.push(candidate),
            _ => kept.push(candidate),
        }
    }

    (kept, vetoed)
}
