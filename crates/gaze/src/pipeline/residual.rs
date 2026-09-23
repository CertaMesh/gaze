//! Raw evidence coverage for the closed known-protective domain.
//!
//! Every byte that a candidate of a protected class claimed (its effective
//! class previews a protective action, `Action::is_protective`) and that no
//! protective selection covers becomes a residual cell of the highest-ranked
//! such claimant, per byte, even inside a `preserve` selection. Admission is
//! per original, never per overlap component, so a preserved or redacted
//! neighbour cannot switch a claimant's coverage off (todo #3740). A cell
//! emits under its claimant's own action: `tokenize` and `format_preserve`
//! mint a class token (a fragment has no format to preserve), `redact` writes
//! the one-way `[REDACTED:<class>]` marker, `generalize` the class placeholder.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cell {
    pub(super) raw: Range<usize>,
    /// Admitted originals active somewhere in the cell, rank order; the
    /// representative first.
    pub(super) parents: Vec<usize>,
    pub(super) representative: usize,
    pub(super) class: PiiClass,
    pub(super) family: String,
    /// What every emission of this cell must do: the representative's
    /// previewed action with `format_preserve` mapped to `tokenize`.
    pub(super) action: Action,
    /// Some byte of the cell lies inside a `preserve` selection, so protection
    /// beat preservation there; the audit row says so.
    pub(super) overrides_preserve: bool,
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Work {
    pub(super) preview_queries: usize,
    pub(super) endpoint_cells: usize,
    pub(super) active_parent_visits: usize,
}

pub(super) struct Plan {
    #[cfg(test)]
    pub(super) work: Work,
    pub(super) cells: Vec<Cell>,
    /// Per original: its effective class previews a protective action.
    pub(super) admitted: Vec<bool>,
    /// Per selection: `Some(true)` previews protective (covers its bytes and
    /// blocks the sweep), `Some(false)` previews `preserve` (cells may be
    /// placed inside it), `None` has no static preview and stays on the
    /// legacy path: it blocks, and its runtime action is not second-guessed.
    blocking: Vec<Option<bool>>,
}
impl Plan {
    /// The planner previewed every selection's action; where a preview
    /// exists the runtime verdict must agree on whether it protects, in both
    /// directions. A selection previewed protective that ships raw left its
    /// claimants uncovered; one previewed `preserve` that replaces bytes would
    /// collide with the cells placed inside it.
    pub(super) fn check_actual(&self, segment: &occurrence::Segment) -> Result<()> {
        if self.blocking.len() != segment.selections.len()
            || self
                .blocking
                .iter()
                .zip(&segment.selections)
                .any(|(&blocks, s)| {
                    blocks
                        .is_some_and(|blocks| s.action.is_some_and(Action::is_protective) != blocks)
                })
        {
            return Err(clean_to_raw_mapping_error(
                "residual policy preview mismatch",
            ));
        }
        Ok(())
    }
}

