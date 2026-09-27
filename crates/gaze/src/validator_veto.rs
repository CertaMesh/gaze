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

        let records = recognizer.validator_on_fail() == ValidatorOnFail::Record
            && kind.allows_recorded_failure();
        // A `luhn` candidate may be the union of overlapping Luhn-valid windows of one digit run
        // (`gaze_types::payment_card::scan_card_run`), which fails Luhn as a whole. It passes
        // when the run still holds a card; a span holding none fails exactly as before.
        //
        // A recording (cue-anchored) card recognizer passes only when its span STARTS with the
        // card, and is narrowed to it, so a valid card is scoped exactly as `card.structural`
        // scopes it (a trailing CVV stays outside, todo 3843). Otherwise it stays a recorded
        // failure over its whole span: a Luhn-failing 4-4-4-4-3 card is one token, never a
        // partial one, and a card-shaped window deeper in the span never leaves leading digits
        // raw (solo todo 3906).
        let outcome = match kind.validate(raw) {
            ValidatorOutcome::Fail { reason } if kind == ValidatorKind::Luhn => {
                let cards = gaze_types::payment_card::scan_card_run(
                    input,
                    candidate.span.clone(),
                    source_spans,
                )
                .cards;
                match cards.first().zip(cards.last()) {
                    Some(_) if !records => ValidatorOutcome::Pass {
                        canonical_form: Some(raw.to_string()),
                    },
                    Some((first, last)) if first.start == candidate.span.start => {
                        candidate.span = first.start..last.end;
                        ValidatorOutcome::Pass {
                            canonical_form: Some(input[candidate.span.clone()].to_string()),
                        }
                    }
                    _ => ValidatorOutcome::Fail { reason },
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
            ValidatorOutcome::Fail { reason } if records => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use gaze_types::{ConflictTier, DetectContext, DetectError, PiiClass, Recognizer};

    struct Probe {
        kind: ValidatorKind,
        on_fail: ValidatorOnFail,
        class: PiiClass,
    }

    impl Recognizer for Probe {
        fn id(&self) -> &str {
            "probe"
        }
        fn supported_class(&self) -> &PiiClass {
            &self.class
        }
        fn detect(
            &self,
            _: &str,
            _: &DetectContext<'_>,
        ) -> std::result::Result<Vec<Candidate>, DetectError> {
            Ok(Vec::new())
        }
        fn token_family(&self) -> &str {
            "counter"
        }
        fn validator_kind(&self) -> Option<ValidatorKind> {
            Some(self.kind)
        }
        fn validator_on_fail(&self) -> ValidatorOnFail {
            self.on_fail
        }
    }

    fn veto(
        kind: ValidatorKind,
        on_fail: ValidatorOnFail,
        text: &str,
    ) -> (Vec<Candidate>, Vec<VetoedCandidate>) {
        let class = PiiClass::custom("probe").expect("class");
        let registry = RecognizerRegistry::builder()
            .register(Probe {
                kind,
                on_fail,
                class: class.clone(),
            })
            .build();
        let candidate = Candidate::new(
            0..text.len(),
            class,
            "probe",
            0.9,
            80,
            None,
            "counter",
            "probe",
            ConflictTier::None,
            Vec::new(),
        )
        .with_evidence(EvidenceKind::Rule);
        apply(vec![candidate], &registry, text, None)
    }

    #[test]
    fn a_recorded_failure_keeps_the_candidate_as_learned_evidence() {
        for (kind, text, reason) in [
            (
                ValidatorKind::Luhn,
                "4111 1111 1111 1112",
                ValidatorFailReason::LuhnFailed,
            ),
            (
                ValidatorKind::IbanMod97,
                "DE99 3704 0044 0532 0130 00",
                ValidatorFailReason::IbanMod97Failed,
            ),
        ] {
            let (kept, vetoed) = veto(kind, ValidatorOnFail::Record, text);
            assert!(vetoed.is_empty(), "{kind:?}");
            assert_eq!(kept.len(), 1, "{kind:?}");
            assert_eq!(kept[0].validator_fail_reason, Some(reason));
            assert_eq!(kept[0].evidence, EvidenceKind::Learned, "{kind:?}");
        }
    }

    #[test]
    fn a_passing_candidate_keeps_its_evidence_and_records_nothing() {
        let (kept, _) = veto(
            ValidatorKind::Luhn,
            ValidatorOnFail::Record,
            "4111 1111 1111 1111",
        );
        assert_eq!(kept[0].validator_fail_reason, None);
        assert_eq!(kept[0].evidence, EvidenceKind::Rule);
    }

    /// A recognizer can declare `Record` through the trait without going through the rulepack
    /// loader; veto still honours it only for IBAN and Luhn.
    #[test]
    fn record_is_ignored_for_every_other_validator() {
        for (kind, text) in [
            (ValidatorKind::BsnMod11, "123456780"),
            (ValidatorKind::DeSteuerIdMod1110, "86095742718"),
            (ValidatorKind::UkNhsMod11, "943 476 5918"),
            (ValidatorKind::EmailRfc, "alice@example"),
        ] {
            let (kept, vetoed) = veto(kind, ValidatorOnFail::Record, text);
            assert!(kept.is_empty(), "{kind:?} must still veto");
            assert_eq!(vetoed.len(), 1, "{kind:?}");
        }
    }

    #[test]
    fn veto_is_the_default() {
        let (kept, vetoed) = veto(
            ValidatorKind::Luhn,
            ValidatorOnFail::Veto,
            "4111 1111 1111 1112",
        );
        assert!(kept.is_empty());
        assert_eq!(vetoed[0].reason, ValidatorFailReason::LuhnFailed);
    }
}
