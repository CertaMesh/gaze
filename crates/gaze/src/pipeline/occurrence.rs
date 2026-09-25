//! Owner-side occurrence authority. Compatibility manifests are immutable phase projections.
use super::*;
use std::cell::OnceCell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Residual {
        segment: usize,
        residual: usize,
    },
    Selection {
        segment: usize,
        selection: usize,
    },
    SafetyNet {
        observation: usize,
        relation: Relation,
        clean: Range<usize>,
    },
    /// A one-way `[REDACTED:<class>]` marker the safety net wrote over a flagged region.
    ///
    /// Unlike [`Origin::SafetyNet`], which resolves a suspect into a restorable token, this
    /// records a replacement that is deliberately not reversible. `clean` is the region as it
    /// stood BEFORE the marker was written, which is what the phase snapshot describes; a merged
    /// region carries every suspect that drove it, because merging must not merge away who asked
    /// for the redaction.
    SafetyNetRedaction {
        observations: Vec<usize>,
        clean: Range<usize>,
    },
    ExistingOwnedUnknown {
        segment: usize,
    },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Relation {
    WholeSuspect,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Batch {
    First,
    Second,
    Deletion,
    /// The one reversible round the terminal phase runs on its own scan, after the fallback has
    /// already deleted. Its own batch so the ledger records which round minted a token.
    Terminal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Basis {
    OriginalInput,
    ExpandedOwnedInput,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Occurrence {
    pub(super) id: usize,
    pub(super) emitted: EmittedTokenSpan,
    pub(super) action: Option<Action>,
    pub(super) owned: bool,
    pub(super) origin: Origin,
}
impl Occurrence {
    pub(super) fn new(
        emitted: EmittedTokenSpan,
        action: Action,
        owned: bool,
        origin: Origin,
    ) -> Self {
        Self {
            id: 0,
            emitted,
            action: Some(action),
            owned,
            origin,
        }
    }
    fn unknown(emitted: EmittedTokenSpan) -> Self {
        Self {
            id: 0,
            emitted,
            action: None,
            owned: false,
            origin: Origin::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Selection {
    pub(super) node: usize,
    pub(super) class: PiiClass,
    pub(super) members: Vec<usize>,
    pub(super) raw: Range<usize>,
    pub(super) recovered: bool,
    pub(super) action: Option<Action>,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Segment {
    pub(super) originals: Vec<Candidate>,
    pub(super) original_raw: Vec<Range<usize>>,
    pub(super) selections: Vec<Selection>,
    pub(super) residuals: Vec<super::residual::Cell>,
    pub(super) residual_order: Vec<usize>,
    /// Per original: admitted to residual coverage (its effective class
    /// previews a protective action). Lets `residual::validate` recompute the
    /// cell parents exactly as the planner filtered them.
    pub(super) residual_admitted: Vec<bool>,
    pub(super) events: Arc<[crate::resolver::ResolutionEvent]>,
    pub(super) raw_offset: usize,
    pub(super) clean_offset: usize,
    pub(super) basis: Basis,
}
#[derive(Clone, Debug, PartialEq)]
struct Phase {
    text_len: usize,
    projection: Arc<Manifest>,
    batch: Batch,
    raw_offset: usize,
    /// Original ranges already removed when this phase was taken, phase-local like `projection`.
    /// Without them the projection alone describes a document that no longer exists, and the raw
    /// re-derivation below would reject every token the terminal round mints.
    removed: Vec<Range<usize>>,
}
#[derive(Clone, Debug, PartialEq)]
struct Observation {
    phase: usize,
    suspect: LeakSuspect,
}
#[derive(Clone, Debug, PartialEq)]
struct Deletion {
    raw: Option<Range<usize>>,
    clean: Range<usize>,
    observations: Vec<usize>,
    removed: Vec<Occurrence>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Ledger {
    records: Vec<Occurrence>,
    segments: Vec<Segment>,
    phases: Vec<Phase>,
    observations: Vec<Observation>,
    deletions: Vec<Deletion>,
    next_id: usize,
    projection: OnceCell<Arc<Manifest>>,
    #[cfg(test)]
    projections: std::cell::Cell<usize>,
}
impl Ledger {
    pub(super) fn new(segment: Segment) -> Self {
        Self {
            segments: vec![segment],
            ..Self::default()
        }
    }
    pub(super) fn segment(&self) -> &Segment {
        &self.segments[0]
    }
    pub(super) fn set_residuals(
        &mut self,
        cells: Vec<super::residual::Cell>,
        order: Vec<usize>,
        admitted: Vec<bool>,
    ) {
        self.segments[0].residuals = cells;
        self.segments[0].residual_order = order;
        self.segments[0].residual_admitted = admitted;
    }
    pub(super) fn set_selection_action(&mut self, selection: usize, action: Action) {
        self.segments[0].selections[selection].action = Some(action);
    }
    pub(super) fn records(&self) -> &[Occurrence] {
        &self.records
    }
    /// Original-request ranges the fallback removed, in the order they were applied.
    ///
    /// Raw coordinates: unlike the clean spans on every other record, nothing a later edit does
    /// to the clean text shifts these, which is what makes a post-deletion clean/raw layout
    /// reconstructible at all. `None` is an undescribed deletion, which the caller fails closed
    /// on rather than guessing.
    pub(super) fn deleted_raw(&self) -> impl ExactSizeIterator<Item = Option<&Range<usize>>> {
        self.deletions.iter().map(|deletion| deletion.raw.as_ref())
    }
    pub(super) fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = &EmittedTokenSpan> + ExactSizeIterator {
        self.records.iter().map(|r| &r.emitted)
    }
    #[cfg(test)]
    pub(super) fn last(&self) -> Option<&EmittedTokenSpan> {
        self.records.last().map(|r| &r.emitted)
    }
    pub(super) fn len(&self) -> usize {
        self.records.len()
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub(super) fn projection(&self) -> &Manifest {
        self.project_arc().as_ref()
    }
    fn project_arc(&self) -> &Arc<Manifest> {
        self.projection.get_or_init(|| {
            #[cfg(test)]
            self.projections.set(self.projections.get() + 1);
            Arc::new(Manifest::from_spans(self.iter().cloned().collect()))
        })
    }
    pub(super) fn into_spans(self) -> Result<Vec<EmittedTokenSpan>> {
        self.validate()?;
        Ok(self.records.into_iter().map(|r| r.emitted).collect())
    }
    pub(super) fn insert(&mut self, mut record: Occurrence) {
        self.projection.take();
        record.id = self.next_id;
        self.next_id += 1;
        let index = self
            .records
            .partition_point(|r| r.emitted.clean_span.start <= record.emitted.clean_span.start);
        self.records.insert(index, record);
    }
    pub(super) fn phase(&mut self, text_len: usize, batch: Batch) -> usize {
        let removed = self
            .deletions
            .iter()
            .filter_map(|deletion| deletion.raw.clone())
            .collect();
        let projection = Arc::clone(self.project_arc());
        let id = self.phases.len();
        self.phases.push(Phase {
            text_len,
            projection,
            batch,
            raw_offset: 0,
            removed,
        });
        id
    }
    pub(super) fn observe(&mut self, phase: usize, suspect: &LeakSuspect) -> usize {
        // Callers supply the original report index once, then reuse this id for every gap.
        let id = self.observations.len();
        self.observations.push(Observation {
            phase,
            suspect: suspect.clone(),
        });
        id
    }
    pub(super) fn replace(
        &mut self,
        span: &Range<usize>,
        replacement_len: usize,
        emitted: Option<Occurrence>,
    ) {
        self.projection.take();
        let mut removed = Vec::new();
        self.records.retain_mut(|r| {
            if ranges_overlap(&r.emitted.clean_span, span) {
                removed.push(r.clone());
                false
            } else {
                if r.emitted.clean_span.start >= span.end {
                    let old = span.end - span.start;
                    if replacement_len >= old {
                        r.emitted.clean_span.start += replacement_len - old;
                        r.emitted.clean_span.end += replacement_len - old;
                    } else {
                        r.emitted.clean_span.start -= old - replacement_len;
                        r.emitted.clean_span.end -= old - replacement_len;
                    }
                }
                true
            }
        });
        if emitted.is_none() {
            self.deletions.push(Deletion {
                raw: None,
                clean: span.clone(),
                observations: Vec::new(),
                removed,
            });
        }
        if let Some(record) = emitted {
            self.insert(record);
        }
    }
    /// Unreachable from the product since the safety-net redact path started writing a
    /// `[REDACTED:<class>]` marker instead of cutting bytes: `Ledger::replace` only records a
    /// deletion when nothing is emitted, and that path was its sole caller.
    ///
    /// Kept, with the rest of the deletion model, until solo todo 3739 measures the removal. A
    /// half-removed deletion model is worse than either end state, and `CleanLayout`'s
    /// deletion-aware branch has to be retired in the same pass.
    #[allow(dead_code)]
    pub(super) fn describe_deletion(&mut self, raw: Range<usize>, observations: Vec<usize>) {
        let last = self
            .deletions
            .last_mut()
            .expect("deletion follows checked replacement");
        last.raw = Some(raw);
        last.observations = observations;
    }
    pub(super) fn existing_owned(&mut self, emitted: EmittedTokenSpan) {
        let segment = self.segments.len();
        self.segments.push(Segment {
            originals: Vec::new(),
            original_raw: Vec::new(),
            selections: Vec::new(),
            residuals: Vec::new(),
            residual_order: Vec::new(),
            residual_admitted: Vec::new(),
            events: Arc::from([]),
            raw_offset: emitted.raw_span.start,
            clean_offset: emitted.clean_span.start,
            basis: Basis::ExpandedOwnedInput,
        });
        self.insert(Occurrence {
            id: 0,
            emitted,
            action: None,
            owned: true,
            origin: Origin::ExistingOwnedUnknown { segment },
        });
    }
    pub(super) fn append(
        &mut self,
        mut other: Self,
        raw_offset: usize,
        clean_offset: usize,
    ) -> Result<()> {
        other.validate()?;
        let segment_offset = self.segments.len();
        let observation_offset = self.observations.len();
        let phase_offset = self.phases.len();
        for segment in &mut other.segments {
            segment.raw_offset = segment
                .raw_offset
                .checked_add(raw_offset)
                .ok_or_else(|| manifest_integrity_error("segment offset overflow"))?;
            segment.clean_offset = segment
                .clean_offset
                .checked_add(clean_offset)
                .ok_or_else(|| manifest_integrity_error("segment offset overflow"))?;
            segment.basis = Basis::ExpandedOwnedInput;
        }
        // Phase coordinates remain local to their original observation; only phase ids move.
        for phase in &mut other.phases {
            phase.raw_offset = phase
                .raw_offset
                .checked_add(raw_offset)
                .ok_or_else(|| manifest_integrity_error("phase offset overflow"))?;
        }
        for observation in &mut other.observations {
            observation.phase += phase_offset;
        }
        for mut record in other.records {
            record.emitted.raw_span = shift(&record.emitted.raw_span, raw_offset)?;
            record.emitted.clean_span = shift(&record.emitted.clean_span, clean_offset)?;
            remap_origin(&mut record.origin, segment_offset, observation_offset);
            self.insert(record);
        }
        for deletion in &mut other.deletions {
            if let Some(raw) = &mut deletion.raw {
                *raw = shift(raw, raw_offset)?;
            }
            deletion.clean = shift(&deletion.clean, clean_offset)?;
            for id in &mut deletion.observations {
                *id += observation_offset;
            }
            for record in &mut deletion.removed {
                record.id = self.next_id;
                self.next_id += 1;
                record.emitted.raw_span = shift(&record.emitted.raw_span, raw_offset)?;
                record.emitted.clean_span = shift(&record.emitted.clean_span, clean_offset)?;
                remap_origin(&mut record.origin, segment_offset, observation_offset);
            }
        }
        self.segments.extend(other.segments);
        self.phases.extend(other.phases);
        self.observations.extend(other.observations);
        self.deletions.extend(other.deletions);
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<()> {
        // The public fragment discriminator is derived from the internal origin,
        // so a record where they disagree is forged or drifted state. Consumers
        // decide whether to index a replacement as an entity on the strength of
        // it, so this fails closed rather than trusting either.
        //
        // This reads no segment and so belongs outside the segment loop: nested
        // it ran once per segment (quadratic on the protect path, where
        // `existing_owned` pushes a segment per carried token) and, worse, was
        // skipped entirely on a ledger with records and no segments — a
        // fail-closed check must not be gated on an unrelated collection being
        // non-empty.
        //
        // `Origin::Unknown` is skipped because it means "not recorded", not "not
        // a fragment". Records imported through `From<Vec<EmittedTokenSpan>>`
        // all carry it, so comparing against it would reject a legitimately
        // emitted fragment on round-trip. Every origin that is actually known is
        // still checked.
        for record in &self.records {
            if matches!(record.origin, Origin::Unknown) {
                continue;
            }
            if record.emitted.origin.is_residual_fragment()
                != matches!(record.origin, Origin::Residual { .. })
            {
                return Err(manifest_integrity_error(
                    "emitted span origin disagrees with occurrence origin",
                ));
            }
        }
        for segment in &self.segments {
            if segment.originals.len() != segment.original_raw.len() {
                return Err(manifest_integrity_error("evidence length mismatch"));
            }
            super::residual::validate(segment)?;
            use crate::resolver::{PairOutcome, ResolutionEvent};
            let mut pairs = BTreeMap::new();
            let mut structural_parents = std::collections::BTreeSet::new();
            for event in segment.events.iter() {
                if let ResolutionEvent::Pair {
                    existing,
                    incoming,
                    result,
                    outcome,
                } = event
                {
                    if *result < segment.originals.len()
                        || *existing >= *result
                        || *incoming >= *result
                        || existing == incoming
                        || (*existing >= segment.originals.len() && !pairs.contains_key(existing))
                        || (*incoming >= segment.originals.len() && !pairs.contains_key(incoming))
                        || pairs
                            .insert(*result, (*existing, *incoming, outcome))
                            .is_some()
                    {
                        return Err(manifest_integrity_error("invalid resolver node identity"));
                    }
                    match outcome {
                        PairOutcome::Merge | PairOutcome::Family => {
                            structural_parents.insert(*existing);
                            structural_parents.insert(*incoming);
                        }
                        PairOutcome::Existing(_) => {
                            structural_parents.insert(*existing);
                        }
                        PairOutcome::Incoming(_) => {
                            structural_parents.insert(*incoming);
                        }
                    }
                }
            }
            let mut nodes = std::collections::BTreeSet::new();
            let mut members = std::collections::BTreeSet::new();
            for selection in &segment.selections {
                let mut pending = vec![selection.node];
                let mut expected = Vec::new();
                let mut visited = std::collections::BTreeSet::new();
                while let Some(node) = pending.pop() {
                    if !visited.insert(node) {
                        return Err(manifest_integrity_error("repeated structural node"));
                    }
                    if node < segment.originals.len() {
                        expected.push(node);
                        continue;
                    }
                    let (existing, incoming, outcome) = pairs.get(&node).ok_or_else(|| {
                        manifest_integrity_error("unknown resolver selection node")
                    })?;
                    match outcome {
                        PairOutcome::Merge | PairOutcome::Family => {
                            pending.push(*incoming);
                            pending.push(*existing);
                        }
                        PairOutcome::Existing(_) => pending.push(*existing),
                        PairOutcome::Incoming(_) => pending.push(*incoming),
                    }
                }
                if expected != selection.members || (!selection.recovered && structural_parents.contains(&selection.node)) || (selection.recovered && !segment.events.iter().any(|e| matches!(e, ResolutionEvent::Recovery { node, raw, .. } if *node == selection.node && *raw == selection.raw))) {
                    return Err(manifest_integrity_error("resolver selection identity mismatch"));
                }
                if !nodes.insert(selection.node)
                    || selection.members.iter().any(|id| !members.insert(*id))
                    || selection.members.is_empty()
                    || selection
                        .members
                        .iter()
                        .any(|id| *id >= segment.originals.len())
                {
                    return Err(manifest_integrity_error("invalid selection members"));
                }
            }
        }
        for observation in &self.observations {
            let phase = self
                .phases
                .get(observation.phase)
                .ok_or_else(|| manifest_integrity_error("invalid observation phase"))?;
            if phase.batch != Batch::Deletion
                && (observation.suspect.span.start >= observation.suspect.span.end
                    || observation.suspect.span.end > phase.text_len)
            {
                return Err(manifest_integrity_error("invalid observed parent bounds"));
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for record in self
            .records
            .iter()
            .chain(self.deletions.iter().flat_map(|d| &d.removed))
        {
            if !ids.insert(record.id) || record.id >= self.next_id {
                return Err(manifest_integrity_error("invalid occurrence id"));
            }
            match &record.origin {
                Origin::Residual { segment, residual } => {
                    let segment = self
                        .segments
                        .get(*segment)
                        .ok_or_else(|| manifest_integrity_error("invalid residual segment"))?;
                    let cell = segment
                        .residuals
                        .get(*residual)
                        .ok_or_else(|| manifest_integrity_error("invalid residual identity"))?;
                    // A cell emits under its claimant's own action; only a
                    // token is a session-owned replacement.
                    if record.action != Some(cell.action)
                        || record.owned != (cell.action == Action::Tokenize)
                        || record.emitted.class != cell.class
                        || record.emitted.raw_span != shift(&cell.raw, segment.raw_offset)?
                    {
                        return Err(manifest_integrity_error("invalid residual disposition"));
                    }
                }
                Origin::Selection { segment, selection } => {
                    let segment = self
                        .segments
                        .get(*segment)
                        .ok_or_else(|| manifest_integrity_error("invalid segment id"))?;
                    let selected = segment
                        .selections
                        .get(*selection)
                        .ok_or_else(|| manifest_integrity_error("invalid selection id"))?;
                    if selected.class != record.emitted.class
                        || selected.action != record.action
                        || shift(&selected.raw, segment.raw_offset)? != record.emitted.raw_span
                    {
                        return Err(manifest_integrity_error("selection raw mismatch"));
                    }
                }
                Origin::SafetyNet {
                    observation,
                    relation,
                    clean,
                } => {
                    let observation = self
                        .observations
                        .get(*observation)
                        .ok_or_else(|| manifest_integrity_error("invalid observation id"))?;
                    let phase = self
                        .phases
                        .get(observation.phase)
                        .ok_or_else(|| manifest_integrity_error("invalid phase id"))?;
                    if clean.start >= clean.end
                        || clean.end > phase.text_len
                        || clean.start < observation.suspect.span.start
                        || clean.end > observation.suspect.span.end
                        || (*relation == Relation::WholeSuspect
                            && *clean != observation.suspect.span)
                    {
                        return Err(manifest_integrity_error(
                            "invalid observed replacement relation",
                        ));
                    }
                    // `map_clean_boundary_to_raw` assumes the projection describes the whole
                    // document, which stops being true the moment the fallback deletes from it,
                    // so a phase that follows a deletion is re-derived through its own layout.
                    let (start, end) = if phase.removed.is_empty() {
                        (
                            map_clean_boundary_to_raw(&phase.projection.spans, clean.start),
                            map_clean_boundary_to_raw(&phase.projection.spans, clean.end),
                        )
                    } else {
                        let layout = CleanLayout::from_parts(
                            phase.projection.spans.iter(),
                            phase.removed.clone(),
                            phase.text_len,
                        )?;
                        (
                            layout.boundary(clean.start, Side::Start),
                            layout.boundary(clean.end, Side::End),
                        )
                    };
                    let start = start.and_then(|v| v.checked_add(phase.raw_offset));
                    let end = end.and_then(|v| v.checked_add(phase.raw_offset));
                    if start != Some(record.emitted.raw_span.start)
                        || end != Some(record.emitted.raw_span.end)
                    {
                        return Err(manifest_integrity_error("observed raw mapping mismatch"));
                    }
                    if matches!(observation.suspect.kind, LeakKind::PartialBleed { .. })
                        && *relation != Relation::Gap
                    {
                        return Err(manifest_integrity_error("partial observation is not whole"));
                    }
                    if record.action != Some(Action::Tokenize)
                        || !record.owned
                        || record.emitted.class != observation.suspect.class
                    {
                        return Err(manifest_integrity_error(
                            "invalid observed replacement ownership",
                        ));
                    }
                }
                Origin::SafetyNetRedaction {
                    observations,
                    clean,
                } => {
                    if observations.is_empty() {
                        return Err(manifest_integrity_error("redaction names no observation"));
                    }
                    // Every suspect that drove the region must be a real observation of the same
                    // deletion-batch phase. A redaction that cannot say who asked for it is not
                    // auditable, and axis 4 does not allow an untraceable one-way replacement.
                    let mut phases = observations.iter().map(|id| {
                        self.observations
                            .get(*id)
                            .ok_or_else(|| {
                                manifest_integrity_error("invalid redaction observation")
                            })
                            .map(|observation| observation.phase)
                    });
                    let first = phases
                        .next()
                        .expect("non-empty observations checked above")?;
                    for phase in phases {
                        if phase? != first {
                            return Err(manifest_integrity_error(
                                "redaction spans more than one phase",
                            ));
                        }
                    }
                    let phase = self
                        .phases
                        .get(first)
                        .ok_or_else(|| manifest_integrity_error("invalid redaction phase"))?;
                    if phase.batch != Batch::Deletion {
                        return Err(manifest_integrity_error("invalid redaction basis"));
                    }
                    if clean.start >= clean.end || clean.end > phase.text_len {
                        return Err(manifest_integrity_error("invalid redaction bounds"));
                    }
                    // Re-derive the raw span from the phase snapshot, exactly as the resolve arm
                    // above does. The marker is a replacement rather than a hole, so the snapshot
                    // stays affine and needs no deletion-aware layout.
                    let start = map_clean_boundary_to_raw(&phase.projection.spans, clean.start)
                        .and_then(|v| v.checked_add(phase.raw_offset));
                    let end = map_clean_boundary_to_raw(&phase.projection.spans, clean.end)
                        .and_then(|v| v.checked_add(phase.raw_offset));
                    if start != Some(record.emitted.raw_span.start)
                        || end != Some(record.emitted.raw_span.end)
                    {
                        return Err(manifest_integrity_error("redaction raw mapping mismatch"));
                    }
                    // Ownership is enforced once, below, by the rule every one-way replacement
                    // shares; repeating it here would only be a second copy to keep in step.
                    if record.action != Some(Action::Redact) {
                        return Err(manifest_integrity_error("invalid redaction action"));
                    }
                    // The region must be explained by the suspects it names, the way the resolve
                    // arm above requires its replacement to sit inside its observed suspect. The
                    // direction is the other way round here: a redaction merges suspects and
                    // expands outward over every manifest entry it swallows, so each driving
                    // suspect's ACTION span lies inside the region rather than containing it.
                    // Without this a record could name observations that had nothing to do with
                    // the bytes it replaced, and the audit trail would credit a redaction to
                    // suspects that did not ask for it.
                    //
                    // The action span, not `suspect.span`: a `PartialBleed` suspect is acted on
                    // over its uncovered sub-range only, so requiring the whole suspect span here
                    // would reject honest output.
                    let mut classes = Vec::with_capacity(observations.len());
                    for id in observations {
                        let observation = self.observations.get(*id).ok_or_else(|| {
                            manifest_integrity_error("invalid redaction observation")
                        })?;
                        let acted = super::suspect_action_span(&observation.suspect);
                        if acted.start < clean.start || acted.end > clean.end {
                            return Err(manifest_integrity_error(
                                "redaction does not cover its observation",
                            ));
                        }
                        classes.push(&observation.suspect.class);
                    }
                    // A merged region carries the class of one of its suspects -- the emitter uses
                    // the lowest-offset one -- and that class is what the marker renders and what
                    // the reader of the clean document is told was removed. Membership rather than
                    // "the lowest" is deliberate: the emitter orders by EXPANDED region start,
                    // which the ledger cannot reconstruct from observations alone, and a check
                    // that guessed at that ordering would reject honest output. Membership still
                    // rejects a class no suspect ever asked for, which is the mislabelling this
                    // guards against.
                    if !classes.contains(&&record.emitted.class) {
                        return Err(manifest_integrity_error("invalid redaction class"));
                    }
                }
                Origin::ExistingOwnedUnknown { segment } => {
                    if !record.owned
                        || self
                            .segments
                            .get(*segment)
                            .is_none_or(|s| s.basis != Basis::ExpandedOwnedInput)
                    {
                        return Err(manifest_integrity_error("invalid reconstructed ownership"));
                    }
                }
                Origin::Unknown => {}
            }
            if record.action == Some(Action::Tokenize) && !record.owned {
                return Err(manifest_integrity_error(
                    "tokenizing occurrence is not owned",
                ));
            }
            if matches!(record.action, Some(Action::Redact | Action::Generalize)) && record.owned {
                return Err(manifest_integrity_error(
                    "one-way replacement cannot be owned",
                ));
            }
        }
        for deletion in &self.deletions {
            for id in &deletion.observations {
                let observation = self
                    .observations
                    .get(*id)
                    .ok_or_else(|| manifest_integrity_error("invalid deletion observation"))?;
                let phase = self
                    .phases
                    .get(observation.phase)
                    .ok_or_else(|| manifest_integrity_error("invalid deletion phase"))?;
                if phase.batch != Batch::Deletion {
                    return Err(manifest_integrity_error("invalid deletion basis"));
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn evidence_count(&self) -> usize {
        self.segments.iter().map(|s| s.originals.len()).sum()
    }
    #[cfg(test)]
    pub(super) fn push(&mut self, span: EmittedTokenSpan) {
        self.insert(Occurrence::unknown(span));
    }
    #[cfg(test)]
    pub(super) fn swap(&mut self, a: usize, b: usize) {
        self.projection.take();
        self.records.swap(a, b);
    }
}
fn shift(span: &Range<usize>, offset: usize) -> Result<Range<usize>> {
    Ok(span
        .start
        .checked_add(offset)
        .ok_or_else(|| manifest_integrity_error("raw offset overflow"))?
        ..span
            .end
            .checked_add(offset)
            .ok_or_else(|| manifest_integrity_error("raw offset overflow"))?)
}
fn remap_origin(origin: &mut Origin, segment: usize, observation: usize) {
    match origin {
        Origin::Residual { segment: id, .. }
        | Origin::Selection { segment: id, .. }
        | Origin::ExistingOwnedUnknown { segment: id } => *id += segment,
        Origin::SafetyNet {
            observation: id, ..
        } => *id += observation,
        Origin::SafetyNetRedaction { observations, .. } => {
            for id in observations {
                *id += observation;
            }
        }
        Origin::Unknown => {}
    }
}
impl From<Vec<EmittedTokenSpan>> for Ledger {
    fn from(spans: Vec<EmittedTokenSpan>) -> Self {
        let records = spans
            .into_iter()
            .enumerate()
            .map(|(id, span)| Occurrence {
                id,
                ..Occurrence::unknown(span)
            })
            .collect::<Vec<_>>();
        Self {
            next_id: records.len(),
            records,
            ..Self::default()
        }
    }
}
impl<'a> IntoIterator for &'a Ledger {
    type Item = &'a EmittedTokenSpan;
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, Occurrence>,
        fn(&'a Occurrence) -> &'a EmittedTokenSpan,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.records.iter().map(|r| &r.emitted)
    }
}
#[cfg(test)]
impl std::ops::Index<usize> for Ledger {
    type Output = EmittedTokenSpan;
    fn index(&self, index: usize) -> &Self::Output {
        &self.records[index].emitted
    }
}
#[cfg(test)]
impl std::ops::IndexMut<usize> for Ledger {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.projection.take();
        &mut self.records[index].emitted
    }
}
#[cfg(test)]
impl PartialEq for Ledger {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl<const N: usize> PartialEq<[EmittedTokenSpan; N]> for Ledger {
    fn eq(&self, other: &[EmittedTokenSpan; N]) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary(raw: &str, action: Action) -> (Session, CleanText) {
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let pipeline = Pipeline::builder()
            .detector(Email)
            .rule(crate::rule::DefaultRule::new(action))
            .build()
            .unwrap();
        let clean = pipeline
            .redact_text_with_manifest_uncached(
                &mut ProtectionTarget::Live(&session),
                raw,
                None,
                DocumentKind::Text,
                &[crate::LocaleTag::Global],
                &DictionaryBundle::default(),
                None,
            )
            .unwrap();
        (session, clean)
    }
    struct Email;
    impl Detector for Email {
        fn detect(&self, text: &str) -> Vec<Detection> {
            text.match_indices("alice@example.invalid")
                .map(|(start, s)| {
                    Detection::new(start..start + s.len(), PiiClass::Email, "synthetic.email")
                })
                .collect()
        }
    }
    fn suspect(span: Range<usize>, kind: LeakKind) -> LeakSuspect {
        LeakSuspect::new(
            span,
            PiiClass::Name,
            "synthetic.net",
            Some(0.9),
            kind,
            "synthetic",
            None,
        )
    }
    fn identifier(mut suspect: LeakSuspect) -> LeakSuspect {
        suspect.class = PiiClass::custom("synthetic_id").unwrap();
        suspect
    }
    fn resolve(clean: &mut CleanText, session: &Session, suspects: Vec<LeakSuspect>, batch: Batch) {
        let report = LeakReport::from_parts(suspects, Vec::new());
        let pipeline = Pipeline::builder().build().unwrap();
        let mut target = ProtectionTarget::Live(session);
        match batch {
            Batch::First => assert_eq!(
                pipeline
                    .resolve_safety_net_suspects(
                        &mut target,
                        clean,
                        &report,
                        DocumentKind::Text,
                        None,
                        None
                    )
                    .unwrap(),
                None
            ),
            Batch::Second | Batch::Terminal => {
                let FollowupResolution::Ready(plan) =
                    plan_followup_resolutions(&target, clean, &report, None).unwrap()
                else {
                    panic!("ready follow-up batch")
                };
                pipeline
                    .apply_followup_resolutions(
                        &mut target,
                        clean,
                        plan,
                        DocumentKind::Text,
                        None,
                        batch,
                        None,
                        None,
                    )
                    .unwrap();
            }
            Batch::Deletion => unreachable!(),
        }
        clean.manifest.validate().unwrap();
    }

    #[test]
    fn both_batches_keep_one_observation_for_gaps_across_owned_interiors() {
        for batch in [Batch::First, Batch::Second] {
            let (session, mut clean) = primary("aaalice@example.invalidbb", Action::Tokenize);
            let end = clean.text.len();
            resolve(
                &mut clean,
                &session,
                vec![suspect(0..end, LeakKind::PartialBleed { uncovered: 0..2 })],
                batch,
            );
            assert_eq!(clean.manifest.observations.len(), 1);
            assert_eq!(clean.manifest.phases.len(), 1);
            assert_eq!(clean.manifest.phases[0].batch, batch);
            assert_eq!(clean.manifest.phases[0].text_len, end);
            assert_eq!(clean.manifest.observations[0].suspect.span, 0..end);
            let gaps = clean
                .manifest
                .records
                .iter()
                .filter(|r| matches!(r.origin, Origin::SafetyNet { .. }))
                .collect::<Vec<_>>();
            assert_eq!(gaps.len(), 2);
            assert!(gaps.iter().all(|r| matches!(
                r.origin,
                Origin::SafetyNet {
                    observation: 0,
                    relation: Relation::Gap,
                    ..
                }
            )));
            assert_eq!(
                gaps.iter()
                    .map(|r| r.emitted.raw_span.clone())
                    .collect::<Vec<_>>(),
                vec![0..2, 23..25]
            );
            assert_eq!(
                session.restore_strict_text(&clean.text).unwrap(),
                "aaalice@example.invalidbb"
            );
            // A clean parent may cross a token interior. Its observation remains phase-local.
            let (session, mut interior) = primary("aaalice@example.invalidbb", Action::Tokenize);
            let end = interior.manifest[0].clean_span.start + 3;
            resolve(
                &mut interior,
                &session,
                vec![suspect(0..end, LeakKind::PartialBleed { uncovered: 0..2 })],
                batch,
            );
            assert_eq!(interior.manifest.observations[0].suspect.span, 0..end);
            assert!(matches!(
                interior.manifest.records[0].origin,
                Origin::SafetyNet {
                    relation: Relation::Gap,
                    ..
                }
            ));
            assert_eq!(interior.manifest.records[0].emitted.raw_span, 0..2);
        }
    }

    #[test]
    fn second_batch_uses_its_own_phase_and_whole_observation_is_not_selection() {
        let (session, mut clean) = primary("aaalice@example.invalidbbcc", Action::Tokenize);
        resolve(
            &mut clean,
            &session,
            vec![suspect(0..2, LeakKind::Uncovered)],
            Batch::First,
        );
        let old_len = clean.text.len();
        resolve(
            &mut clean,
            &session,
            vec![suspect(old_len - 4..old_len - 2, LeakKind::Uncovered)],
            Batch::Second,
        );
        assert_eq!(clean.manifest.phases.len(), 2);
        assert_eq!(clean.manifest.phases[1].text_len, old_len);
        assert_eq!(clean.manifest.observations[1].phase, 1);
        assert!(
            clean
                .manifest
                .records
                .iter()
                .filter(|r| matches!(
                    r.origin,
                    Origin::SafetyNet {
                        relation: Relation::WholeSuspect,
                        ..
                    }
                ))
                .count()
                == 2
        );
        assert_eq!(
            session.restore_strict_text(&clean.text).unwrap(),
            "aaalice@example.invalidbbcc"
        );
    }

    #[test]
    fn one_way_and_format_preserve_ownership_stays_separate_from_selection() {
        for action in [
            Action::Tokenize,
            Action::FormatPreserve,
            Action::Redact,
            Action::Generalize,
            Action::Preserve,
        ] {
            let (session, clean) = primary("alice@example.invalid", action);
            assert_eq!(
                clean.manifest.segments[0].selections[0].action,
                Some(action)
            );
            clean.manifest.validate().unwrap();
            if action == Action::Preserve {
                assert!(clean.manifest.records.is_empty());
                continue;
            }
            let record = &clean.manifest.records[0];
            assert_eq!(record.action, Some(action));
            assert_eq!(
                record.owned,
                matches!(action, Action::Tokenize | Action::FormatPreserve)
            );
            assert_eq!(record.owned, session.contains_token(&clean.text));
            assert!(matches!(record.origin, Origin::Selection { .. }));
        }
    }

    #[test]
    fn composition_moves_evidence_ids_and_offsets_without_copying_payloads() {
        let (_, left) = primary("alice@example.invalid", Action::Tokenize);
        let (_, right) = primary("alice@example.invalid", Action::Tokenize);
        let left_ptr = left.manifest.segments[0].originals.as_ptr();
        let right_ptr = right.manifest.segments[0].originals.as_ptr();
        let mut ledger = Ledger::default();
        ledger.append(left.manifest, 2, 3).unwrap();
        ledger.append(right.manifest, 30, 40).unwrap();
        ledger.existing_owned(EmittedTokenSpan::new(80..90, 60..65, PiiClass::Name));
        ledger.validate().unwrap();
        assert_eq!(ledger.segments[0].originals.as_ptr(), left_ptr);
        assert_eq!(ledger.segments[1].originals.as_ptr(), right_ptr);
        assert_eq!(ledger.records[0].emitted.raw_span, 2..23);
        assert_eq!(ledger.records[1].emitted.raw_span, 30..51);
        assert_eq!(
            ledger.records[1].origin,
            Origin::Selection {
                segment: 1,
                selection: 0
            }
        );
        assert_eq!(
            ledger.records[2].origin,
            Origin::ExistingOwnedUnknown { segment: 2 }
        );
        assert!(ledger
            .segments
            .iter()
            .all(|s| s.basis == Basis::ExpandedOwnedInput));
    }

    #[test]
    fn corrupted_identity_membership_mapping_and_ownership_fail_validation() {
        let (_, clean) = primary("alice@example.invalid", Action::Tokenize);
        for corruption in 0..7 {
            let mut ledger = clean.manifest.clone();
            match corruption {
                0 => ledger.records[0].id = 100,
                1 => {
                    ledger.records[0].origin = Origin::Selection {
                        segment: 100,
                        selection: 0,
                    }
                }
                2 => {
                    ledger.records[0].origin = Origin::Selection {
                        segment: 0,
                        selection: 100,
                    }
                }
                3 => ledger.segments[0].selections[0].members = vec![100],
                4 => ledger.records[0].emitted.raw_span = 1..22,
                5 => ledger.records[0].action = Some(Action::Redact),
                6 => ledger.segments[0].original_raw.clear(),
                _ => unreachable!(),
            }
            assert!(ledger.validate().is_err(), "corruption {corruption}");
        }
        let mut ledger: Ledger = vec![EmittedTokenSpan::new(0..3, 0..2, PiiClass::Name)].into();
        assert_eq!(ledger.records[0].origin, Origin::Unknown);
        assert!(!ledger.records[0].owned);
        ledger.records[0].origin = Origin::SafetyNet {
            observation: 3,
            relation: Relation::WholeSuspect,
            clean: 0..3,
        };
        assert!(ledger.validate().is_err());
    }

    #[test]
    fn valid_but_wrong_source_member_and_node_cannot_replace_selected_identity() {
        struct Ambiguous;
        impl Detector for Ambiguous {
            fn detect(&self, text: &str) -> Vec<Detection> {
                vec![
                    Detection::new(0..text.len(), PiiClass::Email, "synthetic.email"),
                    Detection::new(0..text.len(), PiiClass::Name, "synthetic.name"),
                ]
            }
        }
        let session = Session::new(crate::Scope::Ephemeral).unwrap();
        let pipeline = Pipeline::builder()
            .detector(Ambiguous)
            .rule(crate::rule::DefaultRule::new(Action::Tokenize))
            .build()
            .unwrap();
        let clean = pipeline
            .redact_text_with_manifest_uncached(
                &mut ProtectionTarget::Live(&session),
                "alice@example.invalid",
                None,
                DocumentKind::Text,
                &[crate::LocaleTag::Global],
                &DictionaryBundle::default(),
                None,
            )
            .unwrap();
        let selected = &clean.manifest.segments[0].selections[0];
        assert_eq!(selected.members.len(), 1);
        let other = 1 - selected.members[0];
        for mode in 0..2 {
            let mut corrupt = clean.manifest.clone();
            if mode == 0 {
                corrupt.segments[0].selections[0].members[0] = other;
            } else {
                corrupt.segments[0].selections[0].node = other;
            }
            assert!(corrupt.validate().is_err(), "wrong source identity {mode}");
        }
    }

    #[test]
    fn tokenizing_selection_cannot_lose_its_actual_owned_disposition() {
        let (_, mut clean) = primary("alice@example.invalid", Action::Tokenize);
        clean.manifest.records[0].owned = false;
        assert!(clean.manifest.validate().is_err());
    }

    #[test]
    fn repeated_member_identity_cannot_fabricate_independent_evidence() {
        let (_, mut clean) = primary("alice@example.invalid", Action::Tokenize);
        clean.manifest.segments[0].selections[0].members.push(0);
        assert!(clean.manifest.validate().is_err());
    }

    #[test]
    fn terminal_continuity_rejects_reassigned_occurrence_identity() {
        let (session, mut clean) = primary("alice@example.invalid", Action::Tokenize);
        let target = ProtectionTarget::Live(&session);
        let before = TerminalManifestProvenance::capture(&target, &clean).unwrap();
        clean.manifest.records[0].id = 1;
        clean.manifest.next_id = 2;
        clean.manifest.validate().unwrap();
        assert!(validate_terminal_manifest(&target, &clean, &before).is_err());
    }

    #[test]
    fn phase_projection_is_shared_across_nets_suspects_and_repeated_reads() {
        let raw = std::iter::repeat_n("alice@example.invalid ", 64).collect::<String>();
        let (_, mut clean) = primary(&raw, Action::Tokenize);
        let original = clean.manifest.segments[0].originals.as_ptr();
        for _ in 0..128 {
            assert_eq!(clean.manifest.projection().spans.len(), 64);
        }
        let phase = clean.manifest.phase(clean.text.len(), Batch::First);
        for _ in 0..128 {
            clean
                .manifest
                .observe(phase, &suspect(0..2, LeakKind::Uncovered));
        }
        assert_eq!(clean.manifest.projections.get(), 1);
        assert_eq!(clean.manifest.phases.len(), 1);
        assert_eq!(clean.manifest.segments[0].originals.as_ptr(), original);
        assert_eq!(clean.manifest.evidence_count(), 64);
        assert_eq!(Arc::strong_count(clean.manifest.project_arc()), 2);
    }

    #[test]
    fn many_gap_production_retains_one_payload_table_and_one_phase_projection() {
        let raw = format!(
            " {}",
            std::iter::repeat_n("alice@example.invalid ", 64).collect::<String>()
        );
        let (session, mut clean) = primary(&raw, Action::Tokenize);
        let payloads = clean.manifest.segments[0].originals.as_ptr();
        let end = clean.text.len();
        resolve(
            &mut clean,
            &session,
            vec![suspect(0..end, LeakKind::PartialBleed { uncovered: 0..1 })],
            Batch::First,
        );
        assert_eq!(clean.manifest.records.len(), 129);
        assert_eq!(clean.manifest.evidence_count(), 64);
        assert_eq!(clean.manifest.segments[0].originals.as_ptr(), payloads);
        assert_eq!(clean.manifest.observations.len(), 1);
        assert_eq!(clean.manifest.phases.len(), 1);
        assert_eq!(clean.manifest.phases[0].projection.spans.len(), 64);
        assert_eq!(clean.manifest.projections.get(), 1);
        for _ in 0..16 {
            assert_eq!(clean.manifest.projection().spans.len(), 129);
        }
        assert_eq!(clean.manifest.projections.get(), 2);
        assert_eq!(session.restore_strict_text(&clean.text).unwrap(), raw);
    }

    #[test]
    fn real_two_net_orchestration_reuses_then_invalidates_phase_projections() {
        use std::sync::Mutex;
        type Calls = Arc<Mutex<Vec<(usize, usize, String, usize, usize)>>>;
        struct Script {
            net: usize,
            step: Mutex<usize>,
            calls: Calls,
        }
        impl SafetyNet for Script {
            fn id(&self) -> &str {
                if self.net == 0 {
                    "synthetic.net"
                } else {
                    "synthetic.observer"
                }
            }
            fn supported_locales(&self) -> &[crate::LocaleTag] {
                &[crate::LocaleTag::Global]
            }
            fn check(
                &self,
                text: &str,
                context: SafetyNetContext<'_>,
            ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
                let mut step = self.step.lock().unwrap();
                self.calls.lock().unwrap().push((
                    self.net,
                    *step,
                    text.to_owned(),
                    context.manifest as *const Manifest as usize,
                    context.manifest.spans.len(),
                ));
                assert!(*step < 4, "no extra sweep");
                let result = if self.net == 1 {
                    vec![]
                } else {
                    match *step {
                        // Identifier class: these glued fixture spans cut words, and the
                        // sub-word guard exempts only identifier classes.
                        0 => vec![identifier(suspect(0..2, LeakKind::Uncovered))],
                        1 => vec![identifier(suspect(
                            text.len() - 4..text.len() - 2,
                            LeakKind::Uncovered,
                        ))],
                        2 => vec![suspect(
                            text.len() - 2..text.len(),
                            LeakKind::ClassMismatch {
                                pipeline_class: PiiClass::Email,
                                safety_net_class: PiiClass::Name,
                            },
                        )],
                        3 => vec![],
                        _ => unreachable!(),
                    }
                };
                *step += 1;
                Ok(result)
            }
        }
        let (session, mut clean) = primary("aaalice@example.invalidbbcc", Action::Tokenize);
        let payloads = clean.manifest.segments[0].originals.as_ptr();
        let calls = Calls::default();
        let pipeline = Pipeline::builder()
            .register_safety_net(Script {
                net: 0,
                step: Mutex::new(0),
                calls: calls.clone(),
            })
            .register_safety_net(Script {
                net: 1,
                step: Mutex::new(0),
                calls: calls.clone(),
            })
            .build()
            .unwrap();
        let mut target = ProtectionTarget::Live(&session);
        let decision = SafetyNetDecision::Resolve {
            on_residual: SafetyNetFallback::Redact,
        };
        let locales = &[crate::LocaleTag::Global];
        let mut report = pipeline
            .run_safety_nets(
                &mut target,
                &clean.text,
                clean.manifest.projection(),
                DocumentKind::Text,
                locales,
                None,
                decision,
            )
            .unwrap();
        pipeline
            .apply_safety_net_policy(
                &mut target,
                &mut clean,
                &mut report,
                DocumentKind::Text,
                locales,
                None,
                decision,
                None,
            )
            .unwrap();
        clean.manifest.validate().unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 8);
        for (phase, pair) in calls.chunks_exact(2).enumerate() {
            assert_eq!((pair[0].0, pair[1].0), (0, 1));
            assert_eq!((pair[0].1, pair[1].1), (phase, phase));
            assert_eq!(pair[0].2, pair[1].2);
            assert_eq!(pair[0].3, pair[1].3, "nets share the unchanged phase");
            // The last phase sees 4 spans, not 3: the redaction is a manifest entry now, so the
            // document handed to the final pass describes every byte it replaced.
            assert_eq!(
                (pair[0].4, pair[1].4),
                ([1, 2, 3, 4][phase], [1, 2, 3, 4][phase])
            );
            if phase > 0 {
                assert_ne!(pair[0].2, calls[(phase - 1) * 2].2);
                assert_ne!(
                    pair[0].3,
                    calls[(phase - 1) * 2].3,
                    "changed output invalidates projection"
                );
            }
        }
        assert!(calls[0].2.starts_with("aa"));
        assert!(calls[2].2.ends_with("bbcc"));
        assert!(calls[4].2.ends_with("cc"));
        let final_scan = crate::safety_net_scan::SafetyNetScanText::new(
            &clean.text,
            clean.manifest.projection(),
            |token| session.contains_token(token),
        )
        .unwrap();
        assert_eq!(calls[6].2, final_scan.text());
        // The `cc` tail was redacted, so restore does not bring it back; the marker standing in
        // its place survives the strict scan as ordinary text.
        assert_eq!(
            session.restore_strict_text(&clean.text).unwrap(),
            format!(
                "aaalice@example.invalidbb{}",
                gaze_types::redaction_marker::redaction_marker(&PiiClass::Name)
            )
        );
        assert_eq!(clean.manifest.projections.get(), 4);
        assert_eq!(
            clean
                .manifest
                .phases
                .iter()
                .map(|p| p.batch)
                .collect::<Vec<_>>(),
            vec![Batch::First, Batch::Second, Batch::Deletion]
        );
        for (i, phase) in clean.manifest.phases.iter().enumerate() {
            assert_eq!(Arc::as_ptr(&phase.projection) as usize, calls[i * 2].3);
        }
        assert_eq!(clean.manifest.evidence_count(), 1);
        assert_eq!(clean.manifest.segments[0].originals.as_ptr(), payloads);
        assert_eq!(clean.manifest.observations.len(), 3);
        assert!(clean.manifest.deletions.is_empty());
        // 25..27 is the redaction: a fourth manifest record standing for its own original bytes,
        // where the ledger used to hold a hole no entry described.
        assert_eq!(
            clean
                .manifest
                .records
                .iter()
                .map(|r| r.emitted.raw_span.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 2..23, 23..25, 25..27]
        );
        // Retained geometry is 1 + 2 + 3 phase entries and 4 final entries, not one copy overall.
        assert_eq!(
            clean
                .manifest
                .phases
                .iter()
                .map(|p| p.projection.spans.len())
                .sum::<usize>()
                + clean.manifest.projection().spans.len(),
            10
        );
    }

    /// A redaction is one-way, so the ledger must never accept one marked as owned: an owned entry
    /// is one restore may turn back into original bytes. Pinned against the shared rule rather than
    /// a per-origin copy, so the SafetyNetRedaction origin cannot quietly opt out of it.
    #[test]
    fn a_redaction_marker_entry_can_never_be_owned_or_a_token() {
        let (session, mut clean) = primary("aaalice@example.invalidbb", Action::Tokenize);
        let pipeline = Pipeline::builder().build().unwrap();
        let first = clean.manifest[0].clean_span.clone();
        let report = [suspect(first, LeakKind::Uncovered)];
        pipeline
            .redact_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report.iter().collect::<Vec<_>>(),
                DocumentKind::Text,
                None,
                None,
                true,
                None,
            )
            .unwrap();
        let at = clean
            .manifest
            .records
            .iter()
            .position(|r| matches!(r.origin, Origin::SafetyNetRedaction { .. }))
            .expect("a redaction record");
        clean
            .manifest
            .validate()
            .expect("the honest ledger validates");

        let mut owned = clean.manifest.clone();
        owned.records[at].owned = true;
        assert!(
            owned.validate().is_err(),
            "an owned redaction must be rejected"
        );

        let mut tokenizing = clean.manifest.clone();
        tokenizing.records[at].action = Some(Action::Tokenize);
        assert!(
            tokenizing.validate().is_err(),
            "a redaction recorded as a tokenization must be rejected"
        );
    }

    /// Builds an honest ledger carrying exactly one safety-net redaction, plus the index of that
    /// record. Every rule below is probed by mutating this ledger one field at a time, so each
    /// test names the single thing that made it invalid.
    fn ledger_with_one_redaction() -> (Session, CleanText, usize) {
        let (session, mut clean) = primary("aaalice@example.invalidbb", Action::Tokenize);
        let pipeline = Pipeline::builder().build().unwrap();
        let first = clean.manifest[0].clean_span.clone();
        let report = [suspect(first, LeakKind::Uncovered)];
        pipeline
            .redact_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report.iter().collect::<Vec<_>>(),
                DocumentKind::Text,
                None,
                None,
                true,
                None,
            )
            .unwrap();
        let at = clean
            .manifest
            .records
            .iter()
            .position(|r| matches!(r.origin, Origin::SafetyNetRedaction { .. }))
            .expect("a redaction record");
        clean
            .manifest
            .validate()
            .expect("the honest ledger validates");
        (session, clean, at)
    }

    /// A redaction that cannot say who asked for it is not auditable. Axis 4 does not allow an
    /// untraceable one-way replacement, so an observation id naming nothing must fail closed
    /// rather than validate with an empty provenance.
    #[test]
    fn a_redaction_naming_an_observation_that_does_not_exist_is_rejected() {
        let (_session, clean, at) = ledger_with_one_redaction();
        let mut forged = clean.manifest.clone();
        let Origin::SafetyNetRedaction { observations, .. } = &mut forged.records[at].origin else {
            panic!("a redaction record");
        };
        observations.push(usize::MAX);
        assert!(
            forged.validate().is_err(),
            "an observation id that names nothing must be rejected"
        );
    }

    /// The raw span is re-derived from the phase snapshot rather than trusted. A record claiming
    /// original bytes the projection does not map to is a manifest that disagrees with itself,
    /// and everything downstream -- restore, the proxy residual check, the index -- reads that
    /// span as the authority for what the marker stands for.
    #[test]
    fn a_redaction_whose_raw_span_the_projection_does_not_yield_is_rejected() {
        let (_session, clean, at) = ledger_with_one_redaction();
        let mut forged = clean.manifest.clone();
        forged.records[at].emitted.raw_span.end += 1;
        assert!(
            forged.validate().is_err(),
            "a raw span the phase projection does not yield must be rejected"
        );
    }

    /// The region must be explained by the suspects it names. A record whose observation covers
    /// bytes outside the redacted region is attributing the redaction to a suspect that did not
    /// drive it.
    #[test]
    fn a_redaction_that_does_not_cover_its_observation_is_rejected() {
        let (_session, clean, at) = ledger_with_one_redaction();
        let mut forged = clean.manifest.clone();
        let Origin::SafetyNetRedaction { observations, .. } = &forged.records[at].origin else {
            panic!("a redaction record");
        };
        let observation = observations[0];
        forged.observations[observation].suspect.span.end += 1;
        assert!(
            forged.validate().is_err(),
            "an observation reaching outside the redacted region must be rejected"
        );
    }

    /// A merged region carries the class of its lowest-offset suspect, and that class is what the
    /// marker renders and what the reader of the clean document is told was removed. A record
    /// labelled with a class no suspect asked for mislabels the redaction in the manifest while
    /// the audit row still says something else.
    #[test]
    fn a_redaction_labelled_with_a_class_no_suspect_asked_for_is_rejected() {
        let (_session, clean, at) = ledger_with_one_redaction();
        let mut forged = clean.manifest.clone();
        forged.records[at].emitted.class = PiiClass::Organization;
        assert!(
            forged.validate().is_err(),
            "a class no driving suspect asked for must be rejected"
        );
    }

    /// The region is stated in the coordinates of the phase snapshot it was taken against. One
    /// that runs past the end of that snapshot describes a document that never existed.
    #[test]
    fn a_redaction_whose_region_falls_outside_its_phase_is_rejected() {
        let (_session, clean, at) = ledger_with_one_redaction();
        let mut forged = clean.manifest.clone();
        let Origin::SafetyNetRedaction { clean: region, .. } = &mut forged.records[at].origin
        else {
            panic!("a redaction record");
        };
        region.end = usize::MAX;
        assert!(
            forged.validate().is_err(),
            "a region outside the phase snapshot must be rejected"
        );
    }

    /// `validate_terminal_manifest` lets the fallback add exactly one kind of entry nobody minted
    /// before it ran: its own marker. What keeps that from being a hole is the byte check -- the
    /// entry is admitted only if the text standing there IS the marker for its class. Honest
    /// pipeline output always passes it, so no end-to-end test can see it removed; this drives it.
    ///
    /// Mutation: drop the byte comparison and a redaction entry standing on the original bytes is
    /// admitted as though it were a marker.
    #[test]
    fn the_terminal_check_admits_a_redaction_only_over_its_marker_bytes() {
        let (session, mut clean) = primary("aaalice@example.invalidbb", Action::Tokenize);
        let pipeline = Pipeline::builder().build().unwrap();
        let target = ProtectionTarget::Live(&session);
        let before = TerminalManifestProvenance::capture(&target, &clean).unwrap();
        let first = clean.manifest[0].clean_span.clone();
        let report = [suspect(first, LeakKind::Uncovered)];
        pipeline
            .redact_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report.iter().collect::<Vec<_>>(),
                DocumentKind::Text,
                None,
                None,
                true,
                None,
            )
            .unwrap();
        validate_terminal_manifest(&target, &clean, &before).expect("honest output validates");

        // Same length, so every coordinate still lines up and only the bytes are wrong.
        let span = clean
            .manifest
            .records
            .iter()
            .find(|r| matches!(r.origin, Origin::SafetyNetRedaction { .. }))
            .expect("a redaction record")
            .emitted
            .clean_span
            .clone();
        let forged = "x".repeat(span.end - span.start);
        clean.text.replace_range(span, &forged);
        assert!(
            validate_terminal_manifest(&target, &clean, &before).is_err(),
            "a redaction entry over bytes that are not its marker must be rejected"
        );
    }

    /// Three redactions around two surviving tokens: before, between and after. The surviving
    /// tokens keep their own original bytes, and each redaction stands for exactly the original
    /// range it covered, with the markers interleaved in clean order.
    #[test]
    fn redaction_before_between_after_keeps_original_raw_relations() {
        let (session, mut clean) = primary(
            "aaalice@example.invalidbbalice@example.invalidcc",
            Action::Tokenize,
        );
        let original_ids = clean
            .manifest
            .records
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        let first = clean.manifest[0].clean_span.clone();
        let second = clean.manifest[1].clean_span.clone();
        let reports = [
            suspect(0..2, LeakKind::Uncovered),
            suspect(first.end..second.start, LeakKind::Uncovered),
            suspect(second.end..clean.text.len(), LeakKind::Uncovered),
        ];
        let pipeline = Pipeline::builder().build().unwrap();
        let mut target = ProtectionTarget::Live(&session);
        let before = TerminalManifestProvenance::capture(&target, &clean).unwrap();
        pipeline
            .redact_safety_net_suspects(
                &mut target,
                &mut clean,
                &reports.iter().collect::<Vec<_>>(),
                DocumentKind::Text,
                None,
                None,
                true,
                None,
            )
            .unwrap();
        validate_terminal_manifest(&target, &clean, &before).unwrap();
        // The two pre-existing tokens are untouched: same occurrence ids, same original bytes.
        let survivors = clean
            .manifest
            .records
            .iter()
            .filter(|r| r.action != Some(Action::Redact))
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(survivors, original_ids);

        // Three redactions, in clean order, each standing for the original range it covered and
        // none of them recorded as a deletion.
        let redactions = clean
            .manifest
            .records
            .iter()
            .filter(|r| r.action == Some(Action::Redact))
            .map(|r| r.emitted.raw_span.clone())
            .collect::<Vec<_>>();
        assert_eq!(redactions, vec![0..2, 23..25, 46..48]);
        assert!(clean.manifest.deletions.is_empty());
        assert_eq!(clean.manifest.phases.len(), 1);
        assert_eq!(clean.manifest.observations.len(), 3);

        // Markers replace, so the whole document is still describable by the affine mapper: every
        // record's clean bytes are exactly what the manifest says stands there.
        let marker = gaze_types::redaction_marker::redaction_marker(&PiiClass::Name);
        for record in clean
            .manifest
            .records
            .iter()
            .filter(|r| r.action == Some(Action::Redact))
        {
            assert_eq!(&clean.text[record.emitted.clean_span.clone()], marker);
        }
    }

    #[test]
    fn observed_segment_composition_remaps_phase_ids_and_checks_bad_ownership() {
        let (session, mut clean) = primary("aaalice@example.invalidbb", Action::Tokenize);
        let end = clean.text.len();
        resolve(
            &mut clean,
            &session,
            vec![suspect(0..end, LeakKind::PartialBleed { uncovered: 0..2 })],
            Batch::First,
        );
        let mut combined = Ledger::default();
        combined.append(clean.manifest.clone(), 3, 4).unwrap();
        combined.append(clean.manifest, 100, 120).unwrap();
        combined.validate().unwrap();
        assert_eq!(combined.observations[1].phase, 1);
        assert_eq!(combined.phases[1].raw_offset, 100);
        assert!(matches!(
            combined.records[3].origin,
            Origin::SafetyNet {
                observation: 1,
                relation: Relation::Gap,
                ..
            }
        ));
        for mode in 0..5 {
            let mut corrupt = combined.clone();
            match mode {
                0 => corrupt.records[3].owned = false,
                1 => corrupt.records[3].emitted.raw_span = 101..103,
                2 => {
                    corrupt.records[3].origin = Origin::SafetyNet {
                        observation: 1,
                        relation: Relation::WholeSuspect,
                        clean: 0..2,
                    }
                }
                3 => corrupt.observations[1].phase = 100,
                4 => corrupt.observations[1].suspect.span.end = usize::MAX,
                _ => unreachable!(),
            }
            assert!(corrupt.validate().is_err(), "mode {mode}");
        }
    }

    #[test]
    fn compatibility_projection_serializes_only_existing_geometry_fields() {
        let ledger: Ledger = vec![EmittedTokenSpan::new(1..4, 1..22, PiiClass::Email)].into();
        assert_eq!(
            serde_json::to_string(&ledger.projection().spans).unwrap(),
            r#"[{"clean_span":{"start":1,"end":4},"raw_span":{"start":1,"end":22},"class":"Email"}]"#
        );
        assert_eq!(ledger.records[0].origin, Origin::Unknown);
    }

    /// The redact path replaces a flagged region with a one-way `[REDACTED:<class>]` marker and
    /// records it as an ordinary non-owned manifest entry, so the ledger keeps NO deletion at all.
    ///
    /// That is the whole point of the change: a deletion removes clean bytes and no raw bytes,
    /// which breaks the affine clean/raw mapping from the first hole onwards. A replacement keeps
    /// it, and it keeps the redaction visible to whoever reads the clean document.
    #[test]
    fn redaction_writes_a_marker_entry_and_records_no_deletion() {
        let (session, mut clean) = primary(
            "aaalice@example.invalidbbalice@example.invalidcc",
            Action::Tokenize,
        );
        let pipeline = Pipeline::builder().build().unwrap();
        let original_id = clean.manifest.records[0].id;
        let first = clean.manifest[0].clean_span.clone();
        let report = [suspect(first, LeakKind::Uncovered)];
        pipeline
            .redact_safety_net_suspects(
                &mut ProtectionTarget::Live(&session),
                &mut clean,
                &report.iter().collect::<Vec<_>>(),
                DocumentKind::Text,
                None,
                None,
                true,
                None,
            )
            .unwrap();
        let marker = gaze_types::redaction_marker::redaction_marker(&PiiClass::Name);
        assert!(
            clean.text.contains(&marker),
            "the flagged bytes are replaced by a visible marker, never cut: {}",
            clean.text
        );
        assert!(
            clean.manifest.deletions.is_empty(),
            "a marker is a replacement, so nothing may be recorded as removed"
        );
        assert_eq!(clean.manifest.records.len(), 2);

        let redaction = clean
            .manifest
            .records
            .iter()
            .find(|r| r.action == Some(Action::Redact))
            .expect("the redaction is a manifest record");
        assert_eq!(redaction.emitted.raw_span, 2..23);
        assert_eq!(redaction.emitted.class, PiiClass::Name);
        assert!(
            !redaction.owned,
            "a one-way marker is not owned output and must never restore"
        );
        assert_eq!(&clean.text[redaction.emitted.clean_span.clone()], marker);
        let Origin::SafetyNetRedaction { observations, .. } = &redaction.origin else {
            panic!("a redaction carries the suspects that drove it");
        };
        assert_eq!(observations.len(), 1);

        // The token the redaction swallowed is gone; the untouched one still stands for its own
        // original bytes, at the coordinates the marker's width implies.
        let survivor = clean
            .manifest
            .records
            .iter()
            .find(|r| r.action != Some(Action::Redact))
            .expect("the untouched token survives");
        assert_ne!(survivor.id, original_id);
        assert_eq!(survivor.emitted.raw_span, 25..46);
        assert!(clean
            .manifest
            .records
            .iter()
            .all(|r| !r.emitted.clean_span.is_empty()));
        clean.manifest.validate().unwrap();
    }

    /// The origin-agreement check is fail-closed, so it must not be gated on an
    /// unrelated collection being non-empty. Nested inside the segment loop it
    /// never ran on a ledger holding records and no segments.
    ///
    /// The assertion is on the message, not merely on `is_err`: every known
    /// origin also fails a later check that looks the segment up, so a
    /// segment-gated guard still returns *an* error here. Only the hoisted one
    /// returns *this* error, and only the hoisted one runs first.
    #[test]
    fn the_origin_agreement_guard_runs_on_a_ledger_with_records_and_no_segments() {
        let mut ledger = Ledger::default();
        ledger.insert(Occurrence::new(
            EmittedTokenSpan::residual_fragment(0..5, 0..7, PiiClass::Email),
            Action::Tokenize,
            true,
            Origin::ExistingOwnedUnknown { segment: 0 },
        ));
        assert!(ledger.segments.is_empty());

        let error = ledger
            .validate()
            .expect_err("a record whose two origins disagree must not validate");
        assert!(
            error
                .to_string()
                .contains("emitted span origin disagrees with occurrence origin"),
            "the origin guard must be what rejects this, not a later segment lookup: {error}"
        );
    }

    /// `Origin::Unknown` means "not recorded", not "not a fragment". Every
    /// record built through `From<Vec<EmittedTokenSpan>>` carries it, so
    /// comparing the public discriminator against it would reject a
    /// legitimately emitted fragment on re-import — a mechanical hoist of the
    /// guard would have been a new rejection rather than a strengthening.
    #[test]
    fn a_fragment_reimported_through_the_span_constructor_still_validates() {
        let ledger: Ledger = vec![
            EmittedTokenSpan::new(0..5, 0..7, PiiClass::Email),
            EmittedTokenSpan::residual_fragment(6..9, 8..14, PiiClass::Email),
        ]
        .into();

        assert!(ledger.records[1].emitted.origin.is_residual_fragment());
        assert!(ledger
            .records
            .iter()
            .all(|record| matches!(record.origin, Origin::Unknown)));
        ledger
            .validate()
            .expect("a re-imported fragment is unrecorded, not a disagreement");
    }
}
