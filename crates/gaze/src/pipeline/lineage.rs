//! Typed provenance for protection-trace items (component ledger, slice S1).
//!
//! A trace item's `source_ids` is lineage: defeated candidates and derived
//! dependencies ride on the winner's id list, so it cannot say which
//! recognizer independently found which bytes. This module rebuilds that from
//! the resolver's decision graph (`ResolutionEvent`), which already records
//! every pair verdict and collateral removal by node id. One walk of that
//! graph yields both views: the typed roles on each trace item, and one
//! [`CandidateEvent`] per original candidate, keyed by its index and span. It
//! reads the graph; it never changes a decision, so clean output and manifest
//! are unaffected.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use super::occurrence::{Segment, Selection};
use crate::resolver::{PairOutcome, ResolutionEvent};
use crate::{ConflictTier, PiiClass};
use gaze_types::ValidatorFailReason;

/// How a recognizer contributed to one protection-trace item.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContributionRole {
    /// Defined the emitted span and class.
    Winner,
    /// Found exactly the winner's span and class; merged into it.
    SameSpanMerge,
    /// Independently found its own span, which the item covers in part or in
    /// whole: a collision-family precedence-tie side, a residual-cell parent,
    /// or one of several safety-net suspects in one redacted region.
    CoMember,
    /// Overlapped the item and lost; see [`Defeat`].
    Defeated,
    /// Named in a candidate's lineage without detecting these bytes itself
    /// (the NER street span that licenses a house number).
    DerivedDependency,
}

impl ContributionRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Winner => "winner",
            Self::SameSpanMerge => "same_span_merge",
            Self::CoMember => "co_member",
            Self::Defeated => "defeated",
            Self::DerivedDependency => "derived_dependency",
        }
    }
}

/// How a defeated candidate left resolution.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefeatKind {
    /// Lost a pair arbitration: its tier is the rung that decided that pair.
    Pair,
    /// Removed because a pair winner grew over it. No arbitration ran against
    /// it; its tier is the rung the replacing winner had just won on.
    Collateral,
}

impl DefeatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Collateral => "collateral",
        }
    }
}

/// The one event that took a candidate's line out of resolution: the
/// innermost defeat on its path, not a later loss of the line that beat it.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defeat {
    pub kind: DefeatKind,
    pub tier: ConflictTier,
    /// Original index of the winning side's representative candidate.
    pub winner: usize,
    /// That candidate's recognizer ID.
    pub winner_recognizer: String,
}

/// One recognizer's part in a protection-trace item. Metadata only.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContribution {
    recognizer_id: String,
    role: ContributionRole,
    /// The contributing candidate's own span in the original text (for a
    /// safety-net suspect: the bytes its own action covered). `None` only for
    /// a derived dependency.
    raw_span: Option<Range<usize>>,
    defeat: Option<Defeat>,
}

impl TraceContribution {
    pub(super) fn new(
        recognizer_id: impl Into<String>,
        role: ContributionRole,
        raw_span: Option<Range<usize>>,
        defeat: Option<Defeat>,
    ) -> Self {
        Self {
            recognizer_id: recognizer_id.into(),
            role,
            raw_span,
            defeat,
        }
    }

    pub fn recognizer_id(&self) -> &str {
        &self.recognizer_id
    }

    pub fn role(&self) -> ContributionRole {
        self.role
    }

    pub fn raw_span(&self) -> Option<Range<usize>> {
        self.raw_span.clone()
    }

    /// Present exactly on a defeated contribution.
    pub fn defeat(&self) -> Option<&Defeat> {
        self.defeat.as_ref()
    }
}

/// Which step of the pipeline produced a trace item.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSettlement {
    /// Primary resolution of the candidate pool.
    Resolve,
    /// Recovery resolution of a gap left by primary resolution.
    Recovery,
    /// Repeat-value sweep copy.
    Sweep,
    /// Collision-family precedence tie: a family-level token over both sides.
    CollisionTie,
    /// Missing-anchor fallback to a family-level token.
    AnchorFallback,
    /// Residual cell: bytes of a losing candidate no winner covered.
    Residual,
    /// Safety-net finding.
    SafetyNet,
}

