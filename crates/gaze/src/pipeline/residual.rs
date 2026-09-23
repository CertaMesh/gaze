//! Raw evidence coverage for the closed known-protective domain.
//!
//! A losing candidate's evidence is admitted when the static preview of every
//! action in its overlap component is protective (`Action::is_protective`).
//! Admitted cells always emit tokens; what they should emit under a
//! non-`tokenize` action is decided by todo 3740.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cell {
    pub(super) raw: Range<usize>,
    pub(super) parents: Vec<usize>,
    pub(super) representative: usize,
    pub(super) class: PiiClass,
    pub(super) family: String,
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
    selected: Vec<usize>,
}
impl Plan {
    pub(super) fn check_actual(&self, segment: &occurrence::Segment) -> Result<()> {
        if self.selected.iter().any(|&id| {
            !segment.selections[id]
                .action
                .is_some_and(Action::is_protective)
        }) {
            return Err(clean_to_raw_mapping_error(
                "residual policy preview mismatch",
            ));
        }
        Ok(())
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
    let mut known = |class: &PiiClass| {
        policies
            .entry(class.clone())
            .or_insert_with(|| {
                #[cfg(test)]
                {
                    work.preview_queries += 1;
                }
                crate::rule::preview(&pipeline.rules, class, context, |family| {
                    pipeline.registry.family_member_classes(family)
                })
            })
            .is_some_and(Action::is_protective)
    };
    // Require the original policy as well as its real standalone fallback policy.
    let original_known = segment
        .originals
        .iter()
        .zip(&views)
        .map(|(c, (class, _))| known(&c.class) && known(class))
        .collect::<Vec<_>>();
    let mut intervals = segment
        .original_raw
        .iter()
        .enumerate()
        .map(|(id, span)| (span.clone(), false, id))
        .chain(
            segment
                .selections
                .iter()
                .enumerate()
                .map(|(id, s)| (s.raw.clone(), true, id)),
        )
        .collect::<Vec<_>>();
    intervals.sort_by_key(|(span, _, _)| (span.start, span.end));
    let mut admitted = vec![false; segment.originals.len()];
    let mut checked_selected = Vec::new();
    let mut start = 0;
    while start < intervals.len() {
        let mut end = start + 1;
        let mut boundary = intervals[start].0.end;
        while end < intervals.len() && intervals[end].0.start < boundary {
            boundary = boundary.max(intervals[end].0.end);
            end += 1;
        }
        let component = &intervals[start..end];
        if component.iter().all(|(_, is_selected, id)| {
            if *is_selected {
                known(&selected[*id].class)
            } else {
                original_known[*id]
            }
        }) {
            for (_, is_selected, id) in component {
                if *is_selected {
                    checked_selected.push(*id);
                } else {
                    admitted[*id] = true;
                }
            }
        }
        start = end;
    }
    let mut cells = Vec::<Cell>::new();
    sweep(segment, order, |span, parents, blocked| {
        #[cfg(test)]
        {
            work.endpoint_cells += 1;
            work.active_parent_visits += parents.len();
        }
        if blocked || parents.is_empty() || !admitted[parents[0]] {
            return Ok(());
        }
        let representative = parents[0];
        let (class, family) = &views[representative];
        if !raw.is_char_boundary(span.start) || !raw.is_char_boundary(span.end) || span.is_empty() {
            return Err(clean_to_raw_mapping_error("invalid residual raw geometry"));
        }
        if let Some(last) = cells.last_mut() {
            if last.raw.end == span.start
                && last.parents == parents
                && last.representative == representative
                && last.class == *class
                && last.family == *family
            {
                last.raw.end = span.end;
                return Ok(());
            }
        }
        cells.push(Cell {
            raw: span,
            parents: parents.to_vec(),
            representative,
            class: class.clone(),
            family: family.clone(),
        });
        Ok(())
    })?;
    // An independent interval-union check proves W + R equals the admitted U.
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
    let selected_spans = checked_selected
        .iter()
        .map(|&id| segment.selections[id].raw.clone())
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
        selected: checked_selected,
    })
}

// Events at equal endpoints are applied together. Touching intervals never share a cell.
fn sweep(
    segment: &occurrence::Segment,
    order: &[usize],
    mut visit: impl FnMut(Range<usize>, &[usize], bool) -> Result<()>,
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
    let mut previous = None;
    for (position, changes) in events {
        if let Some(start) = previous {
            if start < position {
                let parents = if blocked == 0 {
                    active.iter().map(|&(_, id)| id).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                visit(start..position, &parents, blocked != 0)?;
            }
        }
        for (original, id, add) in changes {
            if original {
                if add {
                    active.insert((ranks[id], id));
                } else {
                    active.remove(&(ranks[id], id));
                }
            } else if add {
                blocked += 1;
            } else {
                blocked -= 1;
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
    {
        return Err(manifest_integrity_error("invalid residual evidence order"));
    }
    let mut cell = 0;
    let mut covered_end = 0;
    sweep(segment, order, |span, parents, blocked| {
        let Some(current) = segment.residuals.get(cell) else {
            return Ok(());
        };
        if span.end <= current.raw.start {
            return Ok(());
        }
        if span.start < current.raw.start
            || span.end > current.raw.end
            || blocked
            || parents != current.parents
            || parents.first() != Some(&current.representative)
        {
            return Err(manifest_integrity_error("invalid residual parent coverage"));
        }
        covered_end = span.end;
        if span.end == current.raw.end {
            cell += 1;
        }
        Ok(())
    })?;
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
