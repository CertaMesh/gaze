//! Owner-side occurrence authority. Compatibility manifests are immutable phase projections.
use super::*;
use std::cell::OnceCell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Selection {
        segment: usize,
        selection: usize,
    },
    SafetyNet {
        observation: usize,
        relation: Relation,
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
    pub(super) fn selection_for(&self, raw: &Range<usize>, recovered: bool) -> Result<usize> {
        self.segments[0]
            .selections
            .iter()
            .position(|s| s.raw == *raw && s.recovered == recovered)
            .ok_or_else(|| manifest_integrity_error("missing selection evidence"))
    }
    pub(super) fn set_selection_action(&mut self, selection: usize, action: Action) {
        self.segments[0].selections[selection].action = Some(action);
    }
    #[cfg(test)]
    pub(super) fn records(&self) -> &[Occurrence] {
        &self.records
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
    pub(super) fn into_spans(self) -> Vec<EmittedTokenSpan> {
        self.records.into_iter().map(|r| r.emitted).collect()
    }
    pub(super) fn insert(&mut self, mut record: Occurrence) {
        self.projection.take();
        record.id = self.next_id;
        self.next_id += 1;
        self.records.push(record);
        self.records.sort_by_key(|r| r.emitted.clean_span.start);
    }
    pub(super) fn phase(&mut self, text_len: usize, batch: Batch) -> usize {
        let projection = Arc::clone(self.project_arc());
        let id = self.phases.len();
        self.phases.push(Phase {
            text_len,
            projection,
            batch,
            raw_offset: 0,
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
        for segment in &self.segments {
            if segment.originals.len() != segment.original_raw.len() {
                return Err(manifest_integrity_error("evidence length mismatch"));
            }
            for selection in &segment.selections {
                if selection.members.is_empty()
                    || selection
                        .members
                        .iter()
                        .any(|id| *id >= segment.originals.len())
                {
                    return Err(manifest_integrity_error("invalid selection members"));
                }
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
                Origin::Selection { segment, selection } => {
                    let segment = self
                        .segments
                        .get(*segment)
                        .ok_or_else(|| manifest_integrity_error("invalid segment id"))?;
                    let selected = segment
                        .selections
                        .get(*selection)
                        .ok_or_else(|| manifest_integrity_error("invalid selection id"))?;
                    if selected.action != record.action
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
                    let start = map_clean_boundary_to_raw(&phase.projection.spans, clean.start)
                        .and_then(|v| v.checked_add(phase.raw_offset));
                    let end = map_clean_boundary_to_raw(&phase.projection.spans, clean.end)
                        .and_then(|v| v.checked_add(phase.raw_offset));
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
        Origin::Selection { segment: id, .. } | Origin::ExistingOwnedUnknown { segment: id } => {
            *id += segment
        }
        Origin::SafetyNet {
            observation: id, ..
        } => *id += observation,
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
