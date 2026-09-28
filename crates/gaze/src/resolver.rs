use std::collections::BTreeMap;
use std::ops::Range;

use crate::anchor_resolver::{AnchorOutcome, AnchorResolver};
use crate::LocaleTag;
use crate::{Candidate, ConflictTier, EvidenceKind, FamilyPolicyTable, PiiClass};

pub fn resolve_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    resolve_candidates_with_policy(candidates, &FamilyPolicyTable::EMPTY)
}

pub fn resolve_candidates_with_policy(
    mut candidates: Vec<Candidate>,
    policy: &FamilyPolicyTable,
) -> Vec<Candidate> {
    resolve_candidates_inner(&mut candidates, policy, None)
}

#[cfg(test)]
pub(crate) fn resolve_candidates_with_policy_and_anchors(
    mut candidates: Vec<Candidate>,
    policy: &FamilyPolicyTable,
    anchor_resolver: &AnchorResolver,
    input: &str,
    locale_chain: &[LocaleTag],
) -> Vec<Candidate> {
    resolve_candidates_inner(
        &mut candidates,
        policy,
        Some(AnchorContext {
            resolver: anchor_resolver,
            input,
            locale_chain,
        }),
    )
}

#[derive(Clone, Copy)]
struct AnchorContext<'a> {
    resolver: &'a AnchorResolver,
    input: &'a str,
    locale_chain: &'a [LocaleTag],
}

// Original ids refer to immutable detector payloads, never source labels.
pub(crate) struct CandidatePool {
    originals: Vec<Candidate>,
    pub(crate) order: Vec<usize>,
    pub(crate) events: Vec<ResolutionEvent>,
    next_node: usize,
    #[cfg(test)]
    pub(crate) work: ResolutionWork,
}

#[cfg(test)]
#[derive(Clone, Default, Debug)]
pub(crate) struct ResolutionWork {
    pub(crate) pools: usize,
    pub(crate) candidates: usize,
    pub(crate) overlap_probes: usize,
}

pub(crate) struct WholeCandidate {
    pub(crate) candidate: Candidate,
    pub(crate) members: Vec<usize>,
    pub(crate) node: usize,
    pub(crate) settlement: Settlement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContainmentMode {
    Enclosing,
    Prior,
}

/// Whether collision policy has already decided this span's family.
///
/// Kept apart from `Candidate::decided_by`, which is the audit label of the
/// last rung that touched the span: a later, unrelated overlap decided on the
/// base ladder rewrites that label, and the missing-anchor fallback must not
/// reopen a family the policy already settled because of it (todo #3709).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Settlement {
    /// No collision-policy verdict yet; the anchor rung still applies.
    #[default]
    Open,
    /// Won a collision-policy comparison or is a precedence-tie family token.
    CollisionPolicy,
}

// A compact private decision graph. Node ids below originals.len() are evidence;
// subsequent ids refer to earlier Pair events, without recursively cloning history.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ResolutionEvent {
    Pair {
        existing: usize,
        incoming: usize,
        result: usize,
        outcome: PairOutcome,
    },
    Collateral {
        removed: usize,
        replacing: usize,
    },
    Recovery {
        node: usize,
        normalized: Range<usize>,
        raw: Range<usize>,
    },
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(crate) enum PairOutcome {
    Merge,
    Family,
    Incoming(ConflictTier),
    Existing(ConflictTier),
}

pub(crate) fn candidate_order(originals: &[Candidate]) -> Vec<usize> {
    let mut order = (0..originals.len()).collect::<Vec<_>>();
    // Keep the legacy stable key, including its input-order ties.
    order.sort_by(|&a, &b| {
        let (a, b) = (&originals[a], &originals[b]);
        a.span
            .start
            .cmp(&b.span.start)
            .then_with(|| b.span.end.cmp(&a.span.end))
            .then_with(|| class_priority(&b.class).cmp(&class_priority(&a.class)))
            .then_with(|| b.priority.cmp(&a.priority))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.recognizer_id.cmp(&b.recognizer_id))
    });
    order
}

impl CandidatePool {
    pub(crate) fn take_originals(&mut self) -> Vec<Candidate> {
        std::mem::take(&mut self.originals)
    }

    pub(crate) fn originals(&self) -> &[Candidate] {
        &self.originals
    }

    pub(crate) fn new(originals: Vec<Candidate>) -> Self {
        let order = candidate_order(&originals);
        Self {
            next_node: originals.len(),
            originals,
            order,
            events: Vec::new(),
            #[cfg(test)]
            work: ResolutionWork::default(),
        }
    }

    pub(crate) fn resolve(
        &mut self,
        ids: &[usize],
        policy: &FamilyPolicyTable,
        anchors: Option<(&AnchorResolver, &str, &[LocaleTag])>,
    ) -> Vec<WholeCandidate> {
        let event_start = self.events.len();
        let next_node_start = self.next_node;
        #[cfg(test)]
        let work_start = self.work.clone();
        let selected = self.resolve_with_mode(ids, policy, anchors, ContainmentMode::Enclosing);
        let used_containment = self.events[event_start..].iter().any(|event| {
            matches!(
                event,
                ResolutionEvent::Pair {
                    outcome: PairOutcome::Incoming(ConflictTier::SameClassContainment)
                        | PairOutcome::Existing(ConflictTier::SameClassContainment),
                    ..
                }
            )
        });
        if !used_containment {
            return selected;
        }

        // A third partial rival can defeat the new container after it defeats
        // an inner span. Keep the prior selection if that would expose bytes
        // the prior resolver covered. Most pools never enter this second pass.
        let mut prior = Self {
            originals: self.originals.clone(),
            order: self.order.clone(),
            events: self.events[..event_start].to_vec(),
            next_node: next_node_start,
            #[cfg(test)]
            work: work_start,
        };
        let prior_selected = prior.resolve_with_mode(ids, policy, anchors, ContainmentMode::Prior);
        if covers_all_spans(&selected, &prior_selected) {
            return selected;
        }
        self.events = prior.events;
        self.next_node = prior.next_node;
        #[cfg(test)]
        {
            self.work = prior.work;
        }
        prior_selected
    }

    fn resolve_with_mode(
        &mut self,
        ids: &[usize],
        policy: &FamilyPolicyTable,
        anchors: Option<(&AnchorResolver, &str, &[LocaleTag])>,
        mode: ContainmentMode,
    ) -> Vec<WholeCandidate> {
        #[cfg(test)]
        {
            self.work.pools += 1;
            self.work.candidates += ids.len();
        }
        let anchor_ctx = anchors.map(|(resolver, input, locale_chain)| AnchorContext {
            resolver,
            input,
            locale_chain,
        });
        let mut resolved = ResolvedSet::default();
        for &id in ids {
            let candidate = WholeCandidate {
                candidate: self.originals[id].clone(),
                members: vec![id],
                node: id,
                settlement: Settlement::Open,
            };
            self.insert(&mut resolved, candidate, policy, anchor_ctx, mode);
        }
        // Every arbitration arm keeps resolved spans disjoint; the fallback
        // exists for a future arm that does not.
        #[cfg(test)]
        assert!(!resolved.linear, "resolved spans overlap");
        let mut resolved = resolved.into_vec();
        if let Some(ctx) = anchor_ctx {
            resolved = resolved
                .into_iter()
                .map(|mut node| {
                    node.candidate =
                        apply_missing_anchor_fallback(node.candidate, node.settlement, policy, ctx);
                    node
                })
                .collect();
        }
        resolved.sort_by_key(|node| node.candidate.span.start);
        resolved
    }

    fn insert(
        &mut self,
        resolved: &mut ResolvedSet,
        candidate: WholeCandidate,
        policy: &FamilyPolicyTable,
        anchor_ctx: Option<AnchorContext<'_>>,
        mode: ContainmentMode,
    ) {
        let mut probes = 0;
        let slot = resolved.first_overlap(&candidate.candidate.span, &mut probes);
        #[cfg(test)]
        {
            self.work.overlap_probes += probes;
        }
        let Some(slot) = slot else {
            resolved.push(candidate);
            return;
        };
        let mut held = resolved.take(slot);
        let overlap = Overlap::classify(&held.candidate.span, &candidate.candidate.span)
            .expect("first_overlap returns an overlapping slot");
        let existing = held.node;
        let incoming = candidate.node;
        let result = self.next_node;
        self.next_node += 1;
        let mut removal = None;
        let outcome = match arbitrate(
            &held.candidate,
            &candidate.candidate,
            overlap,
            policy,
            anchor_ctx,
            mode,
        ) {
            Arbitration::Merge => {
                held.members.extend(candidate.members);
                merge_same_span_same_class(&mut held.candidate, candidate.candidate);
                PairOutcome::Merge
            }
            Arbitration::Family(tie) => {
                held.members.extend(candidate.members);
                held.candidate = *tie;
                held.settlement = Settlement::CollisionPolicy;
                removal = Some(ConflictTier::CollisionPolicy);
                PairOutcome::Family
            }
            Arbitration::CandidateWins(tier) => {
                let mut candidate = candidate;
                candidate
                    .candidate
                    .source_recognizer_ids
                    .extend(held.candidate.source_recognizer_ids.iter().cloned());
                candidate.candidate.decided_by = tier;
                if tier == ConflictTier::CollisionPolicy {
                    candidate.settlement = Settlement::CollisionPolicy;
                }
                candidate
                    .candidate
                    .merged_sources
                    .push(held.candidate.source.clone());
                // Defeated candidates are provenance, not structural members.
                held = candidate;
                removal = Some(tier);
                PairOutcome::Incoming(tier)
            }
            Arbitration::ExistingWins(tier) => {
                held.candidate
                    .source_recognizer_ids
                    .extend(candidate.candidate.source_recognizer_ids.iter().cloned());
                // Relabel for audit only: a settled family stays settled.
                held.candidate.decided_by = tier;
                if tier == ConflictTier::CollisionPolicy {
                    held.settlement = Settlement::CollisionPolicy;
                }
                held.candidate
                    .merged_sources
                    .push(candidate.candidate.source);
                PairOutcome::Existing(tier)
            }
        };
        held.node = result;
        self.events.push(ResolutionEvent::Pair {
            existing,
            incoming,
            result,
            outcome,
        });
        if overlap != Overlap::Exact {
            if let Some(tier) = removal {
                remove_overlaps(resolved, &mut held, tier, &mut self.events);
            }
        }
        resolved.put(slot, held);
    }
}

