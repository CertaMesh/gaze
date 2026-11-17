//! Typed protection outcomes, enforced on the raw candidate pool.
//!
//! Enforcement runs after detection and validator veto and before conflict resolution, token
//! allocation or any output. Resolution could otherwise let an overlapping candidate discard a
//! refusal, and a token minted before the refusal would already be staged.
//!
//! Refusals are collected, not raised at the first stage: a direct refusal defers until the
//! repeat sweep (D6) has added its own, so one typed error names every refused field of the
//! input. Neither stage mints a token or produces output before the error.

use super::*;
use crate::policy::PossiblySensitiveDisposition;
use gaze_types::{JsonScalarType, ProtectionOutcome, UntransformableReason};

/// `provenance_stage` of every refusal audit row. The row keeps the candidate's own outcome
/// (a refused possibly sensitive value stays `possibly_sensitive`), so this stage is what
/// records the refusal disposition.
pub(crate) const OUTCOME_REFUSAL_STAGE: &str = "primary_pipeline.outcome_refusal";

/// One refused candidate: why, and the value-free field it covers.
struct Refused {
    reason: UntransformableReason,
    candidate: Candidate,
}

/// Refusals found before output, raised together once every pre-output stage has run.
#[derive(Default)]
pub(super) struct Refusals(Vec<Refused>);