impl TraceSettlement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Recovery => "recovery",
            Self::Sweep => "sweep",
            Self::CollisionTie => "collision_tie",
            Self::AnchorFallback => "anchor_fallback",
            Self::Residual => "residual",
            Self::SafetyNet => "safety_net",
        }
    }
}

/// What happened to one detected candidate. Metadata only: IDs, classes,
/// byte offsets and closed-set reasons, never a value.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEvent {
    /// Index in the resolver's candidate pool; `None` for a candidate the
    /// validator vetoed before the pool was built.
    pub original: Option<usize>,
    pub recognizer_id: String,
    pub class: PiiClass,
    /// The candidate's own span in the original text.
    pub raw_span: Range<usize>,
    pub outcome: CandidateOutcome,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CandidateOutcome {
    /// Part of a selection's winning line.
    Selected {
        selection: Range<usize>,
        settlement: TraceSettlement,
        role: ContributionRole,
    },
    /// Lost inside a selection's decision tree.
    Defeated {
        selection: Range<usize>,
        settlement: TraceSettlement,
        defeat: Defeat,
    },
    /// Vetoed by its validator before resolution.
    Vetoed { reason: ValidatorFailReason },
    /// In the pool but in no selection's decision tree.
    Unlinked,
}

/// A trace item's typed provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Lineage {
    pub(super) settlement: TraceSettlement,
    pub(super) contributions: Vec<TraceContribution>,
}

#[cfg(test)]
impl Lineage {
    /// A single-winner lineage for collector-level tests.
    pub(super) fn fixture(recognizer_id: &str) -> Self {
        Self {
            settlement: TraceSettlement::Resolve,
            contributions: vec![TraceContribution::new(
                recognizer_id,
                ContributionRole::Winner,
                Some(0..1),
                None,
            )],
        }
    }
}

/// Node-indexed view of one segment's decision graph.
pub(super) struct DecisionGraph<'a> {
    segment: &'a Segment,
    pairs: BTreeMap<usize, (usize, usize, &'a PairOutcome)>,
    collateral: BTreeMap<usize, Vec<(usize, ConflictTier)>>,
}

/// One selection's lineage plus the per-candidate outcomes behind it.
pub(super) struct SelectionLineage {
    pub(super) lineage: Lineage,
    pub(super) events: Vec<CandidateEvent>,
}

impl<'a> DecisionGraph<'a> {
    pub(super) fn new(segment: &'a Segment) -> Self {
        let mut pairs = BTreeMap::new();
        let mut collateral: BTreeMap<usize, Vec<(usize, ConflictTier)>> = BTreeMap::new();
        for event in segment.events.iter() {
            match event {
                ResolutionEvent::Pair {
                    existing,
                    incoming,
                    result,
                    outcome,
                } => {
                    pairs.insert(*result, (*existing, *incoming, outcome));
                }
                ResolutionEvent::Collateral {
                    removed,
                    replacing,
                    tier,
                } => collateral
                    .entry(*replacing)
                    .or_default()
                    .push((*removed, *tier)),
                ResolutionEvent::Recovery { .. } => {}
            }
        }
        Self {
            segment,
            pairs,
            collateral,
        }
    }