/// The resolved spans of one pool, in arrival order, with an index by start.
///
/// Arrival order decides which rival an incoming candidate meets first, so
/// it is kept as slot order: a replaced entry keeps its slot, a removed one
/// leaves a hole. Finding the first rival by scanning every slot made a pool
/// of N candidates cost O(N^2) (todo 3895); the index finds it among the
/// spans that actually overlap.
#[derive(Default)]
struct ResolvedSet {
    slots: Vec<Option<WholeCandidate>>,
    /// Non-empty spans by start. Resolved spans never overlap each other, so
    /// ordering them by start orders them by end too.
    by_start: BTreeMap<usize, usize>,
    /// Slots holding an empty span, which can sit inside a non-empty one.
    empty: Vec<usize>,
    /// Set once a span would break the no-overlap invariant the index relies
    /// on. The pool then scans every slot, as before the index existed: an
    /// arbitration bug costs time, never a span silently left unarbitrated.
    linear: bool,
}

impl ResolvedSet {
    /// Lowest slot whose span overlaps `span`, as the full scan would find.
    fn first_overlap(&self, span: &Range<usize>, probes: &mut usize) -> Option<usize> {
        self.overlapping(span, probes).into_iter().min()
    }

    fn overlapping(&self, span: &Range<usize>, probes: &mut usize) -> Vec<usize> {
        let mut found = Vec::new();
        if self.linear {
            for (slot, held) in self.slots.iter().enumerate() {
                *probes += 1;
                if held
                    .as_ref()
                    .is_some_and(|held| overlaps(&held.candidate.span, span))
                {
                    found.push(slot);
                }
            }
            return found;
        }
        for (_, &slot) in self.by_start.range(..span.end).rev() {
            *probes += 1;
            let existing = &self.span(slot);
            if existing.end <= span.start {
                break;
            }
            if overlaps(existing, span) {
                found.push(slot);
            }
        }
        for &slot in &self.empty {
            *probes += 1;
            if overlaps(&self.span(slot), span) {
                found.push(slot);
            }
        }
        found
    }

    fn span(&self, slot: usize) -> Range<usize> {
        self.slots[slot]
            .as_ref()
            .expect("indexed slot is occupied")
            .candidate
            .span
            .clone()
    }

    fn push(&mut self, candidate: WholeCandidate) {
        self.slots.push(None);
        self.put(self.slots.len() - 1, candidate);
    }

    fn put(&mut self, slot: usize, candidate: WholeCandidate) {
        let span = candidate.candidate.span.clone();
        self.slots[slot] = Some(candidate);
        if self.linear {
            return;
        }
        if span.is_empty() {
            self.empty.push(slot);
            return;
        }
        let before = self.by_start.range(..=span.start).next_back();
        let after = self.by_start.range(span.start..).next();
        let disjoint = before.is_none_or(|(_, &slot)| self.span(slot).end <= span.start)
            && after.is_none_or(|(&start, _)| span.end <= start);
        if disjoint {
            self.by_start.insert(span.start, slot);
        } else {
            self.linear = true;
        }
    }

    fn take(&mut self, slot: usize) -> WholeCandidate {
        let candidate = self.slots[slot].take().expect("slot is occupied");
        let span = &candidate.candidate.span;
        if !self.linear {
            if span.is_empty() {
                self.empty.retain(|&held| held != slot);
            } else {
                self.by_start.remove(&span.start);
            }
        }
        candidate
    }

    fn into_vec(self) -> Vec<WholeCandidate> {
        self.slots.into_iter().flatten().collect()
    }
}

fn covers_all_spans(selected: &[WholeCandidate], prior: &[WholeCandidate]) -> bool {
    // Both lists are sorted by start. A selected span that ends before one
    // prior span starts ends before every later one too, so the scan resumes
    // there instead of at 0 (restarting made this O(N^2), todo 3895).
    let mut first = 0;
    prior.iter().all(|prior_node| {
        let mut cursor = prior_node.candidate.span.start;
        while selected
            .get(first)
            .is_some_and(|node| node.candidate.span.end <= cursor)
        {
            first += 1;
        }
        for selected_node in &selected[first..] {
            let span = &selected_node.candidate.span;
            if span.end <= cursor {
                continue;
            }
            if span.start > cursor {
                break;
            }
            cursor = cursor.max(span.end);
            if cursor >= prior_node.candidate.span.end {
                return true;
            }
        }
        false
    })
}

fn resolve_candidates_inner(
    candidates: &mut Vec<Candidate>,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Vec<Candidate> {
    let mut pool = CandidatePool::new(std::mem::take(candidates));
    let order = pool.order.clone();
    pool.resolve(
        &order,
        policy,
        anchor_ctx.map(|ctx| (ctx.resolver, ctx.input, ctx.locale_chain)),
    )
    .into_iter()
    .map(|node| node.candidate)
    .collect()
}

/// Geometric relation between an already-resolved span and an incoming one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlap {
    /// Identical spans: the winner keeps the slot; nothing else can overlap.
    Exact,
    /// One span fully covers the other.
    Containment,
    /// Spans overlap without either covering the other.
    Partial,
}

impl Overlap {
    fn classify(existing: &Range<usize>, candidate: &Range<usize>) -> Option<Self> {
        if !overlaps(existing, candidate) {
            return None;
        }
        if existing == candidate {
            Some(Self::Exact)
        } else if contains(existing, candidate) || contains(candidate, existing) {
            Some(Self::Containment)
        } else {
            Some(Self::Partial)
        }
    }
}

/// Outcome of arbitrating one overlapping pair.
enum Arbitration {
    /// Exact span, same class: provenance and confidence merge into `existing`.
    Merge,
    /// Precedence tie inside one collision family: emit the family-level
    /// candidate in place of both rivals.
    Family(Box<Candidate>),
    /// `candidate` replaces `existing`; the tier names what decided it.
    CandidateWins(ConflictTier),
    /// `existing` keeps the slot; the tier names the rung that separated the
    /// pair (never a label left over from an earlier overlap).
    ExistingWins(ConflictTier),
}

fn arbitrate(
    existing: &Candidate,
    candidate: &Candidate,
    overlap: Overlap,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
    mode: ContainmentMode,
) -> Arbitration {
    // A same-class container covers every byte its enclosed candidate protected,
    // and adds more. Geometry settles this before family policy or the base ladder.
    if mode == ContainmentMode::Enclosing
        && overlap == Overlap::Containment
        && existing.class == candidate.class
    {
        return if contains(&candidate.span, &existing.span) {
            Arbitration::CandidateWins(ConflictTier::SameClassContainment)
        } else {
            Arbitration::ExistingWins(ConflictTier::SameClassContainment)
        };
    }

    // Exact-span family ties still collapse into one family token before the
    // same-class merge or base ladder.
    if let Some(tie) = family_tie_candidate(candidate, existing, policy) {
        return Arbitration::Family(Box::new(tie));
    }
    if overlap == Overlap::Exact && existing.class == candidate.class {
        return Arbitration::Merge;
    }

    if mode == ContainmentMode::Prior
        && overlap == Overlap::Containment
        && existing.class == candidate.class
    {
        let candidate_validated = candidate.checksum_validated();
        let existing_validated = existing.checksum_validated();
        if candidate_validated != existing_validated {
            return if candidate_validated {
                Arbitration::CandidateWins(ConflictTier::Validator)
            } else {
                Arbitration::ExistingWins(ConflictTier::Validator)
            };
        }
        return ladder_verdict(existing, candidate);
    }

    if let Some(candidate_wins) = policy.compare(&candidate.recognizer_id, &existing.recognizer_id)
    {
        return if candidate_wins {
            Arbitration::CandidateWins(ConflictTier::CollisionPolicy)
        } else {
            Arbitration::ExistingWins(ConflictTier::CollisionPolicy)
        };
    }

    // Anchor rung, consulted once per pair: an anchored *incoming* candidate
    // takes the slot from a rival outside its family and defers the
    // found/missing verdict to `apply_missing_anchor_fallback`. An anchored
    // incumbent gets no short-circuit; the ladder decides and names the tier,
    // so `AnchoredContext` is never stamped on a ladder-decided overlap.
    if let Some(anchor_ctx) = anchor_ctx {
        if requires_anchor(candidate, policy, anchor_ctx) {
            return Arbitration::CandidateWins(ConflictTier::AnchoredContext);
        }
    }

    // Containment-precedence rung: a candidate that wholly contains a
    // candidate of another class wins the whole span as one token, unless it
    // is less certain than what it would swallow (todo #3740). It sits after
    // collision-family policy and the anchor rung so those keep deciding what
    // they decide today, and before the structured-containment rung, which it
    // generalises: that rung still catches a custom container the guard
    // refuses (a plain-regex URL over a validated email).
    if let Some(container_is_candidate) =
        containment_precedence(existing, candidate, overlap, policy, anchor_ctx)
    {
        return if container_is_candidate {
            Arbitration::CandidateWins(ConflictTier::ContainmentPrecedence)
        } else {
            Arbitration::ExistingWins(ConflictTier::ContainmentPrecedence)
        };
    }

    // Structured-containment rung: a builtin-class span strictly inside a
    // custom-class structured span never evicts its container. Without it the
    // base ladder's class priority (Email/Name/Organization/Location above
    // every `Custom`) let an NER sub-token split a URL, IBAN or credential
    // around a mid-word token and leave the rest raw (todo #3025).
    if let Some(container_is_candidate) = structured_containment(existing, candidate, overlap) {
        return if container_is_candidate {
            Arbitration::CandidateWins(ConflictTier::StructuredContainment)
        } else {
            Arbitration::ExistingWins(ConflictTier::StructuredContainment)
        };
    }

    ladder_verdict(existing, candidate)
}