/// The action a residual fragment takes for a claimant resolved to `action`.
/// A fragment has no surface shape of its own to preserve, so
/// `format_preserve` becomes a plain reversible class token; every other
/// action is the claimant's own.
pub(super) fn fragment_action(action: Action) -> Action {
    match action {
        Action::FormatPreserve => Action::Tokenize,
        other => other,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan(
    pipeline: &Pipeline,
    segment: &occurrence::Segment,
    order: &[usize],
    selected: &[&Candidate],
    normalized: &str,
    raw: &str,
    context: &RuleContext,
    locales: &[crate::LocaleTag],
) -> Result<Plan> {
    #[cfg(test)]
    let mut work = Work::default();
    let views = segment
        .originals
        .iter()
        .map(|c| pipeline.registry.effective_view(c, normalized, locales))
        .collect::<Vec<_>>();
    let mut policies = std::collections::HashMap::new();
    let mut preview = |class: &PiiClass| -> Option<Action> {
        *policies.entry(class.clone()).or_insert_with(|| {
            #[cfg(test)]
            {
                work.preview_queries += 1;
            }
            crate::rule::preview(&pipeline.rules, class, context, |family| {
                pipeline.registry.family_member_classes(family)
            })
        })
    };
    let protective = |action: Option<Action>| action.is_some_and(Action::is_protective);
    // Require the original policy as well as its real standalone fallback policy.
    let mut admitted = Vec::with_capacity(segment.originals.len());
    let mut actions = Vec::with_capacity(segment.originals.len());
    for (candidate, (class, _)) in segment.originals.iter().zip(&views) {
        let own = preview(&candidate.class);
        let view = preview(class);
        admitted.push(protective(own) && protective(view));
        actions.push(view.map(fragment_action));
    }
    let blocking = selected
        .iter()
        .map(|candidate| preview(&candidate.class).map(Action::is_protective))
        .collect::<Vec<_>>();
    let blocks = blocking
        .iter()
        .map(|blocks| blocks.unwrap_or(true))
        .collect::<Vec<bool>>();
    // A `preserve` selection is the adopter's verdict for the candidates it
    // represents: its structural members (the winner, a same-span merge, the
    // rivals of a precedence tie) never override it, only candidates it
    // defeated do. This keeps an explicit family-class `preserve` rule
    // meaningful for the ambiguous span it names.
    for (selection, blocks) in segment.selections.iter().zip(&blocking) {
        if *blocks == Some(false) {
            for &member in &selection.members {
                admitted[member] = false;
            }
        }
    }
    let mut cells = Vec::<Cell>::new();
    sweep(
        segment,
        order,
        &blocks,
        |span, parents, blocked, preserved| {
            #[cfg(test)]
            {
                work.endpoint_cells += 1;
                work.active_parent_visits += parents.len();
            }
            let parents = parents
                .iter()
                .copied()
                .filter(|&id| admitted[id])
                .collect::<Vec<_>>();
            if blocked || parents.is_empty() {
                return Ok(());
            }
            let representative = parents[0];
            let (class, family) = &views[representative];
            let Some(action) = actions[representative] else {
                return Err(clean_to_raw_mapping_error(
                    "residual policy preview mismatch",
                ));
            };
            if !raw.is_char_boundary(span.start)
                || !raw.is_char_boundary(span.end)
                || span.is_empty()
            {
                return Err(clean_to_raw_mapping_error("invalid residual raw geometry"));
            }
            // One claimant yields one fragment per uncovered run: adjacent cells
            // of the same representative and class merge even where an inner
            // candidate starts or ends, and the parent list becomes the union.
            if let Some(last) = cells.last_mut() {
                if last.raw.end == span.start
                    && last.representative == representative
                    && last.class == *class
                    && last.family == *family
                {
                    last.raw.end = span.end;
                    last.overrides_preserve |= preserved;
                    for id in parents {
                        if !last.parents.contains(&id) {
                            last.parents.push(id);
                        }
                    }
                    return Ok(());
                }
            }
            cells.push(Cell {
                raw: span,
                parents,
                representative,
                class: class.clone(),
                family: family.clone(),
                action,
                overrides_preserve: preserved,
            });
            Ok(())
        },
    )?;
    // An independent interval-union check proves W + R equals the admitted U:
    // every byte an admitted original claimed is under a protective selection
    // or a cell, and no cell lies outside that union.
    let union = |mut spans: Vec<Range<usize>>| {
        spans.sort_by_key(|s| (s.start, s.end));
        let mut result = Vec::<Range<usize>>::new();
        for span in spans {
            if let Some(last) = result.last_mut() {
                if span.start <= last.end {
                    last.end = last.end.max(span.end);
                    continue;
                }
            }
            result.push(span);
        }
        result
    };
    let selected_spans = segment
        .selections
        .iter()
        .zip(&blocks)
        .filter(|(_, &blocks)| blocks)
        .map(|(s, _)| s.raw.clone())
        .collect::<Vec<_>>();
    let expected = union(
        segment
            .original_raw
            .iter()
            .enumerate()
            .filter(|(id, _)| admitted[*id])
            .map(|(_, s)| s.clone())
            .chain(selected_spans.iter().cloned())
            .collect(),
    );
    let actual = union(
        selected_spans
            .into_iter()
            .chain(cells.iter().map(|c| c.raw.clone()))
            .collect(),
    );
    if actual != expected {
        return Err(clean_to_raw_mapping_error("incomplete residual raw union"));
    }
    Ok(Plan {
        #[cfg(test)]
        work,
        cells,
        admitted,
        blocking,
    })
}

// Events at equal endpoints are applied together. Touching intervals never
// share a cell. `blocking[id]` says whether selection `id` covers its bytes;
// a selection that does not (a `preserve` winner) is reported to the visitor
// as `preserved` instead of blocking it.
fn sweep(
    segment: &occurrence::Segment,
    order: &[usize],
    blocking: &[bool],
    mut visit: impl FnMut(Range<usize>, &[usize], bool, bool) -> Result<()>,
) -> Result<()> {
    let mut ranks = vec![0; order.len()];
    for (rank, &id) in order.iter().enumerate() {
        ranks[id] = rank;
    }
    let mut events = BTreeMap::<usize, Vec<(bool, usize, bool)>>::new();
    for (id, span) in segment.original_raw.iter().enumerate() {
        events.entry(span.start).or_default().push((true, id, true));
        events.entry(span.end).or_default().push((true, id, false));
    }
    for (id, s) in segment.selections.iter().enumerate() {
        events
            .entry(s.raw.start)
            .or_default()
            .push((false, id, true));
        events
            .entry(s.raw.end)
            .or_default()
            .push((false, id, false));
    }
    let mut active = BTreeSet::new();
    let mut blocked = 0usize;
    let mut preserved = 0usize;
    let mut previous = None;
    for (position, changes) in events {
        if let Some(start) = previous {
            if start < position {
                let parents = if blocked == 0 {
                    active.iter().map(|&(_, id)| id).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                visit(start..position, &parents, blocked != 0, preserved != 0)?;
            }
        }
        for (original, id, add) in changes {
            if original {
                if add {
                    active.insert((ranks[id], id));
                } else {
                    active.remove(&(ranks[id], id));
                }
            } else {
                let counter = if blocking[id] {
                    &mut blocked
                } else {
                    &mut preserved
                };
                if add {
                    *counter += 1;
                } else {
                    *counter -= 1;
                }
            }
        }
        previous = Some(position);
    }
    Ok(())
}

pub(super) fn validate(segment: &occurrence::Segment) -> Result<()> {
    if segment.residuals.is_empty() {
        return Ok(());
    }
    if segment.original_raw.iter().any(Range::is_empty)
        || segment.selections.iter().any(|s| s.raw.is_empty())
    {
        return Err(manifest_integrity_error(
            "invalid residual evidence geometry",
        ));
    }
    let order = &segment.residual_order;
    if *order != crate::resolver::candidate_order(&segment.originals)
        || order.len() != segment.originals.len()
        || order.iter().copied().collect::<BTreeSet<_>>() != (0..segment.originals.len()).collect()
        || segment.residual_admitted.len() != segment.originals.len()
    {
        return Err(manifest_integrity_error("invalid residual evidence order"));
    }
    let admitted = &segment.residual_admitted;
    let blocking = segment
        .selections
        .iter()
        .map(|s| s.action.is_some_and(Action::is_protective))
        .collect::<Vec<_>>();
    let mut cell = 0;
    let mut covered_end = 0;
    let mut seen_parents = BTreeSet::new();
    let mut seen_preserved = false;
    sweep(
        segment,
        order,
        &blocking,
        |span, parents, blocked, preserved| {
            let Some(current) = segment.residuals.get(cell) else {
                return Ok(());
            };
            if span.end <= current.raw.start {
                return Ok(());
            }
            let parents = parents
                .iter()
                .copied()
                .filter(|&id| admitted[id])
                .collect::<Vec<_>>();
            if span.start < current.raw.start
                || span.end > current.raw.end
                || blocked
                || parents.first() != Some(&current.representative)
                || current.parents.first() != Some(&current.representative)
                || parents.iter().any(|id| !current.parents.contains(id))
            {
                return Err(manifest_integrity_error("invalid residual parent coverage"));
            }
            seen_parents.extend(parents);
            seen_preserved |= preserved;
            covered_end = span.end;
            if span.end == current.raw.end {
                if seen_parents.len() != current.parents.len()
                    || seen_preserved != current.overrides_preserve
                    || !current.action.is_protective()
                {
                    return Err(manifest_integrity_error("invalid residual parent coverage"));
                }
                seen_parents.clear();
                seen_preserved = false;
                cell += 1;
            }
            Ok(())
        },
    )?;
    if cell != segment.residuals.len()
        || segment
            .residuals
            .last()
            .is_some_and(|c| c.raw.end != covered_end)
    {
        return Err(manifest_integrity_error("incomplete residual relation"));
    }
    Ok(())
}
