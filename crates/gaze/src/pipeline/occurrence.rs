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
    pub(super) fn set_selection_action(&mut self, selection: usize, action: Action) {
        self.segments[0].selections[selection].action = Some(action);
    }
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
        for segment in &self.segments {
            if segment.originals.len() != segment.original_raw.len() {
                return Err(manifest_integrity_error("evidence length mismatch"));
            }
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
            Batch::Second => {
                let FollowupResolution::Ready(plan) =
                    plan_followup_resolutions(&target, clean, &report, None).unwrap()
                else {
                    panic!("ready second batch")
                };
                pipeline
                    .apply_followup_resolutions(
                        &mut target,
                        clean,
                        plan,
                        DocumentKind::Text,
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
                        0 => vec![suspect(0..2, LeakKind::Uncovered)],
                        1 => vec![suspect(text.len() - 4..text.len() - 2, LeakKind::Uncovered)],
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
            assert_eq!(
                (pair[0].4, pair[1].4),
                ([1, 2, 3, 3][phase], [1, 2, 3, 3][phase])
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
        assert_eq!(calls[6].2, clean.text);
        assert_eq!(
            session.restore_strict_text(&clean.text).unwrap(),
            "aaalice@example.invalidbb"
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
        assert_eq!(clean.manifest.deletions.len(), 1);
        assert_eq!(clean.manifest.deletions[0].raw, Some(25..27));
        assert!(clean.manifest.deletions[0].removed.is_empty());
        assert_eq!(
            clean
                .manifest
                .records
                .iter()
                .map(|r| r.emitted.raw_span.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 2..23, 23..25]
        );
        // Retained geometry is 1 + 2 + 3 phase entries and 3 final entries, not one copy overall.
        assert_eq!(
            clean
                .manifest
                .phases
                .iter()
                .map(|p| p.projection.spans.len())
                .sum::<usize>()
                + clean.manifest.projection().spans.len(),
            9
        );
    }

    #[test]
    fn deletion_before_between_after_keeps_original_raw_relations() {
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
        assert_eq!(
            clean
                .manifest
                .records
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            original_ids
        );
        assert_eq!(
            clean
                .manifest
                .deletions
                .iter()
                .map(|d| d.raw.clone().unwrap())
                .collect::<Vec<_>>(),
            vec![46..48, 23..25, 0..2]
        );
        assert_eq!(clean.manifest.phases.len(), 1);
        assert_eq!(clean.manifest.observations.len(), 3);
        assert!(clean
            .manifest
            .deletions
            .iter()
            .all(|d| d.removed.is_empty()));
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

    #[test]
    fn deletion_retains_disposition_and_never_an_empty_emission() {
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
        assert_eq!(clean.manifest.records.len(), 1);
        assert_eq!(clean.manifest.deletions.len(), 1);
        assert_eq!(clean.manifest.deletions[0].removed[0].id, original_id);
        assert_eq!(clean.manifest.deletions[0].raw, Some(2..23));
        assert_eq!(clean.manifest.deletions[0].observations.len(), 1);
        assert_eq!(clean.manifest.records[0].emitted.raw_span, 25..46);
        assert!(clean
            .manifest
            .records
            .iter()
            .all(|r| !r.emitted.clean_span.is_empty()));
        clean.manifest.validate().unwrap();
    }
}