/// Detects the structured-containment shape and says which side is the
/// container: `Some(true)` when `candidate` encloses a builtin-class
/// `existing`, `Some(false)` when `existing` encloses a builtin-class
/// `candidate`, `None` when the pair is not a custom-class span enclosing a
/// builtin-class span. Geometry decides, never arrival order, so the verdict
/// is permutation-invariant. Same-class pairs, custom-inside-custom,
/// builtin-inside-builtin, and builtin containers over custom spans are all
/// left to the existing rungs.
fn structured_containment(
    existing: &Candidate,
    candidate: &Candidate,
    overlap: Overlap,
) -> Option<bool> {
    if overlap != Overlap::Containment {
        return None;
    }
    let candidate_encloses = contains(&candidate.span, &existing.span);
    let (container, enclosed) = if candidate_encloses {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    let structured_container = matches!(container.class, PiiClass::Custom(_));
    let builtin_enclosed = !matches!(enclosed.class, PiiClass::Custom(_));
    (structured_container && builtin_enclosed).then_some(candidate_encloses)
}

/// Certainty of a candidate's evidence, read from fields it already carries.
/// The order is the guard of the containment-precedence rung: a container
/// may swallow a differently-classed candidate only when its tier is at
/// least the contained candidate's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EvidenceTier {
    /// A candidate whose emitter declared [`EvidenceKind::Learned`], or declared nothing.
    Learned,
    /// A plain regex or dictionary term.
    Pattern,
    /// An anchored or cue-structured match (`structural.*` source, or a
    /// mandatory anchor found in context).
    Anchored,
    /// A validator passed: mod-97, Luhn, RFC email, E.164, ... (the
    /// candidate carries a canonical form).
    Validated,
}