impl Refusals {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Pipeline {
    /// Apply each candidate's typed outcome under this pipeline's `[outcomes]` policy.
    ///
    /// A confirmed-untransformable candidate refuses the whole input. A possibly sensitive one
    /// is protected like a confirmed match, refused, or passed with an audit row, as the policy
    /// says; a possibly sensitive JSON number cannot be protected without changing its type, so
    /// `protect` refuses it too. Refused candidates leave the pool and are returned, not raised:
    /// the caller raises them with [`Self::raise_refusals`] after the repeat sweep, so every
    /// refused field is reported, in document order, and audited.
    pub(super) fn enforce_outcomes(
        &self,
        target: &ProtectionTarget<'_, '_>,
        mut pool: crate::resolver::CandidatePool,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Result<(crate::resolver::CandidatePool, Refusals)> {
        let originals = pool.take_originals();
        if originals
            .iter()
            .all(|candidate| candidate.outcome.is_none())
        {
            return Ok((
                crate::resolver::CandidatePool::new(originals),
                Refusals::default(),
            ));
        }
        let disposition = self.outcomes.possibly_sensitive;
        let mut kept = Vec::with_capacity(originals.len());
        let mut passed = Vec::new();
        let mut refused = Vec::new();
        for candidate in originals {
            match candidate.outcome {
                Some(ProtectionOutcome::ConfirmedUntransformable(reason)) => {
                    refused.push(Refused { reason, candidate })
                }
                Some(ProtectionOutcome::PossiblySensitive(_)) => match disposition {
                    PossiblySensitiveDisposition::Refuse => refused.push(Refused {
                        reason: UntransformableReason::NoSafeRendering,
                        candidate,
                    }),
                    PossiblySensitiveDisposition::PassAudited => passed.push(candidate),
                    _ if candidate
                        .field
                        .as_ref()
                        .is_some_and(|field| field.json_type == JsonScalarType::Number) =>
                    {
                        refused.push(Refused {
                            reason: UntransformableReason::JsonNumberNoTextOnlyPath,
                            candidate,
                        })
                    }
                    _ => kept.push(candidate),
                },
                // `NonSensitive` claims no protection; it never removes another candidate.
                Some(ProtectionOutcome::NonSensitive(_)) => {}
                _ => kept.push(candidate),
            }
        }
        for candidate in &passed {
            self.log_outcome(
                target,
                candidate,
                Action::Preserve,
                field_name,
                document_kind,
            )?;
        }
        // A class the policy leaves in place is not protected, so it has nothing to refuse.
        let context = build_context(field_name);
        refused.retain(|refused| {
            self.resolve_action(&refused.candidate.class, &context)
                .action
                .is_protective()
        });
        Ok((crate::resolver::CandidatePool::new(kept), Refusals(refused)))
    }

    /// Raise every collected refusal as one typed error, after the last pre-output stage.
    pub(super) fn raise_refusals(
        &self,
        target: &ProtectionTarget<'_, '_>,
        refusals: Refusals,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Result<()> {
        if refusals.is_empty() {
            return Ok(());
        }
        Err(self.refuse(target, refusals.0, field_name, document_kind))
    }

    /// Audit every refused candidate and build the typed error naming each refused field in
    /// document order. Callers have already dropped classes the policy leaves in place.
    fn refuse(
        &self,
        target: &ProtectionTarget<'_, '_>,
        mut refused: Vec<Refused>,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Error {
        refused.sort_by_key(|refused| (refused.candidate.span.start, refused.candidate.span.end));
        let context = build_context(field_name);
        for refused in &refused {
            let action = self
                .resolve_action(&refused.candidate.class, &context)
                .action;
            let mut candidate = refused.candidate.clone();
            // A refused possibly sensitive value keeps its outcome and reason: the audit row
            // must not upgrade policy-refused uncertainty into confirmed sensitivity. Only a
            // candidate without a typed outcome (a swept repeat) records the refusal reason.
            if candidate.outcome.is_none() {
                candidate.outcome =
                    Some(ProtectionOutcome::ConfirmedUntransformable(refused.reason));
            }
            if let Err(error) = self.log_outcome_at(
                target,
                &candidate,
                action,
                field_name,
                document_kind,
                Some(OUTCOME_REFUSAL_STAGE),
            ) {
                return error;
            }
        }
        let mut fields = refused.into_iter().map(|refused| {
            let path = refused
                .candidate
                .field
                .map(|field| field.path)
                .unwrap_or_default();
            (refused.reason, path)
        });
        let (reason, field_path) = fields.next().expect("at least one refusal");
        ProtectionError::Untransformable {
            reason,
            field_path,
            others: fields.collect(),
        }
        .into()
    }

    /// D6: a repeat of a field-bound value may not become a token inside a JSON number or
    /// object key. An exact whole-literal repeat there refuses, typed, before any token is
    /// minted; a repeat that only overlaps such a literal is a different value and is dropped
    /// rather than splitting the literal. The sweep's own length floors still apply, so short
    /// values never reach this check. Other classes keep their sweep behavior. Refused repeats
    /// join `refusals`; the caller raises them with any direct refusal.
    pub(super) fn guard_field_bound_repeats(
        &self,
        hits: Vec<crate::sweep::SweepHit>,
        normalized: &crate::normalize::NormalizedText,
        text: &str,
        whole_number: Option<&gaze_types::FieldProvenance>,
        refusals: &mut Refusals,
    ) -> Result<Vec<crate::sweep::SweepHit>> {
        let bound = self.registry.field_bound_classes();
        if !hits.iter().any(|hit| bound.contains(&hit.class)) {
            return Ok(hits);
        }
        let carriers = crate::field_context::json_carriers(text);
        let places = carriers.iter().flat_map(|carriers| {
            let numbers = carriers.numbers.iter().map(|(range, path)| {
                (range, path, UntransformableReason::JsonNumberNoTextOnlyPath)
            });
            let keys = carriers
                .keys
                .iter()
                .map(|(range, path)| (range, path, UntransformableReason::NoSafeRendering));
            numbers.chain(keys)
        });
        let places = places.collect::<Vec<_>>();
        let mut kept = Vec::with_capacity(hits.len());
        for hit in hits {
            if !bound.contains(&hit.class) {
                kept.push(hit);
                continue;
            }
            let raw = crate::normalize::raw_range(hit.span.clone(), &normalized.spans)
                .ok_or_else(|| clean_to_raw_mapping_error("unmappable sweep geometry"))?;
            let place = match whole_number {
                // A whole-input number leaf: every copy sits inside the number literal.
                Some(field) => Some((
                    0..text.len(),
                    field.path.clone(),
                    UntransformableReason::JsonNumberNoTextOnlyPath,
                )),
                None => places
                    .iter()
                    .find(|(range, ..)| range.start < raw.end && raw.start < range.end)
                    .map(|(range, path, reason)| ((*range).clone(), (*path).clone(), *reason)),
            };
            match place {
                None => kept.push(hit),
                Some((range, path, reason)) if range == raw => {
                    let candidate = hit.candidate().with_field(gaze_types::FieldProvenance::new(
                        path,
                        if reason == UntransformableReason::JsonNumberNoTextOnlyPath {
                            JsonScalarType::Number
                        } else {
                            JsonScalarType::String
                        },
                        gaze_types::FieldEvidence::ValidatedParse,
                    ));
                    refusals.0.push(Refused { reason, candidate });
                }
                Some(_) => {}
            }
        }
        Ok(kept)
    }

    /// Audit a decision that mints no token: a possibly sensitive pass.
    fn log_outcome(
        &self,
        target: &ProtectionTarget<'_, '_>,
        candidate: &Candidate,
        action: Action,
        field_name: Option<&str>,
        document_kind: DocumentKind,
    ) -> Result<()> {
        self.log_outcome_at(target, candidate, action, field_name, document_kind, None)
    }

    /// Audit a decision that mints no token, tagging the row with the stage that decided it.
    fn log_outcome_at(
        &self,
        target: &ProtectionTarget<'_, '_>,
        candidate: &Candidate,
        action: Action,
        field_name: Option<&str>,
        document_kind: DocumentKind,
        stage: Option<&str>,
    ) -> Result<()> {
        let mut entry = RedactionEntry::new(
            candidate.source.clone(),
            candidate.class.clone(),
            action,
            field_name.map(str::to_string),
            document_kind,
            false,
            candidate.decided_by,
            crate::redaction_log::current_epoch_ms(),
            Some(target.audit_session_id().to_string()),
        )
        .with_recognizer_metadata(
            Some(candidate.recognizer_id.clone()),
            candidate.recognizer_version_id.clone(),
        );
        if let Some(outcome) = candidate.outcome {
            entry = entry.with_protection_outcome(outcome);
        }
        if let Some(field) = &candidate.field {
            entry = entry.with_field_provenance(field);
        }
        entry.provenance_stage = stage.map(str::to_owned);
        for logger in &self.redaction_loggers {
            let _perf = gaze_types::perf_span!("audit_ns");
            logger.log(&entry)?;
        }
        Ok(())
    }
}
