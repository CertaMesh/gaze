use std::ops::Range;

use crate::normalize::NormalizedText;
use crate::resolver::{CandidatePool, ResolutionEvent, WholeCandidate};
use crate::{Candidate, LocaleTag, RecognizerRegistry, Result};

pub(super) struct WholePlan {
    pub(super) primary: Vec<Candidate>,
    pub(super) recovered: Vec<Candidate>,
    #[allow(dead_code)]
    pub(super) events: Vec<ResolutionEvent>,
    #[cfg(test)]
    pub(super) work: crate::resolver::ResolutionWork,
}

pub(super) fn plan(
    mut pool: CandidatePool,
    registry: &RecognizerRegistry,
    normalized: &NormalizedText,
    raw: &str,
    locales: &[LocaleTag],
) -> Result<WholePlan> {
    let order = pool.order.clone();
    let original_spans = pool
        .originals()
        .iter()
        .map(|candidate| map_span(&candidate.span, normalized, raw))
        .collect::<Result<Vec<_>>>()?;
    // Reject malformed source geometry before anchor code can slice it. Valid
    // normalized scalars can still collide after mapping; freeze checks that
    // separately before any policy, audit or token allocation occurs.
    let primary = registry.resolve_pool(&mut pool, &order, &normalized.text, locales);
    let mut consumed = vec![false; pool.originals().len()];
    let primary = freeze(
        primary,
        &mut pool,
        &mut consumed,
        normalized,
        raw,
        0..raw.len(),
        false,
    )?;
    let mut work = gaps(order, &original_spans, &consumed, &primary, 0..raw.len());
    let mut recovered = Vec::new();
    // Only gaps whose pending pool changes are visited. Each productive pool
    // consumes a new member; no iteration or candidate cap can discard evidence.
    while let Some((gap, ids)) = work.pop() {
        let nodes = registry.resolve_pool(&mut pool, &ids, &normalized.text, locales);
        let selected = freeze(
            nodes,
            &mut pool,
            &mut consumed,
            normalized,
            raw,
            gap.clone(),
            true,
        )?;
        if selected.is_empty() {
            return Err(super::clean_to_raw_mapping_error(
                "nonempty recovery pool made no progress",
            ));
        }
        work.extend(gaps(ids, &original_spans, &consumed, &selected, gap));
        recovered.extend(selected);
    }
    recovered.sort_by_key(|candidate| candidate.span.start);
    Ok(WholePlan {
        primary,
        recovered,
        events: pool.events,
        #[cfg(test)]
        work: pool.work,
    })
}

fn map_span(span: &Range<usize>, normalized: &NormalizedText, raw: &str) -> Result<Range<usize>> {
    if span.is_empty()
        || !normalized.text.is_char_boundary(span.start)
        || !normalized.text.is_char_boundary(span.end)
    {
        return Err(super::clean_to_raw_mapping_error(
            "invalid normalized candidate geometry",
        ));
    }
    let mapped = super::translate_span(span.clone(), &normalized.spans)
        .ok_or_else(|| super::clean_to_raw_mapping_error("unmappable candidate geometry"))?;
    if mapped.is_empty() || !raw.is_char_boundary(mapped.start) || !raw.is_char_boundary(mapped.end)
    {
        return Err(super::clean_to_raw_mapping_error(
            "invalid raw candidate geometry",
        ));
    }
    Ok(mapped)
}

#[allow(clippy::too_many_arguments)]
fn freeze(
    nodes: Vec<WholeCandidate>,
    pool: &mut CandidatePool,
    consumed: &mut [bool],
    normalized: &NormalizedText,
    raw: &str,
    gap: Range<usize>,
    recovery: bool,
) -> Result<Vec<Candidate>> {
    let mut selected = Vec::with_capacity(nodes.len());
    let mut end = gap.start;
    for node in nodes {
        let span = map_span(&node.candidate.span, normalized, raw)?;
        // NFC can expand one raw scalar into disjoint normalized candidates.
        // Such candidates must never become overlapping raw replacements.
        if span.start < end || span.end > gap.end {
            return Err(super::clean_to_raw_mapping_error(
                "whole candidates overlap in raw coordinates",
            ));
        }
        if node.members.is_empty() || node.members.iter().any(|&id| consumed[id]) {
            return Err(super::clean_to_raw_mapping_error(
                "whole selection reused consumed evidence",
            ));
        }
        for &id in &node.members {
            consumed[id] = true;
        }
        if recovery {
            pool.events.push(ResolutionEvent::Recovery {
                node: node.node,
                normalized: node.candidate.span.clone(),
                raw: span.clone(),
            });
        }
        end = span.end;
        selected.push(node.candidate.with_span(span));
    }
    Ok(selected)
}

fn gaps(
    ids: Vec<usize>,
    originals: &[Range<usize>],
    consumed: &[bool],
    frozen: &[Candidate],
    outer: Range<usize>,
) -> Vec<(Range<usize>, Vec<usize>)> {
    let mut pending = vec![Vec::new(); frozen.len() + 1];
    for id in ids {
        if consumed[id] {
            continue;
        }
        let span = &originals[id];
        let gap = frozen.partition_point(|node| node.span.end <= span.start);
        if frozen
            .get(gap)
            .is_none_or(|node| span.end <= node.span.start)
        {
            pending[gap].push(id);
        }
    }
    pending
        .into_iter()
        .enumerate()
        .filter_map(|(index, ids)| {
            if ids.is_empty() {
                return None;
            }
            let start = if index == 0 {
                outer.start
            } else {
                frozen[index - 1].span.end
            };
            let end = frozen.get(index).map_or(outer.end, |node| node.span.start);
            Some((start..end, ids))
        })
        .collect()
}

#[cfg(test)]
mod tests;