    /// The original whose fields a node carries: the first member of its
    /// winning line (existing first on a merge or tie, as the resolver keeps it).
    fn representative(&self, mut node: usize) -> Result<usize, &'static str> {
        while node >= self.segment.originals.len() {
            let &(existing, incoming, outcome) = self
                .pairs
                .get(&node)
                .ok_or("decision node without an event")?;
            node = match outcome {
                PairOutcome::Incoming(_) => incoming,
                _ => existing,
            };
        }
        Ok(node)
    }

    fn defeat(
        &self,
        kind: DefeatKind,
        tier: ConflictTier,
        winner_node: usize,
    ) -> Result<Defeat, &'static str> {
        let winner = self.representative(winner_node)?;
        Ok(Defeat {
            kind,
            tier,
            winner,
            winner_recognizer: self.segment.originals[winner].recognizer_id.clone(),
        })
    }

    /// Lineage of one primary or recovered selection.
    ///
    /// Fails when the graph's structural leaves disagree with the selection's
    /// members: the lineage would then be a guess, and a guess is worse than
    /// no trace.
    pub(super) fn selection(
        &self,
        selection: &Selection,
    ) -> Result<SelectionLineage, &'static str> {
        let originals = self.segment.originals.len();
        let mut structural = Vec::new();
        let mut defeated = BTreeMap::<usize, Defeat>::new();
        let mut tie = false;
        // (node, the innermost defeat seen so far on the path from the selection)
        let mut stack = vec![(selection.node, None::<Defeat>)];
        while let Some((node, lost)) = stack.pop() {
            if let Some(removed) = self.collateral.get(&node) {
                for &(removed, tier) in removed {
                    let defeat = self.defeat(DefeatKind::Collateral, tier, node)?;
                    stack.push((removed, Some(defeat)));
                }
            }
            if node < originals {
                match lost {
                    Some(defeat) => {
                        if defeated.insert(node, defeat).is_some() {
                            return Err("candidate defeated twice in one selection");
                        }
                    }
                    None => structural.push(node),
                }
                continue;
            }
            let Some(&(existing, incoming, outcome)) = self.pairs.get(&node) else {
                return Err("decision node without an event");
            };
            match outcome {
                PairOutcome::Merge => {
                    stack.push((existing, lost.clone()));
                    stack.push((incoming, lost));
                }
                PairOutcome::Family => {
                    tie |= lost.is_none();
                    stack.push((existing, lost.clone()));
                    stack.push((incoming, lost));
                }
                PairOutcome::Incoming(tier) => {
                    let defeat = self.defeat(DefeatKind::Pair, *tier, incoming)?;
                    stack.push((incoming, lost));
                    stack.push((existing, Some(defeat)));
                }
                PairOutcome::Existing(tier) => {
                    let defeat = self.defeat(DefeatKind::Pair, *tier, existing)?;
                    stack.push((existing, lost));
                    stack.push((incoming, Some(defeat)));
                }
            }
        }
        let leaves = structural.iter().copied().collect::<BTreeSet<_>>();
        let members = selection.members.iter().copied().collect::<BTreeSet<_>>();
        if leaves != members || leaves.len() != structural.len() {
            return Err("decision graph disagrees with selection members");
        }

        let first = &self.segment.originals[selection.members[0]];
        let settlement = if first.recognizer_id == crate::sweep::SWEEP_ID {
            TraceSettlement::Sweep
        } else if tie {
            TraceSettlement::CollisionTie
        } else if selection.class.as_family_name().is_some() {
            // A family-level token without a precedence tie is the
            // missing-anchor fallback.
            TraceSettlement::AnchorFallback
        } else if selection.recovered {
            TraceSettlement::Recovery
        } else {
            TraceSettlement::Resolve
        };

        let raw = &self.segment.original_raw;
        let mut contributions = Vec::new();
        let mut events = Vec::new();
        for (index, &id) in selection.members.iter().enumerate() {
            let role = if tie || raw[id] != selection.raw {
                ContributionRole::CoMember
            } else if index == 0 {
                ContributionRole::Winner
            } else {
                ContributionRole::SameSpanMerge
            };
            push_original(self.segment, &mut contributions, id, role, None);
            events.push(self.event(
                id,
                CandidateOutcome::Selected {
                    selection: selection.raw.clone(),
                    settlement,
                    role,
                },
            ));
        }
        for (id, defeat) in defeated {
            events.push(self.event(
                id,
                CandidateOutcome::Defeated {
                    selection: selection.raw.clone(),
                    settlement,
                    defeat: defeat.clone(),
                },
            ));
            push_original(
                self.segment,
                &mut contributions,
                id,
                ContributionRole::Defeated,
                Some(defeat),
            );
        }
        Ok(SelectionLineage {
            lineage: Lineage {
                settlement,
                contributions: finish(contributions),
            },
            events,
        })
    }

    fn event(&self, id: usize, outcome: CandidateOutcome) -> CandidateEvent {
        let original = &self.segment.originals[id];
        CandidateEvent {
            original: Some(id),
            recognizer_id: original.recognizer_id.clone(),
            class: original.class.clone(),
            raw_span: self.segment.original_raw[id].clone(),
            outcome,
        }
    }

    /// Every pool candidate's placements, in pool order. A candidate appears
    /// once per selection tree that reached it: it can lose inside one tree,
    /// see that winner's line lose too, and then be recovered in the gap left
    /// behind. It is selected at most once; a candidate no tree reached is
    /// `Unlinked`.
    pub(super) fn candidate_events(
        &self,
        per_selection: impl IntoIterator<Item = Vec<CandidateEvent>>,
    ) -> Result<Vec<CandidateEvent>, &'static str> {
        let mut slots: Vec<Vec<CandidateEvent>> = vec![Vec::new(); self.segment.originals.len()];
        for event in per_selection.into_iter().flatten() {
            let id = event.original.ok_or("pool event without an original")?;
            slots[id].push(event);
        }
        let mut events = Vec::new();
        for (id, mut placements) in slots.into_iter().enumerate() {
            if placements.is_empty() {
                events.push(self.event(id, CandidateOutcome::Unlinked));
                continue;
            }
            let selected = placements
                .iter()
                .filter(|event| matches!(event.outcome, CandidateOutcome::Selected { .. }))
                .count();
            if selected > 1 {
                return Err("candidate selected twice");
            }
            placements.sort_by_key(|event| match &event.outcome {
                CandidateOutcome::Selected { selection, .. }
                | CandidateOutcome::Defeated { selection, .. } => (selection.start, selection.end),
                _ => (0, 0),
            });
            if placements
                .windows(2)
                .any(|pair| selection_of(&pair[0]) == selection_of(&pair[1]))
            {
                return Err("candidate placed twice in one selection");
            }
            events.extend(placements);
        }
        Ok(events)
    }
}

