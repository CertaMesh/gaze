use gaze::{Candidate, PiiClass, Pipeline, ProtectionError, SessionTransaction};
use std::collections::HashSet;

/// Aggregate expanded original text budget for one argument or response operation.
pub(super) const MAX_OPERATION_TEXT_BYTES: usize = 2 * 1024 * 1024;

/// Broad bare identifiers are literal text, not session claims. Bound each owned
/// expansion before allocation while retaining strict checks on other spellings.
pub(super) fn expand_owned_bounded(
    transaction: &SessionTransaction<'_>,
    input: &str,
    limit: usize,
) -> Result<String, ProtectionError> {
    gaze::token_shape::validate_restore_shapes(input).map_err(|_| ProtectionError::Provenance)?;
    let mut output = String::new();
    let mut cursor = 0;
    for bare in gaze::token_shape::pattern()
        .find_iter(input)
        .filter(|found| {
            gaze::token_shape::is_bare_identifier(found.as_str())
                && !transaction.contains_token(found.as_str())
        })
    {
        let gap = transaction
            .restore_strict_text_bounded(&input[cursor..bare.start()], limit - output.len())
            .map_err(|_| ProtectionError::Provenance)?;
        let size = output
            .len()
            .checked_add(gap.len())
            .and_then(|n| n.checked_add(bare.len()))
            .filter(|n| *n <= limit)
            .ok_or(ProtectionError::Provenance)?;
        output
            .try_reserve(size - output.len())
            .map_err(|_| ProtectionError::Provenance)?;
        output.push_str(&gap);
        output.push_str(bare.as_str());
        cursor = bare.end();
    }
    let tail = transaction
        .restore_strict_text_bounded(&input[cursor..], limit - output.len())
        .map_err(|_| ProtectionError::Provenance)?;
    output.push_str(&tail);
    Ok(output)
}

/// Enumerate all Rule hits independently of projection and candidate selection.
pub(super) fn verify_rule_coverage(
    pipeline: &Pipeline,
    transaction: &SessionTransaction<'_>,
    protected: &str,
) -> Result<(), ProtectionError> {
    let original = expand_owned_bounded(transaction, protected, MAX_OPERATION_TEXT_BYTES)?;
    let mut covered = vec![false; original.len()];
    let (mut wire_cursor, mut raw_cursor) = (0, 0);
    for range in owned_token_ranges(protected, &transaction.tokens()) {
        raw_cursor += range.start - wire_cursor;
        let value = transaction
            .restore_strict_text(&protected[range.clone()])
            .map_err(|_| ProtectionError::Provenance)?;
        let end = raw_cursor
            .checked_add(value.len())
            .ok_or(ProtectionError::Provenance)?;
        covered
            .get_mut(raw_cursor..end)
            .ok_or(ProtectionError::Provenance)?
            .fill(true);
        raw_cursor = end;
        wire_cursor = range.end;
    }
    if raw_cursor + protected.len() - wire_cursor != original.len() {
        return Err(ProtectionError::Provenance);
    }
    for hit in pipeline
        .staged_rule_repeat_candidates(transaction, &original)
        .map_err(|_| ProtectionError::Provenance)?
    {
        if !covered
            .get(hit.span)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte))
        {
            return Err(ProtectionError::Residual);
        }
    }
    Ok(())
}

