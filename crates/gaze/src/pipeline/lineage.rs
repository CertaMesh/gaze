//! Typed provenance for protection-trace items (component ledger, slice S1).
//!
//! A trace item's `source_ids` is lineage: defeated candidates and derived
//! dependencies ride on the winner's id list, so it cannot say which
//! recognizer independently found which bytes. This module rebuilds that from
//! the resolver's decision graph (`ResolutionEvent`), which already records
//! every pair verdict and collateral removal by node id. It reads the graph;
//! it never changes a decision, so clean output and manifest are unaffected.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use super::occurrence::{Segment, Selection};
use crate::resolver::{PairOutcome, ResolutionEvent};
use crate::ConflictTier;

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
    /// Overlapped the item and lost resolution on `tier`.
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

/// One recognizer's part in a protection-trace item. Metadata only.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContribution {
    recognizer_id: String,
    role: ContributionRole,
    /// The contributing candidate's own span in the original text; `None`
    /// when it has none here (a derived dependency, or a suspect merged into
    /// a multi-suspect redaction region).
    raw_span: Option<Range<usize>>,
    /// The rung a defeated candidate lost on.
    tier: Option<ConflictTier>,
}

impl TraceContribution {
    pub(super) fn new(
        recognizer_id: impl Into<String>,
        role: ContributionRole,
        raw_span: Option<Range<usize>>,
        tier: Option<ConflictTier>,
    ) -> Self {
        Self {
            recognizer_id: recognizer_id.into(),
            role,
            raw_span,
            tier,
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

    pub fn tier(&self) -> Option<ConflictTier> {
        self.tier
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
                None,
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

    /// Lineage of one primary or recovered selection.
    ///
    /// Fails when the graph's structural leaves disagree with the selection's
    /// members: the lineage would then be a guess, and a guess is worse than
    /// no trace.
    pub(super) fn selection(&self, selection: &Selection) -> Result<Lineage, &'static str> {
        let originals = self.segment.originals.len();
        let mut structural = Vec::new();
        let mut defeated = BTreeMap::<usize, ConflictTier>::new();
        let mut tie = false;
        // (node, first defeat on the path from the selection)
        let mut stack = vec![(selection.node, None::<ConflictTier>)];
        while let Some((node, lost)) = stack.pop() {
            if let Some(removed) = self.collateral.get(&node) {
                for &(removed, tier) in removed {
                    stack.push((removed, Some(lost.unwrap_or(tier))));
                }
            }
            if node < originals {
                match lost {
                    Some(tier) => {
                        defeated.entry(node).or_insert(tier);
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
                    stack.push((existing, lost));
                    stack.push((incoming, lost));
                }
                PairOutcome::Family => {
                    tie |= lost.is_none();
                    stack.push((existing, lost));
                    stack.push((incoming, lost));
                }
                PairOutcome::Incoming(tier) => {
                    stack.push((incoming, lost));
                    stack.push((existing, Some(lost.unwrap_or(*tier))));
                }
                PairOutcome::Existing(tier) => {
                    stack.push((existing, lost));
                    stack.push((incoming, Some(lost.unwrap_or(*tier))));
                }
            }
        }
        let leaves = structural.iter().copied().collect::<BTreeSet<_>>();
        let members = selection.members.iter().copied().collect::<BTreeSet<_>>();
        if leaves != members || leaves.len() != structural.len() {
            return Err("decision graph disagrees with selection members");
        }

        let raw = &self.segment.original_raw;
        let mut contributions = Vec::new();
        for (index, &id) in selection.members.iter().enumerate() {
            let role = if tie || raw[id] != selection.raw {
                ContributionRole::CoMember
            } else if index == 0 {
                ContributionRole::Winner
            } else {
                ContributionRole::SameSpanMerge
            };
            push_original(self.segment, &mut contributions, id, role, None);
        }
        for (&id, &tier) in &defeated {
            push_original(
                self.segment,
                &mut contributions,
                id,
                ContributionRole::Defeated,
                Some(tier),
            );
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
        Ok(Lineage {
            settlement,
            contributions: finish(contributions),
        })
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
    tier: Option<ConflictTier>,
) {
    let original = &segment.originals[id];
    out.push(TraceContribution::new(
        original.recognizer_id.clone(),
        role,
        Some(segment.original_raw[id].clone()),
        tier,
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

/// Lineage of a safety-net item: one suspect is the winner on the item's span;
/// several suspects merged into one redaction region are co-members without a
/// span of their own here.
pub(super) fn safety_net(ids: &[String], raw_span: &Range<usize>) -> Lineage {
    let contributions = if let [only] = ids {
        vec![TraceContribution::new(
            only.clone(),
            ContributionRole::Winner,
            Some(raw_span.clone()),
            None,
        )]
    } else {
        ids.iter()
            .map(|id| TraceContribution::new(id.clone(), ContributionRole::CoMember, None, None))
            .collect()
    };
    Lineage {
        settlement: TraceSettlement::SafetyNet,
        contributions: finish(contributions),
    }
}

fn finish(mut contributions: Vec<TraceContribution>) -> Vec<TraceContribution> {
    contributions.sort_by_key(|item| {
        (
            item.recognizer_id.clone(),
            item.role,
            item.raw_span.as_ref().map(|span| (span.start, span.end)),
        )
    });
    contributions.dedup();
    contributions
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::pipeline::occurrence::Basis;
    use crate::{Candidate, PiiClass};

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
        Option<ConflictTier>,
    );

    fn roles(lineage: &Lineage) -> Vec<Role<'_>> {
        lineage
            .contributions
            .iter()
            .map(|c| (c.recognizer_id(), c.role(), c.raw_span(), c.tier()))
            .collect()
    }

    #[test]
    fn collateral_removal_is_a_defeat_on_the_winners_tier() {
        // The event shape `resolver::collateral_removal_has_no_fabricated_pair_outcome_or_membership` pins.
        let segment = segment(
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
        );
        let lineage = DecisionGraph::new(&segment)
            .selection(&selection(3, vec![2], 3..12, PiiClass::Email))
            .expect("lineage");
        assert_eq!(lineage.settlement, TraceSettlement::Resolve);
        assert_eq!(
            roles(&lineage),
            vec![
                (
                    "a",
                    ContributionRole::Defeated,
                    Some(0..5),
                    Some(ConflictTier::ClassPriority)
                ),
                (
                    "b",
                    ContributionRole::Defeated,
                    Some(10..15),
                    Some(ConflictTier::ClassPriority)
                ),
                ("c", ContributionRole::Winner, Some(3..12), None),
            ]
        );
    }

    #[test]
    fn a_nested_loser_carries_the_tier_its_subtree_left_the_winning_line_on() {
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
            roles(&lineage),
            vec![
                (
                    "x",
                    ContributionRole::Defeated,
                    Some(0..4),
                    Some(ConflictTier::ClassPriority)
                ),
                (
                    "y",
                    ContributionRole::Defeated,
                    Some(0..6),
                    Some(ConflictTier::ClassPriority)
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
        assert_eq!(lineage.settlement, TraceSettlement::CollisionTie);
        assert_eq!(
            roles(&lineage),
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
        assert_eq!(lineage.settlement, TraceSettlement::AnchorFallback);
        assert_eq!(
            roles(&lineage),
            vec![("tax.rule", ContributionRole::Winner, Some(0..6), None)]
        );
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
            roles(&lineage),
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
            roles(&merged),
            vec![
                ("a", ContributionRole::Winner, Some(0..5), None),
                ("b", ContributionRole::SameSpanMerge, Some(0..5), None),
            ]
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
    fn merged_safety_net_suspects_have_no_span_of_their_own() {
        let one = safety_net(&["nym".to_string()], &(3..9));
        assert_eq!(
            roles(&one),
            vec![("nym", ContributionRole::Winner, Some(3..9), None)]
        );
        let many = safety_net(&["a".to_string(), "b".to_string()], &(3..9));
        assert_eq!(
            roles(&many),
            vec![
                ("a", ContributionRole::CoMember, None, None),
                ("b", ContributionRole::CoMember, None, None),
            ]
        );
    }
}
