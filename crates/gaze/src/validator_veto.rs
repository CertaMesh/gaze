use gaze_types::{
    Candidate, EvidenceKind, ValidatorFailReason, ValidatorKind, ValidatorOnFail, ValidatorOutcome,
};

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
    source_spans: Option<&[(usize, usize)]>,
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

        // A `luhn` candidate may be the union of overlapping Luhn-valid windows of one digit run
        // (`gaze_types::payment_card::scan_card_run`), which fails Luhn as a whole. It passes
        // when the run still holds a card; a span holding none fails exactly as before.
        let outcome = match kind.validate(raw) {
            ValidatorOutcome::Fail { .. }
                if kind == ValidatorKind::Luhn
                    && gaze_types::payment_card::holds_card(
                        input,
                        candidate.span.clone(),
                        source_spans,
                    ) =>
            {
                ValidatorOutcome::Pass {
                    canonical_form: Some(raw.to_string()),
                }
            }
            outcome => outcome,
        };
        match outcome {
            ValidatorOutcome::Pass { canonical_form } => {
                if candidate.canonical_form.is_none() {
                    candidate.canonical_form = canonical_form;
                }
                kept.push(candidate);
            }
            // An IBAN or card the recognizer found by shape and context stays a candidate when
            // its checksum fails: a mistyped or masked number is still someone's financial data
            // (user ruling 2026-09-27). The failure goes on its audit row, and it is `Learned`
            // evidence, so the repeat-value sweep never spreads an unvalidated value.
            ValidatorOutcome::Fail { reason }
                if recognizer.validator_on_fail() == ValidatorOnFail::Record
                    && kind.allows_recorded_failure() =>
            {
                candidate.validator_fail_reason = Some(reason);
                candidate.evidence = EvidenceKind::Learned;
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
