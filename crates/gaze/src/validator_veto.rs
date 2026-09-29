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
    field_name: Option<&str>,
) -> (Vec<Candidate>, Vec<VetoedCandidate>) {
    let mut kept = Vec::with_capacity(candidates.len());
    let mut vetoed = Vec::new();

    for mut candidate in candidates {
        let Some(recognizer) = registry.recognizer(&candidate.recognizer_id) else {
            kept.push(candidate);
            continue;
        };
        // A weak, cue-less shape rule that opted in is vetoed when its match sits inside a
        // benign structure (an order number's tail, an amount, a room). Only this recognizer's
        // candidate goes; another candidate over the same bytes still protects them.
        // The allowlist is checked here too, so a custom `Recognizer` impl cannot opt in.
        if let Some(structure) = recognizer
            .benign_lookalikes()
            .iter()
            .filter(|_| gaze_types::benign_lookalike::is_audited(&candidate.recognizer_id))
            .find(|structure| structure.matches(input, candidate.span.clone(), field_name))
        {
            vetoed.push(VetoedCandidate {
                candidate,
                reason: structure.reason(),
            });
            continue;
        }
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
        // A recording (cue-anchored) card candidate passes the same way and keeps its WHOLE span:
        // after a card cue, a 4-4-4-4-3 number whose first 16 digits pass Luhn may be a mistyped
        // 19-digit card or a card and its CVV, and the digits cannot tell which, so both stay in
        // one token (the CVV is sensitive too). Its longer span then wins the same-class
        // containment over `card.structural`'s card; without a cue `card.structural` still
        // scopes a card exactly (todo 3843). Solo todo 3906, review of #694 round 3.
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
            // An opted-in recognizer can keep a failed candidate when its own shape and cue
            // provide enough evidence. The failure goes on its audit row, and `Learned`
            // evidence prevents the repeat-value sweep from spreading an unvalidated value.
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
        apply(vec![candidate], &registry, text, None, None)
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
            (
                ValidatorKind::DeSteuerIdMod1110,
                "86095742718",
                ValidatorFailReason::DeSteuerIdMod1110Failed,
            ),
            (
                ValidatorKind::BsnMod11,
                "123456780",
                ValidatorFailReason::BsnMod11Failed,
            ),
            (
                ValidatorKind::CpfMod11,
                "123.456.789-00",
                ValidatorFailReason::CpfMod11Failed,
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

    #[cfg(feature = "bundled-recognizers")]
    #[test]
    fn failed_phone_region_keeps_its_typed_reason() {
        use gaze_types::Region;

        let (kept, vetoed) = veto(
            ValidatorKind::E164PhoneNational(Region::Us),
            ValidatorOnFail::Record,
            // Ofcom's reserved +44 7700 900xxx drama range.
            "+44 7700 900123",
        );
        assert!(vetoed.is_empty());
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].validator_fail_reason,
            Some(ValidatorFailReason::PhoneNationalRegionMismatch)
        );
        assert_eq!(kept[0].evidence, EvidenceKind::Learned);
    }

    /// A recognizer can declare `Record` through the trait without going through the rulepack
    /// loader; veto still honours it only for the allowed validators.
    #[test]
    fn record_is_ignored_for_every_other_validator() {
        for (kind, text) in [
            (ValidatorKind::UkNhsMod11, "943 476 5918"),
            (ValidatorKind::EmailRfc, "alice@example"),
        ] {
            let (kept, vetoed) = veto(kind, ValidatorOnFail::Record, text);
            assert!(kept.is_empty(), "{kind:?} must still veto");
            assert_eq!(vetoed.len(), 1, "{kind:?}");
        }
    }

    /// A recording card span that holds a card is kept WHOLE, never narrowed: a 4-4-4-4-3
    /// number whose first 16 digits pass Luhn may be a mistyped 19-digit card (review of #694,
    /// round 3). A valid window deeper in the span keeps the whole span too.
    #[test]
    fn a_recorded_card_span_holding_a_card_stays_whole() {
        for text in [
            "6759 6498 2643 8453 012",
            "4111 1111 1111 1111 123",
            "1234 4111 1111 1111 1111",
        ] {
            let (kept, vetoed) = veto(ValidatorKind::Luhn, ValidatorOnFail::Record, text);
            assert!(vetoed.is_empty(), "{text}");
            assert_eq!(kept[0].span, 0..text.len(), "{text}");
            assert_eq!(kept[0].validator_fail_reason, None, "{text}");
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