fn evidence_tier(
    candidate: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> EvidenceTier {
    if candidate.checksum_validated() {
        return EvidenceTier::Validated;
    }
    if candidate.source.starts_with("structural.") {
        return EvidenceTier::Anchored;
    }
    if let Some(ctx) = anchor_ctx {
        if matches!(
            ctx.resolver
                .resolve(candidate, ctx.input, policy, ctx.locale_chain),
            AnchorOutcome::Found
        ) {
            return EvidenceTier::Anchored;
        }
    }
    match candidate.evidence {
        EvidenceKind::Learned => EvidenceTier::Learned,
        EvidenceKind::Rule => EvidenceTier::Pattern,
    }
}

/// Detects the containment-precedence shape and says which side is the
/// container: `Some(true)` when `candidate` wholly encloses a
/// differently-classed `existing` and may swallow it, `Some(false)` when
/// `existing` encloses `candidate` and may, `None` when the spans are not
/// nested, share a class, or the container's evidence tier is below the
/// contained candidate's. Equal tiers go to the container: on the reference
/// letter the phone rule is validator-backed like the IBAN, and breaking the
/// tie by score hands the middle of the IBAN to the phone. Geometry and
/// tiers decide, never arrival order. Partial overlaps and exact same-class
/// pairs keep their existing rungs; strict same-class containment resolves
/// earlier by the enclosing span.
fn containment_precedence(
    existing: &Candidate,
    candidate: &Candidate,
    overlap: Overlap,
    policy: &FamilyPolicyTable,
    anchor_ctx: Option<AnchorContext<'_>>,
) -> Option<bool> {
    if overlap != Overlap::Containment || existing.class == candidate.class {
        return None;
    }
    let candidate_encloses = contains(&candidate.span, &existing.span);
    let (container, enclosed) = if candidate_encloses {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    (evidence_tier(container, policy, anchor_ctx) >= evidence_tier(enclosed, policy, anchor_ctx))
        .then_some(candidate_encloses)
}

fn requires_anchor(
    candidate: &Candidate,
    policy: &FamilyPolicyTable,
    anchor_ctx: AnchorContext<'_>,
) -> bool {
    match anchor_ctx
        .resolver
        .resolve(candidate, anchor_ctx.input, policy, anchor_ctx.locale_chain)
    {
        AnchorOutcome::Found | AnchorOutcome::Missing { .. } => true,
        AnchorOutcome::NotRequired => false,
    }
}

/// Runs the base ladder in both directions and labels the winner with the rung
/// that separated the pair. When every rung ties (same recognizer id, class
/// priority, rule priority, score, and length) the incumbent keeps the slot
/// and the row carries the terminal `RecognizerId` rung rather than a stale
/// tier from an earlier overlap.
fn ladder_verdict(existing: &Candidate, candidate: &Candidate) -> Arbitration {
    if let Some(tier) = compare_base_ladder(candidate, existing) {
        return Arbitration::CandidateWins(tier);
    }
    Arbitration::ExistingWins(
        compare_base_ladder(existing, candidate).unwrap_or(ConflictTier::RecognizerId),
    )
}

fn merge_same_span_same_class(existing: &mut Candidate, candidate: Candidate) {
    existing.score = combine_confidence(existing.score, candidate.score);
    existing
        .source_recognizer_ids
        .extend(candidate.source_recognizer_ids.iter().cloned());
    append_unique(&mut existing.recognizer_id, &candidate.recognizer_id);
    append_unique(&mut existing.source, &candidate.source);
    if existing.canonical_form.is_none() {
        existing.canonical_form = candidate.canonical_form;
    }
    // A recorded checksum failure sticks: the merged value was never validated.
    existing.validator_fail_reason = existing
        .validator_fail_reason
        .or(candidate.validator_fail_reason);
    existing.decided_by = ConflictTier::Merged;
    existing.merged_sources.push(candidate.source);
}

fn combine_confidence(left: f32, right: f32) -> f32 {
    1.0 - (1.0 - left.clamp(0.0, 1.0)) * (1.0 - right.clamp(0.0, 1.0))
}

fn append_unique(existing: &mut String, next: &str) {
    if existing.split('+').any(|part| part == next) {
        return;
    }
    if !existing.is_empty() {
        existing.push('+');
    }
    existing.push_str(next);
}

/// The base conflict ladder: class-priority > rule-priority > score >
/// span-length > recognizer-id. Returns the tier at which `candidate` beats
/// `existing`, or `None` when it does not.
fn compare_base_ladder(candidate: &Candidate, existing: &Candidate) -> Option<ConflictTier> {
    if class_priority(&candidate.class) != class_priority(&existing.class) {
        return (class_priority(&candidate.class) > class_priority(&existing.class))
            .then_some(ConflictTier::ClassPriority);
    }
    if candidate.priority != existing.priority {
        return (candidate.priority > existing.priority).then_some(ConflictTier::RulePriority);
    }
    if candidate.score != existing.score {
        return candidate
            .score
            .total_cmp(&existing.score)
            .is_gt()
            .then_some(ConflictTier::Score);
    }
    let candidate_len = candidate.span.end - candidate.span.start;
    let existing_len = existing.span.end - existing.span.start;
    if candidate_len != existing_len {
        return (candidate_len > existing_len).then_some(ConflictTier::SpanLength);
    }
    (candidate.recognizer_id < existing.recognizer_id).then_some(ConflictTier::RecognizerId)
}

fn family_tie_candidate(
    candidate: &Candidate,
    existing: &Candidate,
    policy: &FamilyPolicyTable,
) -> Option<Candidate> {
    let family = policy.precedence_tie_family(&candidate.recognizer_id, &existing.recognizer_id)?;
    let mut merged_sources = existing.merged_sources.clone();
    merged_sources.extend(candidate.merged_sources.iter().cloned());
    merged_sources.push(existing.recognizer_id.clone());
    merged_sources.push(candidate.recognizer_id.clone());
    merged_sources.sort();
    merged_sources.dedup();
    let source_recognizer_ids = existing
        .source_recognizer_ids
        .iter()
        .chain(&candidate.source_recognizer_ids)
        .cloned()
        .collect();
    // A tie rests on the weaker of its two sides.
    let evidence =
        if candidate.evidence == EvidenceKind::Rule && existing.evidence == EvidenceKind::Rule {
            EvidenceKind::Rule
        } else {
            EvidenceKind::Learned
        };
    let mut tied = Candidate::new(
        candidate.span.start.min(existing.span.start)..candidate.span.end.max(existing.span.end),
        PiiClass::family(family),
        format!("collision-family:{family}"),
        candidate.score.max(existing.score),
        candidate.priority.max(existing.priority),
        None,
        "collision-family",
        format!("collision-family:{family}"),
        ConflictTier::CollisionPolicy,
        merged_sources,
    );
    tied.source_recognizer_ids = source_recognizer_ids;
    Some(tied.with_evidence(evidence))
}

fn apply_missing_anchor_fallback(
    candidate: Candidate,
    settlement: Settlement,
    policy: &FamilyPolicyTable,
    anchor_ctx: AnchorContext<'_>,
) -> Candidate {
    match missing_anchor_family(&candidate, settlement, policy, anchor_ctx) {
        Some(family) => family_fallback_candidate(candidate, family, ConflictTier::AnchoredContext),
        None => candidate,
    }
}

fn missing_anchor_family(
    candidate: &Candidate,
    settlement: Settlement,
    policy: &FamilyPolicyTable,
    anchor_ctx: AnchorContext<'_>,
) -> Option<String> {
    if settlement == Settlement::CollisionPolicy {
        return None;
    }
    match anchor_ctx
        .resolver
        .resolve(candidate, anchor_ctx.input, policy, anchor_ctx.locale_chain)
    {
        AnchorOutcome::Missing { family, .. } => Some(family),
        AnchorOutcome::Found | AnchorOutcome::NotRequired => None,
    }
}

pub(crate) fn effective_view(
    candidate: &Candidate,
    policy: &FamilyPolicyTable,
    resolver: &AnchorResolver,
    input: &str,
    locale_chain: &[LocaleTag],
) -> (PiiClass, String) {
    // Detector originals have not been through arbitration, so no collision
    // policy has settled their family yet.
    match missing_anchor_family(
        candidate,
        Settlement::Open,
        policy,
        AnchorContext {
            resolver,
            input,
            locale_chain,
        },
    ) {
        Some(family) => (
            PiiClass::family(&family),
            format!("collision-family:{family}"),
        ),
        None => (candidate.class.clone(), candidate.token_family.clone()),
    }
}

fn family_fallback_candidate(
    candidate: Candidate,
    family: String,
    decided_by: ConflictTier,
) -> Candidate {
    let original_recognizer_id = candidate.recognizer_id.clone();
    let mut merged_sources = candidate.merged_sources;
    if !merged_sources
        .iter()
        .any(|source| source == &original_recognizer_id)
    {
        merged_sources.push(original_recognizer_id);
    }
    let source_recognizer_ids = candidate.source_recognizer_ids.clone();
    let evidence = candidate.evidence;
    let mut fallback = Candidate::new(
        candidate.span,
        PiiClass::family(&family),
        format!("collision-family:{family}"),
        candidate.score,
        candidate.priority,
        None,
        format!("collision-family:{family}"),
        candidate.source,
        decided_by,
        merged_sources,
    );
    fallback.source_recognizer_ids = source_recognizer_ids;
    fallback.with_evidence(evidence)
}

#[cfg(test)]
thread_local! {
    /// Test-only count of `remove_overlaps` entries on the current thread.
    ///
    /// The two exact-span short-circuits in `insert_candidate` cannot be
    /// observed through the resolver's output: an exact-overlap winner keeps
    /// the very span it replaced, and the resolved set is pairwise disjoint
    /// (property 1 in `tests/prop_resolver_invariants.rs`), so overlap removal
    /// would find nothing to remove and return an identical set. Dropping both
    /// guards is therefore output-identical, and only the entry count can lock
    /// them; `non_exact_winner_enters_overlap_removal` is the positive control
    /// that keeps that count honest. Thread-local because the harness runs
    /// tests in parallel.
    static REMOVE_OVERLAPS_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn remove_overlaps(
    resolved: &mut ResolvedSet,
    winner: &mut WholeCandidate,
    tier: ConflictTier,
    events: &mut Vec<ResolutionEvent>,
) {
    #[cfg(test)]
    REMOVE_OVERLAPS_CALLS.with(|calls| calls.set(calls.get() + 1));

    let mut losers = resolved.overlapping(&winner.candidate.span, &mut 0);
    // Slot order is arrival order, which fixes the audit order of the losers.
    losers.sort_unstable();
    for slot in losers {
        let loser = resolved.take(slot);
        events.push(ResolutionEvent::Collateral {
            removed: loser.node,
            replacing: winner.node,
        });
        winner.candidate.merged_sources.push(loser.candidate.source);
        // Audit relabel only; `settlement` belongs to the winner and is
        // already correct for the pair it just won.
        winner.candidate.decided_by = tier;
    }
}

fn class_priority(class: &PiiClass) -> u8 {
    match class {
        PiiClass::Email => 90,
        PiiClass::Name => 80,
        PiiClass::Organization => 70,
        PiiClass::Location => 60,
        PiiClass::Custom(_) => 50,
    }
}

fn contains(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start <= right.start && left.end >= right.end
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stamps evidence as the registry would: the `ner` stand-in is learned, every other
    /// fixture id stands in for a rule.
    fn candidate(span: Range<usize>, class: PiiClass, score: f32, id: &str) -> Candidate {
        let evidence = if id == crate::NER_RECOGNIZER_ID {
            EvidenceKind::Learned
        } else {
            EvidenceKind::Rule
        };
        Candidate::new(
            span,
            class,
            id,
            score,
            0,
            None,
            "counter",
            id,
            ConflictTier::None,
            Vec::new(),
        )
        .with_evidence(evidence)
    }

    /// A canonical form proves a validator passed only when no failure was recorded: an IBAN or
    /// card kept by `ValidatorOnFail::Record` must not rank as validated (solo todo 3906).
    #[test]
    fn a_recorded_checksum_failure_is_not_validated_evidence() {
        let class = PiiClass::custom("iban").expect("class");
        let mut failed = candidate(0..4, class.clone(), 0.7, "iban.structural");
        failed.canonical_form = Some("DE99".into());
        assert!(failed.checksum_validated());
        failed.validator_fail_reason = Some(gaze_types::ValidatorFailReason::IbanMod97Failed);
        assert!(!failed.checksum_validated());
        let policy = crate::RecognizerRegistry::builder().build();
        assert_eq!(
            evidence_tier(&failed, policy.family_policy(), None),
            EvidenceTier::Pattern
        );
    }

    #[test]
    fn a_merge_keeps_a_recorded_checksum_failure_from_either_side() {
        let class = PiiClass::custom("iban").expect("class");
        let mut failed = candidate(0..4, class.clone(), 0.7, "iban.cued");
        failed.validator_fail_reason = Some(gaze_types::ValidatorFailReason::IbanMod97Failed);
        let clean = candidate(0..4, class, 0.7, "iban.structural");

        let mut existing = clean.clone();
        merge_same_span_same_class(&mut existing, failed.clone());
        assert_eq!(
            existing.validator_fail_reason,
            Some(gaze_types::ValidatorFailReason::IbanMod97Failed)
        );
        let mut existing = failed;
        merge_same_span_same_class(&mut existing, clean);
        assert_eq!(
            existing.validator_fail_reason,
            Some(gaze_types::ValidatorFailReason::IbanMod97Failed)
        );
    }

    /// Two variants of one collision family at equal precedence: the shape
    /// `family_tie_candidate` recognises as a precedence tie.
    fn tenant_document_registry() -> crate::RecognizerRegistry {
        crate::RecognizerRegistry::builder()
            .register_collision(
                "doc.alpha",
                crate::CollisionMembership::new("tenant-document", "alpha", 10, None),
            )
            .register_collision(
                "doc.beta",
                crate::CollisionMembership::new("tenant-document", "beta", 10, None),
            )
            .build()
    }

    /// Resolves and reports how many times the resolver entered
    /// `remove_overlaps`, so the exact-span short-circuits can be asserted
    /// directly rather than through output that does not change.
    fn counting_removals<F>(resolve: F) -> (Vec<Candidate>, usize)
    where
        F: FnOnce() -> Vec<Candidate>,
    {
        REMOVE_OVERLAPS_CALLS.with(|calls| calls.set(0));
        let resolved = resolve();
        (resolved, REMOVE_OVERLAPS_CALLS.with(std::cell::Cell::get))
    }

    #[test]
    fn exact_span_same_class_merges_provenance_and_confidence() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Email, 0.70, "regex"),
            candidate(0..5, PiiClass::Email, 0.50, "dict"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "regex+dict");
        assert!((resolved[0].score - 0.85).abs() < 0.0001);
    }

    #[test]
    fn exact_span_different_class_uses_class_priority_then_score() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Name, 0.99, "ner"),
            candidate(0..5, PiiClass::Email, 0.70, "regex"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    #[test]
    fn collision_policy_precedes_class_priority() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban",
                crate::CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
            )
            .build();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(0..5, PiiClass::Email, 0.70, "pan"),
                candidate(
                    0..5,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.70,
                    "iban",
                ),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban");
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
    }

    #[test]
    fn family_policy_arbitrates_before_mandatory_anchor_resolution() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan.structural",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(0..5, PiiClass::Email, 0.70, "pan.structural"),
                candidate(
                    0..5,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.70,
                    "iban.structural",
                ),
            ],
            registry.family_policy(),
            &AnchorResolver::default(),
            "DE893",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].class, PiiClass::Custom("iban".to_string()));
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
    }

    #[test]
    fn precedence_tie_emits_family_level_candidate() {
        let registry = tenant_document_registry();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(
                    0..5,
                    PiiClass::custom("alpha").expect("valid custom class"),
                    0.70,
                    "doc.alpha",
                ),
                candidate(
                    0..5,
                    PiiClass::custom("beta").expect("valid custom class"),
                    0.70,
                    "doc.beta",
                ),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].class,
            PiiClass::Custom("family:tenant-document".to_string())
        );
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            resolved[0].merged_sources,
            vec!["doc.alpha".to_string(), "doc.beta".to_string()]
        );
    }

    /// `arbitrate` probes the family tie *before* the exact same-class merge.
    /// Both rivals here report the same class, so a merge that ran first would
    /// swallow the tie and emit an ordinary merged candidate instead of the
    /// family token. The other precedence-tie fixtures pair different classes
    /// and cannot fail when that ordering regresses.
    #[test]
    fn exact_span_same_class_precedence_tie_emits_family_candidate_not_merge() {
        let registry = tenant_document_registry();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(
                    0..5,
                    PiiClass::custom("tenant-doc").expect("valid custom class"),
                    0.70,
                    "doc.alpha",
                ),
                candidate(
                    0..5,
                    PiiClass::custom("tenant-doc").expect("valid custom class"),
                    0.70,
                    "doc.beta",
                ),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].class,
            PiiClass::Custom("family:tenant-document".to_string()),
            "an exact same-class overlap must not merge ahead of the family tie"
        );
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            resolved[0].merged_sources,
            vec!["doc.alpha".to_string(), "doc.beta".to_string()]
        );
    }

    /// A non-family rival that loses to the first family-tie partner before the
    /// second partner arrives must survive in `merged_sources` after the tie
    /// collapses the two partners into the family-level candidate. The audit
    /// trail derives loser accounting exclusively from `merged_sources`, so a
    /// reset here would silently drop the earlier loser from `conflict_loser`
    /// rows and `AmbiguityRecord.losing_candidates`.
    #[test]
    fn family_tie_preserves_prior_nonfamily_loser_in_merged_sources() {
        let registry = tenant_document_registry();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(
                    0..5,
                    PiiClass::custom("alpha_doc").expect("valid custom class"),
                    0.95,
                    "doc.alpha",
                ),
                candidate(
                    0..5,
                    PiiClass::custom("nonfamily").expect("valid custom class"),
                    0.92,
                    "aaa.nonfamily",
                ),
                candidate(
                    0..5,
                    PiiClass::custom("beta_doc").expect("valid custom class"),
                    0.90,
                    "doc.beta",
                ),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].class,
            PiiClass::Custom("family:tenant-document".to_string())
        );
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert!(
            resolved[0]
                .merged_sources
                .iter()
                .any(|src| src == "aaa.nonfamily"),
            "prior non-family loser must survive the family tie: {:?}",
            resolved[0].merged_sources
        );
        assert!(
            resolved[0]
                .merged_sources
                .iter()
                .any(|src| src == "doc.alpha"),
            "first tie partner must be recorded: {:?}",
            resolved[0].merged_sources
        );
        assert!(
            resolved[0]
                .merged_sources
                .iter()
                .any(|src| src == "doc.beta"),
            "second tie partner must be recorded: {:?}",
            resolved[0].merged_sources
        );
    }

    /// Positive control for `counting_removals`. A non-exact winner widens the
    /// slot it took, so it must enter overlap removal; without this, the two
    /// zero-call assertions below would also pass on a counter that never
    /// increments.
    #[test]
    fn non_exact_winner_enters_overlap_removal() {
        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates(vec![
                candidate(0..6, PiiClass::Name, 0.70, "ner"),
                candidate(3..12, PiiClass::Email, 0.80, "regex"),
            ])
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 3..12);
        assert_eq!(removal_calls, 1);
    }

    /// An exact-span family tie installs a span identical to the one it
    /// evicted, so no other resolved candidate can overlap it and overlap
    /// removal is dead work. Skipping it is invisible in the output, so assert
    /// on the entry count itself (see `REMOVE_OVERLAPS_CALLS`).
    #[test]
    fn exact_span_family_tie_skips_overlap_removal() {
        let registry = tenant_document_registry();

        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates_with_policy(
                vec![
                    candidate(
                        0..5,
                        PiiClass::custom("tenant-doc").expect("valid custom class"),
                        0.70,
                        "doc.alpha",
                    ),
                    candidate(
                        0..5,
                        PiiClass::custom("tenant-doc").expect("valid custom class"),
                        0.70,
                        "doc.beta",
                    ),
                ],
                registry.family_policy(),
            )
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            removal_calls, 0,
            "an exact-span family tie must not enter overlap removal"
        );
    }

    /// The same guard on the other exact-span path: a collision-policy win
    /// replaces the slot span-for-span, so it must not enter overlap removal
    /// either.
    #[test]
    fn exact_span_candidate_win_skips_overlap_removal() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban",
                crate::CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
            )
            .build();

        let (resolved, removal_calls) = counting_removals(|| {
            resolve_candidates_with_policy(
                vec![
                    candidate(0..5, PiiClass::Email, 0.70, "pan"),
                    candidate(
                        0..5,
                        PiiClass::custom("iban").expect("valid custom class"),
                        0.70,
                        "iban",
                    ),
                ],
                registry.family_policy(),
            )
        });

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban");
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            removal_calls, 0,
            "an exact-span collision-policy win must not enter overlap removal"
        );
    }

    #[test]
    fn rule_priority_beats_score_when_class_ties() {
        let mut low_priority = candidate(0..5, PiiClass::Email, 0.99, "low");
        low_priority.priority = 1;
        let mut high_priority = candidate(0..5, PiiClass::Email, 0.70, "high");
        high_priority.priority = 2;

        let resolved = resolve_candidates(vec![low_priority, high_priority]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "high+low");
    }

    #[test]
    fn same_class_containment_never_loses_covered_bytes_to_validator_or_score() {
        let mut validated = candidate(0..5, PiiClass::Email, 0.99, "validator");
        validated.canonical_form = Some("canonical".to_string());
        let resolved = resolve_candidates(vec![
            candidate(0..10, PiiClass::Email, 0.50, "container"),
            validated,
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "container");
        assert_eq!(resolved[0].span, 0..10);
        assert_eq!(resolved[0].decided_by, ConflictTier::SameClassContainment);
    }

    #[test]
    fn same_class_containment_precedes_declared_evidence_tiers() {
        for (outer_evidence, inner_evidence) in [
            (EvidenceKind::Rule, EvidenceKind::Learned),
            (EvidenceKind::Learned, EvidenceKind::Rule),
        ] {
            let outer =
                candidate(0..10, PiiClass::Email, 0.50, "outer").with_evidence(outer_evidence);
            let mut inner =
                candidate(2..8, PiiClass::Email, 0.99, "inner").with_evidence(inner_evidence);
            inner.canonical_form = Some("validated".into());
            for candidates in [
                vec![outer.clone(), inner.clone()],
                vec![inner.clone(), outer.clone()],
            ] {
                let resolved = resolve_candidates(candidates);
                assert_eq!(resolved.len(), 1);
                assert_eq!(resolved[0].recognizer_id, "outer");
                assert_eq!(resolved[0].span, 0..10);
                assert_eq!(resolved[0].decided_by, ConflictTier::SameClassContainment);
                assert!(resolved[0].merged_sources.contains(&"inner".to_string()));
            }
        }
    }

    #[test]
    fn same_class_containment_never_reduces_prior_protected_bytes() {
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }
        fn covered(selected: &[Range<usize>], prior: &[Range<usize>]) -> bool {
            prior.iter().all(|span| {
                (span.start..span.end)
                    .all(|byte| selected.iter().any(|candidate| candidate.contains(&byte)))
            })
        }

        let mut seed = 0x5eed_2026_0727_3880u64;
        let mut nested_pools = 0usize;
        let mut unsafe_without_guard = 0usize;
        for count in 2..=5 {
            for case in 0..10_000 {
                let mut input = Vec::with_capacity(count);
                for index in 0..count {
                    let start = (next(&mut seed) % 20) as usize;
                    let end = start + 1 + (next(&mut seed) % 12) as usize;
                    let class = match next(&mut seed) % 3 {
                        0 => PiiClass::Email,
                        1 => PiiClass::Name,
                        _ => PiiClass::custom("postal_code").unwrap(),
                    };
                    let mut item = candidate(
                        start..end,
                        class,
                        [0.4, 0.6, 0.8, 0.99][(next(&mut seed) % 4) as usize],
                        &format!("r{case}_{index}"),
                    );
                    item.priority = (next(&mut seed) % 5) as i32 - 2;
                    if next(&mut seed).is_multiple_of(4) {
                        item.canonical_form = Some("synthetic".into());
                    }
                    item.evidence = if next(&mut seed).is_multiple_of(2) {
                        EvidenceKind::Rule
                    } else {
                        EvidenceKind::Learned
                    };
                    input.push(item);
                }
                if input.iter().enumerate().any(|(i, a)| {
                    input.iter().skip(i + 1).any(|b| {
                        a.class == b.class
                            && a.span != b.span
                            && (contains(&a.span, &b.span) || contains(&b.span, &a.span))
                    })
                }) {
                    nested_pools += 1;
                }
                let mut prior_pool = CandidatePool::new(input.clone());
                let order = prior_pool.order.clone();
                let prior = prior_pool.resolve_with_mode(
                    &order,
                    &FamilyPolicyTable::EMPTY,
                    None,
                    ContainmentMode::Prior,
                );
                let prior_spans = prior
                    .iter()
                    .map(|node| node.candidate.span.clone())
                    .collect::<Vec<_>>();
                let mut raw_pool = CandidatePool::new(input.clone());
                let order = raw_pool.order.clone();
                let unguarded = raw_pool.resolve_with_mode(
                    &order,
                    &FamilyPolicyTable::EMPTY,
                    None,
                    ContainmentMode::Enclosing,
                );
                let unguarded_spans = unguarded
                    .iter()
                    .map(|node| node.candidate.span.clone())
                    .collect::<Vec<_>>();
                if !covered(&unguarded_spans, &prior_spans) {
                    unsafe_without_guard += 1;
                }
                let selected = resolve_candidates(input);
                let selected_spans = selected
                    .iter()
                    .map(|node| node.span.clone())
                    .collect::<Vec<_>>();
                assert!(
                    covered(&selected_spans, &prior_spans),
                    "pool size {count}, case {case}: {prior_spans:?} -> {selected_spans:?}"
                );
            }
        }
        eprintln!("checked 40000 pools, {nested_pools} had strict same-class nesting, {unsafe_without_guard} needed the coverage safeguard");
        assert!(nested_pools > 100);
        assert!(unsafe_without_guard > 0);
    }

    #[test]
    fn partial_overlap_prefers_higher_confidence() {
        let resolved = resolve_candidates(vec![
            candidate(0..6, PiiClass::Name, 0.70, "ner"),
            candidate(3..12, PiiClass::Email, 0.80, "regex"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    #[test]
    fn multi_overlap_replacement_leaves_disjoint_set() {
        let resolved = resolve_candidates(vec![
            candidate(0..5, PiiClass::Location, 0.70, "a"),
            candidate(3..8, PiiClass::Name, 0.70, "b"),
            candidate(0..10, PiiClass::Email, 0.70, "c"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..10);
        assert_eq!(resolved[0].class, PiiClass::Email);
    }

    /// The anchor short-circuit must not label an overlap the base ladder
    /// decided. Here the anchored incumbent wins on Score; the audit row has to
    /// say Score, not AnchoredContext.
    #[test]
    fn anchored_incumbent_that_wins_on_score_is_labelled_score() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(LocaleTag::DeDe, "iban", vec!["IBAN".to_string()], None);

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(
                    6..10,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.90,
                    "iban.structural",
                ),
                candidate(
                    6..10,
                    PiiClass::custom("digits").expect("valid custom class"),
                    0.50,
                    "digits.generic",
                ),
            ],
            registry.family_policy(),
            &anchors,
            "IBAN: DE89",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(
            resolved[0].class,
            PiiClass::custom("iban").expect("valid custom class")
        );
        assert_eq!(
            resolved[0].merged_sources,
            vec!["digits.generic".to_string()]
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::Score);
    }

    /// Same mislabel through the partial-overlap path.
    #[test]
    fn anchored_incumbent_that_wins_partial_overlap_on_score_is_labelled_score() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(LocaleTag::DeDe, "iban", vec!["IBAN".to_string()], None);

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(
                    6..10,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.90,
                    "iban.structural",
                ),
                candidate(
                    8..12,
                    PiiClass::custom("digits").expect("valid custom class"),
                    0.50,
                    "digits.generic",
                ),
            ],
            registry.family_policy(),
            &anchors,
            "IBAN: DE8912",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].span, 6..10);
        assert_eq!(resolved[0].decided_by, ConflictTier::Score);
    }

    /// When an anchored recognizer's mandatory anchor is `Missing`, the
    /// winner is rebuilt as the family-level fallback candidate. A rival that
    /// lost to the winner on the base ladder *before* the fallback fired must
    /// survive in `merged_sources` — the audit trail derives loser accounting
    /// exclusively from `merged_sources`, so a reset here would drop the actual
    /// loser from `conflict_loser` rows and `AmbiguityRecord.losing_candidates`
    /// and mis-attribute the loser slot to the original winner's recognizer id.
    #[test]
    fn missing_anchor_fallback_preserves_prior_loser_in_merged_sources() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        // No anchor cue bundle registered → anchor resolves to `Missing`.
        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                candidate(
                    6..10,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.90,
                    "iban.structural",
                ),
                candidate(
                    6..10,
                    PiiClass::custom("digits").expect("valid custom class"),
                    0.50,
                    "digits.generic",
                ),
            ],
            registry.family_policy(),
            &AnchorResolver::default(),
            "field DE89",
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:payment-card-or-iban"
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::AnchoredContext);
        assert!(
            resolved[0]
                .merged_sources
                .iter()
                .any(|src| src == "digits.generic"),
            "prior rival that lost on score must survive the fallback: {:?}",
            resolved[0].merged_sources
        );
        assert!(
            resolved[0]
                .merged_sources
                .iter()
                .any(|src| src == "iban.structural"),
            "the demoted original variant must still be recorded: {:?}",
            resolved[0].merged_sources
        );
    }

    /// When the ladder is exhausted (identical recognizer id, class, priority,
    /// score, and length) the incumbent keeps the slot; the row must carry the
    /// terminal rung, not whatever tier an earlier overlap stamped.
    #[test]
    fn fully_tied_partial_overlap_never_keeps_stale_tier() {
        let resolved = resolve_candidates(vec![
            candidate(0..4, PiiClass::Name, 0.70, "dict"),
            candidate(2..6, PiiClass::Name, 0.70, "dict"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..4);
        assert_eq!(resolved[0].merged_sources, vec!["dict".to_string()]);
        assert_eq!(resolved[0].decided_by, ConflictTier::RecognizerId);
    }

    fn prioritized(mut candidate: Candidate, priority: i32) -> Candidate {
        candidate.priority = priority;
        candidate
    }

    /// An NER organisation token strictly inside a rule-recognised URL: the
    /// container keeps the slot whichever candidate arrives first, the
    /// enclosed span is recorded as a merged source, and the rung that
    /// decided it is named truthfully: containment precedence, a pattern-tier
    /// container over a learned-tier sub-token. Before the containment rungs
    /// existed the enclosed builtin span won on `ClassPriority` and
    /// `remove_overlaps` dropped the whole URL, leaving its head and tail raw
    /// around a mid-word token.
    #[test]
    fn builtin_sub_span_does_not_evict_custom_container() {
        for container_first in [true, false] {
            let container = prioritized(
                candidate(
                    0..24,
                    PiiClass::custom("url").expect("valid custom class"),
                    0.80,
                    "url.anchored",
                ),
                90,
            );
            let enclosed = candidate(12..16, PiiClass::Organization, 0.99, "ner");
            let input = if container_first {
                vec![container, enclosed]
            } else {
                vec![enclosed, container]
            };

            let resolved = resolve_candidates(input);

            assert_eq!(resolved.len(), 1, "container_first={container_first}");
            assert_eq!(resolved[0].span, 0..24);
            assert_eq!(
                resolved[0].class,
                PiiClass::custom("url").expect("valid custom class")
            );
            assert_eq!(resolved[0].recognizer_id, "url.anchored");
            assert_eq!(resolved[0].decided_by, ConflictTier::ContainmentPrecedence);
            assert_eq!(resolved[0].merged_sources, vec!["ner".to_string()]);
        }
    }

    /// Two builtin sub-spans of different classes inside one structured span
    /// converge to the container alone (multi-overlap fixed point).
    #[test]
    fn several_builtin_sub_spans_collapse_into_the_custom_container() {
        let resolved = resolve_candidates(vec![
            candidate(12..16, PiiClass::Organization, 0.99, "ner"),
            prioritized(
                candidate(
                    0..30,
                    PiiClass::custom("url").expect("valid custom class"),
                    0.80,
                    "url.anchored",
                ),
                90,
            ),
            candidate(20..26, PiiClass::Name, 0.97, "ner"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 0..30);
        assert_eq!(
            resolved[0].class,
            PiiClass::custom("url").expect("valid custom class")
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::ContainmentPrecedence);
    }

    /// The structured-containment rung still names the case the guard
    /// refuses: a plain custom container over a validator-backed builtin
    /// sub-span (a URL regex over an RFC-validated email) keeps the slot for
    /// the todo #3025 reason, and the row says `StructuredContainment`.
    #[test]
    fn structured_containment_still_names_a_guard_refused_builtin_sub_span() {
        for container_first in [true, false] {
            let container = prioritized(
                candidate(
                    0..24,
                    PiiClass::custom("url").expect("valid custom class"),
                    0.80,
                    "url.anchored",
                ),
                85,
            );
            let mut enclosed =
                prioritized(candidate(8..20, PiiClass::Email, 0.99, "email.global"), 90);
            enclosed.canonical_form = Some("a@b.invalid".into());
            let input = if container_first {
                vec![container, enclosed]
            } else {
                vec![enclosed, container]
            };

            let resolved = resolve_candidates(input);

            assert_eq!(resolved.len(), 1, "container_first={container_first}");
            assert_eq!(resolved[0].recognizer_id, "url.anchored");
            assert_eq!(resolved[0].decided_by, ConflictTier::StructuredContainment);
            assert_eq!(resolved[0].merged_sources, vec!["email.global".to_string()]);
        }
    }

    /// Scope pin: the rung is containment-only. A builtin span that merely
    /// straddles the structured span's edge is still decided by the base
    /// ladder (class priority), exactly as before.
    #[test]
    fn partial_overlap_with_a_custom_span_still_uses_class_priority() {
        let resolved = resolve_candidates(vec![
            prioritized(
                candidate(
                    0..24,
                    PiiClass::custom("url").expect("valid custom class"),
                    0.80,
                    "url.anchored",
                ),
                90,
            ),
            candidate(20..30, PiiClass::Organization, 0.99, "ner"),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Organization);
        assert_eq!(resolved[0].decided_by, ConflictTier::ClassPriority);
    }

    /// Containment precedence (todo #3740): a custom span that wholly
    /// contains a differently-classed custom span wins the whole span as one
    /// token when its evidence tier is at least the contained span's, and
    /// the rung, not the base ladder, is named on the row.
    #[test]
    fn custom_inside_custom_containment_goes_to_the_container() {
        for container_first in [true, false] {
            let container = prioritized(
                candidate(
                    0..13,
                    PiiClass::custom("tax_number").expect("valid custom class"),
                    0.85,
                    "tax_number.cue_anchored",
                ),
                84,
            );
            let enclosed = prioritized(
                candidate(
                    8..13,
                    PiiClass::custom("postal_code").expect("valid custom class"),
                    0.80,
                    "postal.de",
                ),
                70,
            );
            let input = if container_first {
                vec![container, enclosed]
            } else {
                vec![enclosed, container]
            };

            let resolved = resolve_candidates(input);

            assert_eq!(resolved.len(), 1, "container_first={container_first}");
            assert_eq!(resolved[0].span, 0..13);
            assert_eq!(resolved[0].recognizer_id, "tax_number.cue_anchored");
            assert_eq!(resolved[0].decided_by, ConflictTier::ContainmentPrecedence);
            assert_eq!(resolved[0].merged_sources, vec!["postal.de".to_string()]);
        }
    }

    /// Equal certainty goes to the container. The reference letter: a
    /// validator-backed phone inside a validator-backed IBAN, the phone with
    /// the higher rule priority and score. Breaking the tie by either hands
    /// the middle of the IBAN to the phone and fragments one entity into
    /// five tokens.
    #[test]
    fn a_tie_in_evidence_tier_goes_to_the_container() {
        for container_first in [true, false] {
            let mut container = prioritized(
                candidate(
                    5..39,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.80,
                    "iban.structural",
                ),
                80,
            );
            container.canonical_form = Some("PL56094289817280566322004500".into());
            let mut enclosed = prioritized(
                candidate(
                    10..24,
                    PiiClass::custom("phone").expect("valid custom class"),
                    0.95,
                    "phone.national.de",
                ),
                85,
            );
            enclosed.canonical_form = Some("+49942898172805".into());
            let input = if container_first {
                vec![container, enclosed]
            } else {
                vec![enclosed, container]
            };

            let resolved = resolve_candidates(input);

            assert_eq!(resolved.len(), 1, "container_first={container_first}");
            assert_eq!(resolved[0].span, 5..39);
            assert_eq!(resolved[0].recognizer_id, "iban.structural");
            assert_eq!(resolved[0].decided_by, ConflictTier::ContainmentPrecedence);
            assert_eq!(
                resolved[0].merged_sources,
                vec!["phone.national.de".to_string()]
            );
        }
    }

    /// The guard: a container less certain than what it would swallow falls
    /// through to today's rungs. A plain-regex JSON field over a validated
    /// phone with the higher rule priority loses on the base ladder, so the
    /// identifier keeps its own class and token; the field's remainder is
    /// residual coverage's job.
    #[test]
    fn a_less_certain_container_does_not_swallow_a_validated_identifier() {
        let container = prioritized(
            candidate(
                0..30,
                PiiClass::custom("json_field").expect("valid custom class"),
                0.90,
                "json.field",
            ),
            50,
        );
        let mut enclosed = prioritized(
            candidate(
                11..25,
                PiiClass::custom("phone").expect("valid custom class"),
                0.80,
                "phone.e164",
            ),
            80,
        );
        enclosed.canonical_form = Some("+49301234567".into());

        let resolved = resolve_candidates(vec![container, enclosed]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 11..25);
        assert_eq!(resolved[0].recognizer_id, "phone.e164");
        assert_eq!(resolved[0].decided_by, ConflictTier::RulePriority);
    }

    /// A learned (NER-tier) container ranks below a plain regex, so the base
    /// ladder decides: here the regex's higher rule priority wins and the
    /// learned span is evicted. With a higher priority the learned container
    /// still wins, but on `RulePriority`, never on containment.
    #[test]
    fn a_learned_container_ranks_below_a_plain_regex() {
        for (container_priority, expected_id, expected_tier) in [
            (50, "postal.de", ConflictTier::RulePriority),
            (90, "ner", ConflictTier::RulePriority),
        ] {
            let mut container = prioritized(
                candidate(
                    0..30,
                    PiiClass::custom("blob").expect("valid custom class"),
                    0.99,
                    "ner",
                ),
                container_priority,
            );
            container.source = "ner/bert".into();
            let enclosed = prioritized(
                candidate(
                    10..15,
                    PiiClass::custom("postal_code").expect("valid custom class"),
                    0.80,
                    "postal.de",
                ),
                70,
            );

            let resolved = resolve_candidates(vec![container, enclosed]);

            assert_eq!(resolved.len(), 1, "priority={container_priority}");
            assert_eq!(resolved[0].recognizer_id, expected_id);
            assert_eq!(resolved[0].decided_by, expected_tier);
        }
    }

    /// An anchored or cue-structured match (`structural.*` source) outranks a
    /// plain regex and yields to a validated identifier.
    #[test]
    fn a_structural_container_ranks_between_pattern_and_validated() {
        let structural = || {
            let mut container = prioritized(
                candidate(
                    0..20,
                    PiiClass::custom("recipient").expect("valid custom class"),
                    0.85,
                    "name.forward_marker",
                ),
                10,
            );
            container.source = "structural.forward_marker".into();
            container
        };
        let pattern = prioritized(
            candidate(
                5..10,
                PiiClass::custom("postal_code").expect("valid custom class"),
                0.80,
                "postal.de",
            ),
            70,
        );
        let resolved = resolve_candidates(vec![structural(), pattern]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "name.forward_marker");
        assert_eq!(resolved[0].decided_by, ConflictTier::ContainmentPrecedence);

        let mut validated = prioritized(
            candidate(
                5..10,
                PiiClass::custom("iban").expect("valid custom class"),
                0.80,
                "iban.structural",
            ),
            70,
        );
        validated.canonical_form = Some("DE89".into());
        let resolved = resolve_candidates(vec![structural(), validated]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].decided_by, ConflictTier::RulePriority);
    }

    /// The anchored tier also counts a mandatory anchor found in context. Every
    /// bundled anchored rule is validator-backed, so this is how an adopter's
    /// anchored rule without a validator reaches it. With its cue present the
    /// container ties a cue-structured (`structural.*`) span inside it and
    /// keeps the whole span; without the cue it is a plain pattern, the guard
    /// refuses, and the base ladder hands the span to the inner span's higher
    /// rule priority.
    #[test]
    fn a_found_mandatory_anchor_ranks_a_container_as_anchored() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "contract.anchored",
                crate::CollisionMembership::new(
                    "tenant-document",
                    "contract",
                    10,
                    Some("contract".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(
            LocaleTag::DeDe,
            "contract",
            vec!["Vertrag".to_string()],
            None,
        );
        for (input, expected_id, expected_tier) in [
            (
                "Vertrag: AB-1234-XYZ-7788 end",
                "contract.anchored",
                ConflictTier::ContainmentPrecedence,
            ),
            (
                "Notiz:   AB-1234-XYZ-7788 end",
                "name.forward_marker",
                ConflictTier::RulePriority,
            ),
        ] {
            let container = prioritized(
                candidate(
                    9..25,
                    PiiClass::custom("contract").expect("valid custom class"),
                    0.80,
                    "contract.anchored",
                ),
                10,
            );
            let mut inner = prioritized(
                candidate(
                    12..20,
                    PiiClass::custom("recipient").expect("valid custom class"),
                    0.85,
                    "name.forward_marker",
                ),
                90,
            );
            inner.source = "structural.forward_marker".into();

            let resolved = resolve_candidates_with_policy_and_anchors(
                vec![container, inner],
                registry.family_policy(),
                &anchors,
                input,
                &[LocaleTag::DeDe],
            );

            assert_eq!(resolved.len(), 1, "{input}");
            assert_eq!(resolved[0].recognizer_id, expected_id, "{input}");
            assert_eq!(resolved[0].decided_by, expected_tier, "{input}");
        }
    }

    /// Placement: collision-family policy decides a declared rivalry before
    /// containment does. A lower-precedence family member that wholly
    /// contains a higher-precedence rival still loses to it on
    /// `CollisionPolicy`; with the rung placed before family policy the
    /// container would win and the family label would change.
    #[test]
    fn family_policy_decides_before_containment_precedence() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "pan.structural",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new("payment-card-or-iban", "iban", 10, None),
            )
            .build();

        let resolved = resolve_candidates_with_policy(
            vec![
                candidate(
                    0..12,
                    PiiClass::custom("pan").expect("valid custom class"),
                    0.90,
                    "pan.structural",
                ),
                candidate(
                    2..10,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.70,
                    "iban.structural",
                ),
            ],
            registry.family_policy(),
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 2..10);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].decided_by, ConflictTier::CollisionPolicy);
    }

    /// Placement: an anchored incoming candidate takes the slot from a plain
    /// container on the anchor rung; containment precedence never sees the
    /// pair, so the container cannot swallow an anchored identifier.
    #[test]
    fn anchor_rung_decides_before_containment_precedence() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .build();
        let mut anchors = AnchorResolver::default();
        anchors.register(LocaleTag::DeDe, "iban", vec!["IBAN".to_string()], None);
        let input = "IBAN: DE89 3704 0044 0532 0130 00 end";

        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                prioritized(
                    candidate(
                        0..input.len(),
                        PiiClass::custom("json_field").expect("valid custom class"),
                        0.90,
                        "json.field",
                    ),
                    90,
                ),
                candidate(
                    6..33,
                    PiiClass::custom("iban").expect("valid custom class"),
                    0.70,
                    "iban.structural",
                ),
            ],
            registry.family_policy(),
            &anchors,
            input,
            &[LocaleTag::DeDe],
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].span, 6..33);
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(
            resolved[0].class,
            PiiClass::custom("iban").expect("valid custom class")
        );
        assert_eq!(resolved[0].decided_by, ConflictTier::AnchoredContext);
    }

    /// Scope pin: a builtin container over a custom sub-span already won on
    /// class priority; the new rung must not relabel that decision.
    #[test]
    fn builtin_container_over_custom_sub_span_is_still_class_priority() {
        let resolved = resolve_candidates(vec![
            candidate(0..20, PiiClass::Location, 0.95, "ner"),
            prioritized(
                candidate(
                    10..15,
                    PiiClass::custom("postal_code").expect("valid custom class"),
                    0.80,
                    "postal.de",
                ),
                70,
            ),
        ]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].class, PiiClass::Location);
        assert_eq!(resolved[0].decided_by, ConflictTier::ClassPriority);
    }

    /// The shipped IBAN / card rivalry: IBAN outranks the card variant and
    /// requires the `iban` anchor, the card variant has none.
    fn payment_family_registry() -> crate::RecognizerRegistry {
        crate::RecognizerRegistry::builder()
            .register_collision(
                "iban.structural",
                crate::CollisionMembership::new(
                    "payment-card-or-iban",
                    "iban",
                    10,
                    Some("iban".to_string()),
                ),
            )
            .register_collision(
                "card.structural",
                crate::CollisionMembership::new("payment-card-or-iban", "pan", 20, None),
            )
            .build()
    }

    const SETTLED_IBAN_INPUT: &str = "AT61 1904 3002 3457 3201 Kontoinhaber";

    fn iban_at(span: Range<usize>) -> Candidate {
        prioritized(
            candidate(
                span,
                PiiClass::custom("iban").expect("valid custom class"),
                0.90,
                "iban.structural",
            ),
            10,
        )
    }

    fn card_at(span: Range<usize>) -> Candidate {
        prioritized(
            candidate(
                span,
                PiiClass::custom("credit_card").expect("valid custom class"),
                0.90,
                "card.structural",
            ),
            10,
        )
    }

    /// A lower-priority rival outside the family: the shape postal.at_ch
    /// produces on an IBAN's last group plus the following capitalised word.
    fn foreign_postal_at(span: Range<usize>) -> Candidate {
        prioritized(
            candidate(
                span,
                PiiClass::custom("postal_code").expect("valid custom class"),
                0.90,
                "postal.at_ch",
            ),
            5,
        )
    }

    /// Todo #3709: once collision policy has settled the family (IBAN beat
    /// the card variant), a later unrelated overlap decided on the base ladder
    /// must not reopen the anchor check. Before the fix the ladder rung
    /// overwrote `decided_by`, the fallback keyed on it, and the settled IBAN
    /// became the family token (raw under a tokenize-iban + preserve policy).
    #[test]
    fn collision_settled_family_survives_later_foreign_overlap() {
        let registry = payment_family_registry();
        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![iban_at(0..24), card_at(5..24), foreign_postal_at(20..37)],
            registry.family_policy(),
            &AnchorResolver::default(),
            SETTLED_IBAN_INPUT,
            &[LocaleTag::DeAt],
        );

        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].class, PiiClass::Custom("iban".to_string()));
        // `decided_by` stays the audit label of the last rung that touched
        // the span; the settled state lives beside it, not in it.
        assert_eq!(resolved[0].decided_by, ConflictTier::RulePriority);
        assert!(resolved[0]
            .merged_sources
            .iter()
            .any(|source| source == "postal.at_ch"));
    }

    /// Order independence: the verdict for the settled IBAN is the same with
    /// or without the unrelated overlap.
    #[test]
    fn collision_settled_verdict_does_not_depend_on_unrelated_overlaps() {
        let registry = payment_family_registry();
        let resolve = |candidates| {
            resolve_candidates_with_policy_and_anchors(
                candidates,
                registry.family_policy(),
                &AnchorResolver::default(),
                SETTLED_IBAN_INPUT,
                &[LocaleTag::DeAt],
            )
        };
        let without = resolve(vec![iban_at(0..24), card_at(5..24)]);
        let with = resolve(vec![
            iban_at(0..24),
            card_at(5..24),
            foreign_postal_at(20..37),
        ]);

        assert_eq!(without[0].decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(with[0].class, without[0].class);
        assert_eq!(with[0].recognizer_id, without[0].recognizer_id);
        assert_eq!(with[0].token_family, without[0].token_family);
    }

    /// The precedence-tie family token is settled by construction, not by the
    /// accident that its synthetic id has no family membership today. The
    /// public builder accepts any recognizer id, so an id that shadows the
    /// token (`collision-family:<f>`) with a mandatory anchor must not pull
    /// the token back through the missing-anchor fallback.
    #[test]
    fn precedence_tie_token_never_reenters_anchor_fallback() {
        let registry = crate::RecognizerRegistry::builder()
            .register_collision(
                "doc.alpha",
                crate::CollisionMembership::new("tenant-document", "alpha", 10, None),
            )
            .register_collision(
                "doc.beta",
                crate::CollisionMembership::new("tenant-document", "beta", 10, None),
            )
            .register_collision(
                "collision-family:tenant-document",
                crate::CollisionMembership::new("shadow", "token", 10, Some("shadow".to_string())),
            )
            .build();
        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![
                prioritized(
                    candidate(0..5, PiiClass::custom("alpha").unwrap(), 0.70, "doc.alpha"),
                    10,
                ),
                prioritized(
                    candidate(0..5, PiiClass::custom("beta").unwrap(), 0.70, "doc.beta"),
                    10,
                ),
                foreign_postal_at(3..10),
            ],
            registry.family_policy(),
            &AnchorResolver::default(),
            "AB123 Wiener",
            &[LocaleTag::DeAt],
        );

        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].class, PiiClass::family("tenant-document"));
        assert_eq!(
            resolved[0].recognizer_id,
            "collision-family:tenant-document"
        );
    }

    /// Axis-4 invariant behind #3709: the survivor must not depend on the
    /// order the three overlapping candidates arrive in. `candidate_order`
    /// fixes the order in production, so feed every permutation of ids to the
    /// pool directly; only `decided_by` (the last-rung audit label) may vary.
    #[test]
    fn collision_settled_survivor_is_arrival_order_independent() {
        let registry = payment_family_registry();
        let originals = vec![iban_at(0..24), card_at(5..24), foreign_postal_at(20..37)];
        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let resolver = AnchorResolver::default();
        let survivors = permutations.map(|ids| {
            CandidatePool::new(originals.clone())
                .resolve(
                    &ids,
                    registry.family_policy(),
                    Some((&resolver, SETTLED_IBAN_INPUT, &[LocaleTag::DeAt])),
                )
                .into_iter()
                .map(|node| {
                    let c = node.candidate;
                    (c.span, c.class, c.recognizer_id, c.token_family)
                })
                .collect::<Vec<_>>()
        });

        assert_eq!(survivors[0].len(), 1, "{survivors:?}");
        assert_eq!(survivors[0][0].1, PiiClass::Custom("iban".to_string()));
        for (ids, survivor) in permutations.iter().zip(&survivors) {
            assert_eq!(survivor, &survivors[0], "arrival order {ids:?}");
        }
    }

    /// Same defect when the IBAN arrives second and takes the slot from the
    /// card variant by collision policy (`CandidateWins`), then the unrelated
    /// overlap arrives.
    #[test]
    fn incoming_collision_policy_winner_stays_settled() {
        let registry = payment_family_registry();
        let resolved = resolve_candidates_with_policy_and_anchors(
            vec![card_at(0..10), iban_at(5..24), foreign_postal_at(20..37)],
            registry.family_policy(),
            &AnchorResolver::default(),
            SETTLED_IBAN_INPUT,
            &[LocaleTag::DeAt],
        );

        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].recognizer_id, "iban.structural");
        assert_eq!(resolved[0].class, PiiClass::Custom("iban".to_string()));
        assert_eq!(resolved[0].decided_by, ConflictTier::RulePriority);
    }

    /// Negative control: an IBAN whose family was never settled (no card
    /// rival) still takes the missing-anchor fallback, with or without the
    /// unrelated overlap. The fix must not widen the skip.
    #[test]
    fn unsettled_iban_without_anchor_still_takes_family_fallback() {
        let registry = payment_family_registry();
        for candidates in [
            vec![iban_at(0..24)],
            vec![iban_at(0..24), foreign_postal_at(20..37)],
        ] {
            let resolved = resolve_candidates_with_policy_and_anchors(
                candidates,
                registry.family_policy(),
                &AnchorResolver::default(),
                SETTLED_IBAN_INPUT,
                &[LocaleTag::DeAt],
            );
            assert_eq!(resolved.len(), 1, "{resolved:?}");
            assert_eq!(
                resolved[0].class,
                PiiClass::family("payment-card-or-iban"),
                "{resolved:?}"
            );
            assert_eq!(resolved[0].decided_by, ConflictTier::AnchoredContext);
        }
    }
}