fn selection_of(event: &CandidateEvent) -> Option<&Range<usize>> {
    match &event.outcome {
        CandidateOutcome::Selected { selection, .. }
        | CandidateOutcome::Defeated { selection, .. } => Some(selection),
        _ => None,
    }
}

/// Event of a candidate the validator vetoed before resolution.
pub(super) fn vetoed_event(vetoed: &crate::validator_veto::VetoedCandidate) -> CandidateEvent {
    CandidateEvent {
        original: None,
        recognizer_id: vetoed.candidate.recognizer_id.clone(),
        class: vetoed.candidate.class.clone(),
        raw_span: vetoed.candidate.span.clone(),
        outcome: CandidateOutcome::Vetoed {
            reason: vetoed.reason,
        },
    }
}

/// Lineage of one residual cell: the representative parent is the winner,
/// every other parent a co-member on its own span.
pub(super) fn residual(segment: &Segment, parents: &[usize], representative: usize) -> Lineage {
    let mut contributions = Vec::new();
    for &id in parents {
        let role = if id == representative {
            ContributionRole::Winner
        } else {
            ContributionRole::CoMember
        };
        push_original(segment, &mut contributions, id, role, None);
    }
    Lineage {
        settlement: TraceSettlement::Residual,
        contributions: finish(contributions),
    }
}

fn push_original(
    segment: &Segment,
    out: &mut Vec<TraceContribution>,
    id: usize,
    role: ContributionRole,
    defeat: Option<Defeat>,
) {
    let original = &segment.originals[id];
    out.push(TraceContribution::new(
        original.recognizer_id.clone(),
        role,
        Some(segment.original_raw[id].clone()),
        defeat,
    ));
    for dependency in &original.source_recognizer_ids {
        if dependency != &original.recognizer_id {
            out.push(TraceContribution::new(
                dependency.clone(),
                ContributionRole::DerivedDependency,
                None,
                None,
            ));
        }
    }
}

/// Lineage of a safety-net item: each suspect with the raw bytes its own
/// action covered. One suspect is the winner; several merged into one
/// redaction region are co-members, so each backend keeps its own bytes.
pub(super) fn safety_net(suspects: &[(String, Range<usize>)]) -> Lineage {
    let role = if suspects.len() == 1 {
        ContributionRole::Winner
    } else {
        ContributionRole::CoMember
    };
    Lineage {
        settlement: TraceSettlement::SafetyNet,
        contributions: finish(
            suspects
                .iter()
                .map(|(id, span)| {
                    TraceContribution::new(id.clone(), role, Some(span.clone()), None)
                })
                .collect(),
        ),
    }
}

