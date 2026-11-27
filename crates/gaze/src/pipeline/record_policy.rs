//! Request-local policy inheritance. Edges connect the complete record value to
//! detections that share its bytes; equal normalized values share one node.
//! No edge discovers a match or changes the evidence used by the resolver/sweep.
use super::*;
use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
pub(super) struct RecordPolicy(BTreeMap<(usize, usize), Action>);

impl RecordPolicy {
    pub(super) fn effective(&self, span: &Range<usize>, own: Action) -> Action {
        self.0
            .get(&(span.start, span.end))
            .map_or(own, |&a| own.strictest(a))
    }

    pub(super) fn preview(&self, span: &Range<usize>, own: Option<Action>) -> Option<Action> {
        // A floor is installed only if *every* contributing action is known,
        // or some contributor mandates Redact (the maximum even for an opaque rule).
        self.0.get(&(span.start, span.end)).copied().or(own)
    }

    pub(super) fn plan(
        pipeline: &Pipeline,
        segment: &occurrence::Segment,
        normalized: &crate::normalize::NormalizedText,
        context: &RuleContext,
        locales: &[crate::LocaleTag],
    ) -> Result<Self> {
        if !segment.originals.iter().any(|c| c.complete_record_id) {
            return Ok(Self::default());
        }

        // Work in raw coordinates (the same coordinates as all emissions), but
        // compare values in the shared detection view, including fullwidth copies.
        let value = |(start, end)| {
            let lo = normalized.spans.partition_point(|&(_, e)| e <= start);
            let hi = normalized.spans.partition_point(|&(s, _)| s < end);
            &normalized.text[lo..hi]
        };
        let claims: BTreeSet<_> = segment
            .originals
            .iter()
            .zip(&segment.original_raw)
            .filter(|(c, _)| c.complete_record_id)
            .map(|(_, r)| value((r.start, r.end)))
            .collect();
        let mut spans = BTreeMap::new();
        for s in segment
            .original_raw
            .iter()
            .chain(segment.selections.iter().map(|s| &s.raw))
        {
            spans
                .entry((s.start, s.end))
                .or_insert_with(|| value((s.start, s.end)));
        }
        let mut edges = HashMap::<&str, BTreeSet<&str>>::new();
        let mut occurrences = HashMap::new();
        // A containing or partially overlapping winner must not restore any of
        // the ID either. Index interval ends so disjoint IDs don't scan the pool.
        let mut max_end = 0;
        let intervals: Vec<_> = spans
            .iter()
            .map(|(&(start, end), &v)| {
                max_end = max_end.max(end);
                (start, end, max_end, v)
            })
            .collect();
        for &(start, end, _, id) in &intervals {
            if !claims.contains(id) {
                continue;
            }
            *occurrences.entry(id).or_insert(0usize) += 1;
            let first = intervals.partition_point(|&(_, _, max_end, _)| max_end <= start);
            for &(_, other_end, _, other) in intervals[first..].iter().take_while(|i| i.0 < end) {
                if other_end <= start {
                    continue;
                }
                edges.entry(id).or_default().insert(other);
                edges.entry(other).or_default().insert(id);
            }
        }
        let mut classes = HashMap::<&str, BTreeSet<PiiClass>>::new();
        for (original, raw) in segment.originals.iter().zip(&segment.original_raw) {
            let v = spans[&(raw.start, raw.end)];
            if edges.contains_key(v) {
                let set = classes.entry(v).or_default();
                set.insert(original.class.clone());
                set.insert(
                    pipeline
                        .registry
                        .effective_view(original, &normalized.text, locales)
                        .0,
                );
            }
        }
        for s in &segment.selections {
            let v = spans[&(s.raw.start, s.raw.end)];
            if edges.contains_key(v) {
                classes.entry(v).or_default().insert(s.class.clone());
            }
        }

        let mut policies = HashMap::new();
        let mut actions = HashMap::new();
        for id in claims {
            if actions.contains_key(id) {
                continue;
            }
            // A single span with only its own class needs no inheritance. Its
            // ordinary runtime lookup (including an opaque rule) stays untouched.
            if occurrences[id] == 1 && edges[id].len() == 1 && classes[id].len() == 1 {
                continue;
            }
            let mut stack = vec![id];
            let mut component = Vec::new();
            let mut action = Action::Preserve;
            let mut unknown = false;
            while let Some(v) = stack.pop() {
                let Some(adjacent) = edges.remove(v) else {
                    continue;
                };
                component.push(v);
                stack.extend(adjacent);
                for class in &classes[v] {
                    // An opaque callback may be stateful. Do not probe it in
                    // planning, guess its action, or mint a possibly restorable copy.
                    let preview = policies.entry(class).or_insert_with(|| {
                        crate::rule::preview(&pipeline.rules, class, context, |family| {
                            pipeline.registry.family_member_classes(family)
                        })
                    });
                    if let Some(a) = *preview {
                        action = action.strictest(a);
                    } else {
                        unknown = true;
                    }
                }
            }
            if unknown && action != Action::Redact {
                return Err(Error::OpaqueRecordPolicy);
            }
            actions.extend(component.into_iter().map(|v| (v, action)));
        }
        Ok(Self(
            spans
                .into_iter()
                .filter_map(|(s, v)| actions.get(v).map(|&a| (s, a)))
                .collect(),
        ))
    }
}