#[cfg(test)]
mod recovery_event_tests {
    use super::*;
    /// `covers_all_spans` resumes its scan instead of restarting it for each
    /// prior span (todo 3895). Pin it against the restarting original on
    /// random sorted, non-overlapping span lists, including empty spans.
    #[test]
    fn covers_all_spans_matches_the_restarting_scan() {
        fn restarting(selected: &[WholeCandidate], prior: &[WholeCandidate]) -> bool {
            prior.iter().all(|prior_node| {
                let mut cursor = prior_node.candidate.span.start;
                for selected_node in selected {
                    let span = &selected_node.candidate.span;
                    if span.end <= cursor {
                        continue;
                    }
                    if span.start > cursor {
                        break;
                    }
                    cursor = cursor.max(span.end);
                    if cursor >= prior_node.candidate.span.end {
                        return true;
                    }
                }
                false
            })
        }
        fn next(state: &mut u64) -> u64 {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            *state
        }
        fn spans(state: &mut u64) -> Vec<WholeCandidate> {
            let mut at = 0;
            (0..next(state) % 8)
                .map(|id| {
                    let start = at + (next(state) % 3) as usize;
                    let end = start + (next(state) % 4) as usize;
                    at = end;
                    WholeCandidate {
                        candidate: Candidate::new(
                            start..end,
                            PiiClass::Email,
                            "span",
                            0.5,
                            0,
                            None,
                            "span",
                            "span",
                            ConflictTier::None,
                            vec![],
                        ),
                        members: vec![id as usize],
                        node: id as usize,
                        settlement: Settlement::Open,
                    }
                })
                .collect()
        }
        let mut state = 0x3895_u64;
        let (mut covered, mut uncovered) = (0, 0);
        for _ in 0..20_000 {
            let selected = spans(&mut state);
            let prior = spans(&mut state);
            let expected = restarting(&selected, &prior);
            assert_eq!(covers_all_spans(&selected, &prior), expected);
            if expected {
                covered += 1;
            } else {
                uncovered += 1;
            }
        }
        assert!(
            covered > 1_000 && uncovered > 1_000,
            "{covered} / {uncovered}"
        );
    }