/// Stable order. Only exact duplicate derived dependencies collapse: every
/// detecting or defeated entry stands for its own candidate and keeps its
/// multiplicity.
fn finish(mut contributions: Vec<TraceContribution>) -> Vec<TraceContribution> {
    contributions.sort_by_key(|item| {
        (
            item.recognizer_id.clone(),
            item.role,
            item.raw_span.as_ref().map(|span| (span.start, span.end)),
        )
    });
    contributions.dedup_by(|later, earlier| {
        later.role == ContributionRole::DerivedDependency && later == earlier
    });
    contributions
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::pipeline::occurrence::Basis;
    use crate::Candidate;

    fn candidate(span: Range<usize>, class: PiiClass, id: &str) -> Candidate {
        Candidate::new(
            span,
            class,
            id,
            0.9,
            0,
            None,
            "fixture",
            id,
            ConflictTier::None,
            Vec::new(),
        )
    }

    fn segment(originals: Vec<Candidate>, events: Vec<ResolutionEvent>) -> Segment {
        Segment {
            original_raw: originals.iter().map(|c| c.span.clone()).collect(),
            originals,
            selections: Vec::new(),
            residuals: Vec::new(),
            residual_order: Vec::new(),
            residual_admitted: Vec::new(),
            events: Arc::from(events),
            raw_offset: 0,
            clean_offset: 0,
            basis: Basis::OriginalInput,
        }
    }

    fn selection(
        node: usize,
        members: Vec<usize>,
        raw: Range<usize>,
        class: PiiClass,
    ) -> Selection {
        Selection {
            node,
            class,
            members,
            raw,
            recovered: false,
            action: None,
        }
    }

    type Role<'a> = (
        &'a str,
        ContributionRole,
        Option<Range<usize>>,
        Option<(DefeatKind, ConflictTier, &'a str)>,
    );

    fn roles(lineage: &Lineage) -> Vec<Role<'_>> {
        lineage
            .contributions
            .iter()
            .map(|c| {
                (
                    c.recognizer_id(),
                    c.role(),
                    c.raw_span(),
                    c.defeat()
                        .map(|d| (d.kind, d.tier, d.winner_recognizer.as_str())),
                )
            })
            .collect()
    }

    fn collateral_segment() -> Segment {
        // The event shape `resolver::collateral_removal_has_no_fabricated_pair_outcome_or_membership` pins.
        segment(
            vec![
                candidate(0..5, PiiClass::Name, "a"),
                candidate(10..15, PiiClass::Name, "b"),
                candidate(3..12, PiiClass::Email, "c"),
            ],
            vec![
                ResolutionEvent::Pair {
                    existing: 0,
                    incoming: 2,
                    result: 3,
                    outcome: PairOutcome::Incoming(ConflictTier::ClassPriority),
                },
                ResolutionEvent::Collateral {
                    removed: 1,
                    replacing: 3,
                    tier: ConflictTier::ClassPriority,
                },
            ],
        )
    }

    #[test]
    fn collateral_removal_is_its_own_defeat_kind() {
        let segment = collateral_segment();
        let graph = DecisionGraph::new(&segment);
        let lineage = graph
            .selection(&selection(3, vec![2], 3..12, PiiClass::Email))
            .expect("lineage");
        assert_eq!(lineage.lineage.settlement, TraceSettlement::Resolve);
        assert_eq!(
            roles(&lineage.lineage),
            vec![
                (
                    "a",
                    ContributionRole::Defeated,
                    Some(0..5),
                    Some((DefeatKind::Pair, ConflictTier::ClassPriority, "c"))
                ),
                (
                    "b",
                    ContributionRole::Defeated,
                    Some(10..15),
                    Some((DefeatKind::Collateral, ConflictTier::ClassPriority, "c"))
                ),
                ("c", ContributionRole::Winner, Some(3..12), None),
            ]
        );
    }

    #[test]
    fn a_nested_loser_keeps_the_rung_it_lost_on_and_who_beat_it() {
        // x lost to y on score; y's line then lost to z on class priority.
        let segment = segment(
            vec![
                candidate(0..4, PiiClass::Name, "x"),
                candidate(0..6, PiiClass::Name, "y"),
                candidate(0..8, PiiClass::Email, "z"),
            ],
            vec![
                ResolutionEvent::Pair {
                    existing: 1,
                    incoming: 0,
                    result: 3,
                    outcome: PairOutcome::Existing(ConflictTier::Score),
                },
                ResolutionEvent::Pair {
                    existing: 3,
                    incoming: 2,
                    result: 4,
                    outcome: PairOutcome::Incoming(ConflictTier::ClassPriority),
                },
            ],
        );
        let lineage = DecisionGraph::new(&segment)
            .selection(&selection(4, vec![2], 0..8, PiiClass::Email))
            .expect("lineage");
        assert_eq!(
            roles(&lineage.lineage),
            vec![
                (
                    "x",
                    ContributionRole::Defeated,
                    Some(0..4),
                    Some((DefeatKind::Pair, ConflictTier::Score, "y"))
                ),
                (
                    "y",
                    ContributionRole::Defeated,
                    Some(0..6),
                    Some((DefeatKind::Pair, ConflictTier::ClassPriority, "z"))
                ),
                ("z", ContributionRole::Winner, Some(0..8), None),
            ]
        );
    }

    #[test]
    fn precedence_tie_sides_are_co_members_on_their_own_spans() {
        let family = PiiClass::family("identifier");
        let segment = segment(
            vec![
                candidate(0..6, PiiClass::Custom("tax".into()), "tax.rule"),
                candidate(2..9, PiiClass::Custom("id".into()), "id.rule"),
            ],
            vec![ResolutionEvent::Pair {
                existing: 0,
                incoming: 1,
                result: 2,
                outcome: PairOutcome::Family,
            }],
        );
        let lineage = DecisionGraph::new(&segment)
            .selection(&selection(2, vec![0, 1], 0..9, family))
            .expect("lineage");
        assert_eq!(lineage.lineage.settlement, TraceSettlement::CollisionTie);
        assert_eq!(
            roles(&lineage.lineage),
            vec![
                ("id.rule", ContributionRole::CoMember, Some(2..9), None),
                ("tax.rule", ContributionRole::CoMember, Some(0..6), None),
            ]
        );
    }

    #[test]
    fn a_family_token_without_a_tie_is_the_anchor_fallback() {
        let segment = segment(
            vec![candidate(0..6, PiiClass::Custom("tax".into()), "tax.rule")],
            vec![],
        );
        let lineage = DecisionGraph::new(&segment)
            .selection(&selection(0, vec![0], 0..6, PiiClass::family("identifier")))
            .expect("lineage");
        assert_eq!(lineage.lineage.settlement, TraceSettlement::AnchorFallback);
    }

    #[test]
    fn sweep_and_recovery_settlements() {
        let segment = segment(
            vec![
                candidate(0..6, PiiClass::Email, crate::sweep::SWEEP_ID),
                candidate(8..12, PiiClass::Email, "email.rule"),
            ],
            vec![],
        );
        let graph = DecisionGraph::new(&segment);
        let swept = graph
            .selection(&selection(0, vec![0], 0..6, PiiClass::Email))
            .expect("lineage");
        assert_eq!(swept.lineage.settlement, TraceSettlement::Sweep);
        let mut gap = selection(1, vec![1], 8..12, PiiClass::Email);
        gap.recovered = true;
        let recovered = graph.selection(&gap).expect("lineage");
        assert_eq!(recovered.lineage.settlement, TraceSettlement::Recovery);
        assert!(matches!(
            recovered.events[0].outcome,
            CandidateOutcome::Selected {
                settlement: TraceSettlement::Recovery,
                role: ContributionRole::Winner,
                ..
            }
        ));
    }

    #[test]
    fn a_licensing_recognizer_is_a_derived_dependency_without_a_span() {
        let mut number = candidate(
            10..12,
            PiiClass::Location,
            crate::HOUSE_NUMBER_RECOGNIZER_ID,
        );
        number
            .source_recognizer_ids
            .push(crate::NER_RECOGNIZER_ID.to_string());
        let segment = segment(vec![number], vec![]);
        let lineage = DecisionGraph::new(&segment)
            .selection(&selection(0, vec![0], 10..12, PiiClass::Location))
            .expect("lineage");
        assert_eq!(
            roles(&lineage.lineage),
            vec![
                (
                    crate::HOUSE_NUMBER_RECOGNIZER_ID,
                    ContributionRole::Winner,
                    Some(10..12),
                    None
                ),
                (
                    crate::NER_RECOGNIZER_ID,
                    ContributionRole::DerivedDependency,
                    None,
                    None
                ),
            ]
        );
    }

    #[test]
    fn a_graph_that_disagrees_with_the_members_is_refused() {
        let segment = segment(
            vec![
                candidate(0..5, PiiClass::Email, "a"),
                candidate(0..5, PiiClass::Email, "b"),
            ],
            vec![ResolutionEvent::Pair {
                existing: 0,
                incoming: 1,
                result: 2,
                outcome: PairOutcome::Merge,
            }],
        );
        let graph = DecisionGraph::new(&segment);
        assert!(graph
            .selection(&selection(2, vec![0], 0..5, PiiClass::Email))
            .is_err());
        assert!(graph
            .selection(&selection(9, vec![0], 0..5, PiiClass::Email))
            .is_err());
        let merged = graph
            .selection(&selection(2, vec![0, 1], 0..5, PiiClass::Email))
            .expect("lineage");
        assert_eq!(
            roles(&merged.lineage),
            vec![
                ("a", ContributionRole::Winner, Some(0..5), None),
                ("b", ContributionRole::SameSpanMerge, Some(0..5), None),
            ]
        );
    }

    #[test]
    fn candidate_events_cover_every_original_and_keep_every_placement() {
        let segment = collateral_segment();
        let graph = DecisionGraph::new(&segment);
        let selected = graph
            .selection(&selection(3, vec![2], 3..12, PiiClass::Email))
            .expect("lineage");
        let events = graph
            .candidate_events([selected.events.clone()])
            .expect("events");
        assert_eq!(
            events.iter().map(|e| e.original).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
        assert!(matches!(
            &events[1].outcome,
            CandidateOutcome::Defeated { defeat, selection, .. }
                if defeat.kind == DefeatKind::Collateral && defeat.winner == 2 && *selection == (3..12)
        ));
        assert!(matches!(
            events[2].outcome,
            CandidateOutcome::Selected {
                role: ContributionRole::Winner,
                ..
            }
        ));
        // Nothing reached: every candidate is Unlinked, never dropped.
        let none = graph
            .candidate_events(Vec::<Vec<CandidateEvent>>::new())
            .expect("events");
        assert!(none.iter().all(|e| e.outcome == CandidateOutcome::Unlinked));
        // Selected twice, or placed twice in one selection, is a contradiction.
        assert!(graph
            .candidate_events([selected.events.clone(), selected.events.clone()])
            .is_err());
        // Defeated in one tree, then recovered in another: both placements kept.
        let recovered = CandidateEvent {
            outcome: CandidateOutcome::Selected {
                selection: 0..5,
                settlement: TraceSettlement::Recovery,
                role: ContributionRole::Winner,
            },
            ..events[0].clone()
        };
        let both = graph
            .candidate_events([selected.events, vec![recovered]])
            .expect("events");
        assert_eq!(
            both.iter().filter(|e| e.original == Some(0)).count(),
            2,
            "a recovered loser keeps its defeat and its selection"
        );
    }

    #[test]
    fn residual_parents_are_winner_and_co_members() {
        let segment = segment(
            vec![
                candidate(0..8, PiiClass::Name, "rep"),
                candidate(2..10, PiiClass::Name, "other"),
            ],
            vec![],
        );
        let lineage = residual(&segment, &[0, 1], 0);
        assert_eq!(lineage.settlement, TraceSettlement::Residual);
        assert_eq!(
            roles(&lineage),
            vec![
                ("other", ContributionRole::CoMember, Some(2..10), None),
                ("rep", ContributionRole::Winner, Some(0..8), None),
            ]
        );
    }

    #[test]
    fn merged_safety_net_suspects_keep_their_own_spans_and_multiplicity() {
        let one = safety_net(&[("nym".to_string(), 3..9)]);
        assert_eq!(
            roles(&one),
            vec![("nym", ContributionRole::Winner, Some(3..9), None)]
        );
        // Two suspects from one backend on disjoint bytes stay two entries.
        let many = safety_net(&[
            ("nym".to_string(), 3..6),
            ("nym".to_string(), 5..9),
            ("opf".to_string(), 4..7),
        ]);
        assert_eq!(
            roles(&many),
            vec![
                ("nym", ContributionRole::CoMember, Some(3..6), None),
                ("nym", ContributionRole::CoMember, Some(5..9), None),
                ("opf", ContributionRole::CoMember, Some(4..7), None),
            ]
        );
    }
}