/// Project every Rule hit, including overlaps, without replacing caller-owned tokens.
/// Compatible intermediate tokens may merge before publication. The final bitmap
/// proof is deliberately separate from this projection.
pub(super) fn cover_rule_repeats(
    pipeline: &Pipeline,
    transaction: &mut SessionTransaction<'_>,
    original: &str,
    input: &str,
) -> Result<String, ProtectionError> {
    let mut names = transaction.tokens();
    names.sort_unstable_by_key(|token| std::cmp::Reverse(token.len()));
    let caller_owned = owned_token_ranges(original, &names)
        .into_iter()
        .map(|range| original[range].to_owned())
        .collect::<HashSet<_>>();
    let expanded = expand_owned_bounded(transaction, input, MAX_OPERATION_TEXT_BYTES)?;
    let entries = transaction.snapshot_entries();
    let mut tokens = Vec::new();
    let (mut wire_cursor, mut view_cursor) = (0, 0);
    for wire in owned_token_ranges(input, &names) {
        let entry = entries
            .iter()
            .find(|entry| entry.token == input[wire.clone()])
            .ok_or(ProtectionError::Provenance)?;
        view_cursor += wire.start - wire_cursor;
        let value = transaction
            .restore_strict_text(&input[wire.clone()])
            .map_err(|_| ProtectionError::Provenance)?;
        let view = view_cursor..view_cursor + value.len();
        let immutable = caller_owned.contains(&input[wire.clone()]);
        wire_cursor = wire.end;
        view_cursor = view.end;
        tokens.push(RuleProjectionToken {
            wire,
            view,
            class: entry.class.clone(),
            family: entry.family.clone(),
            immutable,
        });
    }
    let hits = pipeline
        .staged_rule_repeat_candidates(transaction, &expanded)
        .map_err(|_| ProtectionError::Provenance)?;
    let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
    for hit in hits {
        groups
            .entry((hit.class.clone(), hit.token_family.clone()))
            .or_default()
            .push(hit);
    }
    let mut replacements = Vec::new();
    for hits in groups.values() {
        let unions = union_rule_ranges(hits.iter().map(|hit| (hit.span.clone(), hit)).collect());
        let mut parts = Vec::new();
        for (range, hit) in unions {
            let blocked = tokens.iter().filter(|token| {
                token.immutable || token.class != hit.class || token.family != hit.token_family
            });
            let mut cursor = range.start;
            for token in blocked {
                if token.view.end <= cursor || token.view.start >= range.end {
                    continue;
                }
                if cursor < token.view.start {
                    parts.push((cursor..token.view.start, hit));
                }
                cursor = cursor.max(token.view.end);
                if cursor >= range.end {
                    break;
                }
            }
            if cursor < range.end {
                parts.push((cursor..range.end, hit));
            }
        }
        for (range, hit) in &mut parts {
            for token in &tokens {
                if token.view.start < range.end && token.view.end > range.start {
                    if token.immutable
                        || token.class != hit.class
                        || token.family != hit.token_family
                    {
                        return Err(ProtectionError::Provenance);
                    }
                    range.start = range.start.min(token.view.start);
                    range.end = range.end.max(token.view.end);
                }
            }
        }
        for (view, hit) in union_rule_ranges(parts) {
            if tokens.iter().any(|token| {
                token.immutable
                    && token.view.is_empty()
                    && view.start <= token.view.start
                    && view.end >= token.view.end
            }) {
                // A zero-width owned value has no unique expanded-to-wire boundary.
                return Err(ProtectionError::Provenance);
            }
            let start = expanded_boundary_to_wire(view.start, &tokens, expanded.len())
                .ok_or(ProtectionError::Provenance)?;
            let end = expanded_boundary_to_wire(view.end, &tokens, expanded.len())
                .ok_or(ProtectionError::Provenance)?;
            replacements.push((start..end, view, hit.clone()));
        }
    }
    replacements.sort_by_key(|(wire, _, _)| (wire.start, wire.end));
    if replacements
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start)
    {
        return Err(ProtectionError::Provenance);
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for (wire, view, hit) in replacements {
        output.push_str(&input[cursor..wire.start]);
        if tokens
            .iter()
            .any(|token| token.wire == wire && token.view == view)
        {
            output.push_str(&input[wire.clone()]);
            cursor = wire.end;
            continue;
        }
        let token = pipeline
            .tokenize_staged_rule_repeat_fragment(transaction, &hit, &expanded[view])
            .map_err(|_| ProtectionError::Provenance)?;
        output.push_str(&token);
        cursor = wire.end;
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

struct RuleProjectionToken {
    wire: std::ops::Range<usize>,
    view: std::ops::Range<usize>,
    class: PiiClass,
    family: String,
    immutable: bool,
}

fn union_rule_ranges(
    mut ranges: Vec<(std::ops::Range<usize>, &Candidate)>,
) -> Vec<(std::ops::Range<usize>, &Candidate)> {
    ranges.sort_by(|(left, a), (right, b)| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.end.cmp(&left.end))
            .then_with(|| a.class.cmp(&b.class))
            .then_with(|| a.token_family.cmp(&b.token_family))
    });
    let mut union: Vec<(std::ops::Range<usize>, &Candidate)> = Vec::new();
    for (range, hit) in ranges {
        if let Some((previous, _)) = union.last_mut().filter(|(r, _)| r.end >= range.start) {
            previous.end = previous.end.max(range.end);
        } else {
            union.push((range, hit));
        }
    }
    union
}

fn expanded_boundary_to_wire(
    position: usize,
    tokens: &[RuleProjectionToken],
    length: usize,
) -> Option<usize> {
    let (mut wire, mut view) = (0, 0);
    for token in tokens {
        if position <= token.view.start {
            return Some(wire + position - view);
        }
        if position < token.view.end {
            return None;
        }
        wire = token.wire.end;
        view = token.view.end;
    }
    (position <= length).then_some(wire + position - view)
}

/// Locate immutable owned tokens in wire order, preferring the longest at a shared start.
fn owned_token_ranges(input: &str, tokens: &[String]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        let next = tokens
            .iter()
            .filter_map(|token| {
                input[cursor..]
                    .find(token)
                    .map(|offset| (cursor + offset, token.len()))
            })
            .min_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
        let Some((start, length)) = next else {
            break;
        };
        ranges.push(start..start + length);
        cursor = start + length;
    }
    ranges
}