    fn whole(pool: &CandidatePool, id: usize) -> WholeCandidate {
        WholeCandidate {
            candidate: pool.originals[id].clone(),
            members: vec![id],
            node: id,
            settlement: Settlement::Open,
        }
    }

    fn counter(span: Range<usize>, class: PiiClass, id: &str) -> Candidate {
        Candidate::new(
            span,
            class,
            id,
            0.9,
            0,
            None,
            "counter",
            id,
            ConflictTier::None,
            vec![],
        )
    }

    /// Collateral losers leave in arrival (slot) order, not in the reverse
    /// start order the index yields them in, so `Collateral` events and the
    /// winner's `merged_sources` keep the order the full scan produced.
    #[test]
    fn collateral_losers_leave_in_arrival_order() {
        // Slot order 20..25, 0..5, 10..15, 6..8 differs from start order.
        let mut pool = CandidatePool::new(vec![
            counter(20..25, PiiClass::Name, "first"),
            counter(0..5, PiiClass::Name, "a"),
            counter(10..15, PiiClass::Name, "c"),
            counter(6..8, PiiClass::Name, "b"),
            counter(3..24, PiiClass::Email, "winner"),
        ]);
        let mut nodes = ResolvedSet::default();
        for id in 0..4 {
            nodes.push(whole(&pool, id));
        }
        let incoming = whole(&pool, 4);
        pool.insert(
            &mut nodes,
            incoming,
            &FamilyPolicyTable::EMPTY,
            None,
            ContainmentMode::Enclosing,
        );
        let nodes = nodes.into_vec();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].candidate.span, 3..24);
        let removed = pool
            .events
            .iter()
            .filter_map(|event| match event {
                ResolutionEvent::Collateral { removed, .. } => Some(*removed),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(removed, vec![1, 2, 3]);
        let sources = (0..4)
            .map(|id| pool.originals[id].source.clone())
            .collect::<Vec<_>>();
        assert_eq!(nodes[0].candidate.merged_sources, sources);
    }

    /// A span that would overlap an indexed one switches the set to the full
    /// scan instead of dropping the span from the index (fail closed, no
    /// panic in release builds).
    #[test]
    fn overlapping_put_falls_back_to_the_full_scan() {
        let pool = CandidatePool::new(vec![
            counter(0..10, PiiClass::Name, "a"),
            counter(0..4, PiiClass::Name, "same-start"),
            counter(8..12, PiiClass::Name, "partial"),
        ]);
        let mut nodes = ResolvedSet::default();
        nodes.push(whole(&pool, 0));
        assert!(!nodes.linear);
        nodes.push(whole(&pool, 1));
        assert!(nodes.linear);
        nodes.push(whole(&pool, 2));
        let mut probes = 0;
        assert_eq!(nodes.overlapping(&(9..11), &mut probes), vec![0, 2]);
        assert_eq!(nodes.first_overlap(&(2..3), &mut probes), Some(0));
        assert_eq!(nodes.take(1).candidate.span, 0..4);
        assert_eq!(nodes.overlapping(&(0..20), &mut probes), vec![0, 2]);
        assert_eq!(nodes.into_vec().len(), 2);
    }

    #[test]
    fn collateral_removal_has_no_fabricated_pair_outcome_or_membership() {
        let make = |span, class, id| {
            Candidate::new(
                span,
                class,
                id,
                0.9,
                0,
                None,
                "counter",
                id,
                ConflictTier::None,
                vec![],
            )
        };
        let mut pool = CandidatePool::new(vec![
            make(0..5, PiiClass::Name, "a"),
            make(10..15, PiiClass::Name, "b"),
            make(3..12, PiiClass::Email, "c"),
        ]);
        let mut nodes = ResolvedSet::default();
        for id in 0..2 {
            nodes.push(WholeCandidate {
                candidate: pool.originals[id].clone(),
                members: vec![id],
                node: id,
                settlement: Settlement::Open,
            });
        }
        let incoming = WholeCandidate {
            candidate: pool.originals[2].clone(),
            members: vec![2],
            node: 2,
            settlement: Settlement::Open,
        };
        pool.insert(
            &mut nodes,
            incoming,
            &FamilyPolicyTable::EMPTY,
            None,
            ContainmentMode::Enclosing,
        );
        let nodes = nodes.into_vec();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].members, vec![2]);
        assert!(matches!(
            pool.events.as_slice(),
            [
                ResolutionEvent::Pair {
                    existing: 0,
                    incoming: 2,
                    result: 3,
                    outcome: PairOutcome::Incoming(ConflictTier::ClassPriority)
                },
                ResolutionEvent::Collateral {
                    removed: 1,
                    replacing: 3
                }
            ]
        ));
        assert_eq!(pool.originals[1].span, 10..15);
    }
}
