use super::DispatchError;
use gaze::{Candidate, PiiClass, Pipeline, ProtectionError, SessionTransaction};
use std::collections::HashSet;

/// Existing MCP sizing constant used to derive the full expansion budget.
pub(super) const MAX_OPERATION_INPUT_BYTES: usize = 2 * 1024 * 1024;
/// Aggregate full restored text budget; decoded input has no separate cap.
pub(super) const MAX_OPERATION_EXPANDED_BYTES: usize = 16 * MAX_OPERATION_INPUT_BYTES;

/// Expand only exact current-session mappings. Other namespaces remain literal,
/// as in the primary protection path. Check the full size before allocating.
pub(super) fn expand_owned_bounded(
    transaction: &SessionTransaction<'_>,
    input: &str,
    limit: usize,
) -> Result<String, DispatchError> {
    let ranges = owned_token_ranges(input, &transaction.tokens())?;
    let unchanged = ranges
        .iter()
        .try_fold(input.len(), |size, range| size.checked_sub(range.len()))
        .ok_or(ProtectionError::Provenance)?;
    let mut remaining = limit
        .checked_sub(unchanged)
        .ok_or(DispatchError::ResourceLimit)?;
    let mut expanded = Vec::with_capacity(ranges.len());
    for range in ranges {
        let raw = transaction
            .restore_strict_text_bounded(&input[range.clone()], remaining)
            .map_err(|error| match error {
                gaze::BoundedRestoreError::CapExceeded => DispatchError::ResourceLimit,
                _ => ProtectionError::Provenance.into(),
            })?;
        remaining -= raw.len();
        expanded.push((range, raw));
    }
    let mut output = String::new();
    output
        .try_reserve_exact(limit - remaining)
        .map_err(|_| DispatchError::ResourceLimit)?;
    let mut cursor = 0;
    for (range, raw) in expanded {
        output.push_str(&input[cursor..range.start]);
        output.push_str(&raw);
        cursor = range.end;
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

/// Enumerate all Rule hits independently of projection and candidate selection.
pub(super) fn verify_rule_coverage(
    pipeline: &Pipeline,
    transaction: &SessionTransaction<'_>,
    protected: &str,
) -> Result<(), DispatchError> {
    let original = expand_owned_bounded(transaction, protected, MAX_OPERATION_EXPANDED_BYTES)?;
    let mut covered = vec![false; original.len()];
    let mut owned_spans = Vec::new();
    let (mut wire_cursor, mut raw_cursor) = (0, 0);
    for range in owned_token_ranges(protected, &transaction.tokens())? {
        raw_cursor += range.start - wire_cursor;
        let value = transaction
            .restore(&protected[range.clone()])
            .ok_or(ProtectionError::Provenance)?;
        let end = raw_cursor
            .checked_add(value.len())
            .ok_or(ProtectionError::Provenance)?;
        covered
            .get_mut(raw_cursor..end)
            .ok_or(ProtectionError::Provenance)?
            .fill(true);
        owned_spans.push(raw_cursor..end);
        raw_cursor = end;
        wire_cursor = range.end;
    }
    if raw_cursor + protected.len() - wire_cursor != original.len() {
        return Err(ProtectionError::Provenance.into());
    }
    // Rebuild interior-gap coverage from actual output ownership, independently
    // of the projection's selections. Outer or non-whitespace bytes stay raw.
    for pair in owned_spans.windows(2) {
        let gap = pair[0].end..pair[1].start;
        if original[gap.clone()].chars().all(char::is_whitespace) {
            covered[gap].fill(true);
        }
    }
    for hit in pipeline
        .staged_rule_repeat_candidates(transaction, &original)
        .map_err(|_| ProtectionError::Provenance)?
    {
        if !covered
            .get(hit.span)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte))
        {
            return Err(ProtectionError::Residual.into());
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
) -> Result<String, DispatchError> {
    let mut names = transaction.tokens();
    names.sort_unstable_by_key(|token| std::cmp::Reverse(token.len()));
    let caller_owned = owned_token_ranges(original, &names)?
        .into_iter()
        .map(|range| original[range].to_owned())
        .collect::<HashSet<_>>();
    let expanded = expand_owned_bounded(transaction, input, MAX_OPERATION_EXPANDED_BYTES)?;
    let entries = transaction.snapshot_entries();
    let mut tokens = Vec::new();
    let (mut wire_cursor, mut view_cursor) = (0, 0);
    for wire in owned_token_ranges(input, &names)? {
        let entry = entries
            .iter()
            .find(|entry| entry.token == input[wire.clone()])
            .ok_or(ProtectionError::Provenance)?;
        view_cursor += wire.start - wire_cursor;
        let value = transaction
            .restore(&input[wire.clone()])
            .ok_or(ProtectionError::Provenance)?;
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
    let public_gaps = tokens
        .windows(2)
        .map(|pair| pair[0].view.end..pair[1].view.start)
        .filter(|gap| expanded[gap.clone()].chars().all(char::is_whitespace))
        .collect::<Vec<_>>();
    let mut replacements = Vec::new();
    for hits in groups.values() {
        let unions = union_rule_ranges(hits.iter().map(|hit| (hit.span.clone(), hit)).collect());
        let mut parts = Vec::new();
        for (range, hit) in unions {
            // Subtract public gaps before expanding fragments to compatible tokens.
            // This includes tokens newly protected during this operation.
            let mut blocked = tokens
                .iter()
                .filter(|token| {
                    token.immutable || token.class != hit.class || token.family != hit.token_family
                })
                .map(|token| token.view.clone())
                .chain(public_gaps.iter().cloned())
                .collect::<Vec<_>>();
            for (index, token) in tokens.iter().enumerate() {
                let left = index.checked_sub(1).map_or(0, |i| tokens[i].view.end);
                let right = tokens
                    .get(index + 1)
                    .map_or(expanded.len(), |t| t.view.start);
                let before = &expanded[left..token.view.start];
                let after = &expanded[token.view.end..right];
                let leading = token.view.end
                    ..token.view.end + after.len()
                        - after.trim_start_matches(char::is_whitespace).len();
                let trailing =
                    left + before.trim_end_matches(char::is_whitespace).len()..token.view.start;
                // The other side will be protected by this hit. Whitespace at
                // the outer edge of a hit still requires ordinary coverage.
                if !leading.is_empty() && range.start <= leading.start && leading.end < range.end {
                    blocked.push(leading);
                }
                if !trailing.is_empty() && range.start < trailing.start && trailing.end <= range.end
                {
                    blocked.push(trailing);
                }
            }
            blocked.sort_unstable_by_key(|span| (span.start, span.end));
            let mut cursor = range.start;
            for span in blocked {
                if span.end <= cursor || span.start >= range.end {
                    continue;
                }
                if cursor < span.start {
                    parts.push((cursor..span.start, hit));
                }
                cursor = cursor.max(span.end);
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
                if !token.view.is_empty()
                    && token.view.start < range.end
                    && token.view.end > range.start
                {
                    if token.immutable
                        || token.class != hit.class
                        || token.family != hit.token_family
                    {
                        return Err(ProtectionError::Provenance.into());
                    }
                    range.start = range.start.min(token.view.start);
                    range.end = range.end.max(token.view.end);
                }
            }
        }
        for (view, hit) in union_rule_ranges(parts) {
            // Empty owned values are wire markers. Split at interior markers,
            // then place starts after boundary markers and ends before them.
            // No replacement may consume a marker or change its wire position.
            let mut start = view.start;
            for end in tokens
                .iter()
                .filter(|token| {
                    token.view.is_empty()
                        && view.start < token.view.start
                        && token.view.start < view.end
                })
                .map(|token| token.view.start)
                .chain(std::iter::once(view.end))
            {
                if start == end {
                    continue;
                }
                let wire_start =
                    expanded_boundary_to_wire(start, &tokens, expanded.len(), RuleRangeEdge::Start)
                        .ok_or(ProtectionError::Provenance)?;
                let wire_end =
                    expanded_boundary_to_wire(end, &tokens, expanded.len(), RuleRangeEdge::End)
                        .ok_or(ProtectionError::Provenance)?;
                replacements.push((wire_start..wire_end, start..end, hit.clone()));
                start = end;
            }
        }
    }
    replacements.sort_by_key(|(wire, _, _)| (wire.start, wire.end));
    if replacements
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start)
    {
        return Err(ProtectionError::Provenance.into());
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

enum RuleRangeEdge {
    Start,
    End,
}

fn expanded_boundary_to_wire(
    position: usize,
    tokens: &[RuleProjectionToken],
    length: usize,
    edge: RuleRangeEdge,
) -> Option<usize> {
    let (mut wire, mut view) = (0, 0);
    for token in tokens {
        if position == token.view.start
            && token.view.is_empty()
            && matches!(edge, RuleRangeEdge::Start)
        {
            wire = token.wire.end;
            view = token.view.end;
            continue;
        }
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

/// Match the primary protection path's exact manifest spellings and boundaries.
fn owned_token_ranges(
    input: &str,
    tokens: &[String],
) -> Result<Vec<std::ops::Range<usize>>, ProtectionError> {
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let mut tokens = tokens.to_vec();
    tokens.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let pattern = tokens
        .iter()
        .map(|token| {
            let escaped = regex::escape(token);
            if token.starts_with('<') && token.ends_with('>') {
                escaped
            } else if token.split_once(':').is_some_and(|(prefix, _)| {
                prefix.len() == 8
                    && prefix
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }) {
                format!(r"(?:{escaped})\b")
            } else {
                format!(r"\b(?:{escaped})\b")
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    let pattern = regex::Regex::new(&pattern).map_err(|_| ProtectionError::Provenance)?;
    Ok(pattern
        .find_iter(input)
        .filter(|found| {
            !(found
                .as_str()
                .split_once(':')
                .is_some_and(|(_, body)| body.starts_with("custom:family:"))
                && input.as_bytes().get(found.end()) == Some(&b'-'))
        })
        .map(|found| found.range())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::owned_token_ranges;

    #[test]
    fn owned_locator_keeps_primary_protection_boundaries() {
        let tokens = [
            "<deadbeef:Name_1>",
            "deadbeef:name_1",
            "email1.deadbeef@gaze-fake.invalid",
            "deadbeef:custom:family:account_1",
        ]
        .map(str::to_owned);
        for literal in [
            "<cafebabe:Name_1>",
            "deadbeef:name_10",
            "xemail1.deadbeef@gaze-fake.invalid",
            "email1.deadbeef@gaze-fake.invalidx",
            "deadbeef:custom:family:account_1-more",
        ] {
            assert!(owned_token_ranges(literal, &tokens).unwrap().is_empty());
        }
        for token in &tokens {
            let text = format!("before {token} after");
            let ranges = owned_token_ranges(&text, &tokens).unwrap();
            assert_eq!(ranges.len(), 1);
            assert_eq!(&text[ranges[0].clone()], token);
        }
        assert_eq!(
            owned_token_ranges("prefixdeadbeef:name_1", &tokens)
                .unwrap()
                .len(),
            1
        );
    }
    struct Names;
    impl gaze::Recognizer for Names {
        fn id(&self) -> &str {
            "synthetic-names"
        }
        fn evidence(&self) -> gaze::EvidenceKind {
            gaze::EvidenceKind::Rule
        }
        fn supported_class(&self) -> &gaze::PiiClass {
            &gaze::PiiClass::Name
        }
        fn token_family(&self) -> &str {
            "name"
        }
        fn detect(
            &self,
            input: &str,
            _: &gaze::DetectContext<'_>,
        ) -> Result<Vec<gaze::Candidate>, gaze_types::DetectError> {
            Ok(regex::Regex::new(r"Maria(?: van)? Schneider")
                .unwrap()
                .find_iter(input)
                .map(|found| {
                    gaze::Candidate::new(
                        found.range(),
                        gaze::PiiClass::Name,
                        self.id(),
                        1.0,
                        0,
                        None,
                        "name",
                        "synthetic",
                        gaze::ConflictTier::None,
                        vec![],
                    )
                })
                .collect())
        }
    }
    #[test]
    fn final_proof_covers_only_whitespace_interior_gaps_independently_of_projection() {
        let pipeline = gaze::Pipeline::builder()
            .recognizer(Names)
            .rule(gaze::ClassRule::new(
                gaze::PiiClass::Name,
                gaze::Action::Tokenize,
            ))
            .rule(gaze::DefaultRule::new(gaze::Action::Preserve))
            .build()
            .unwrap();
        let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
        let first = session.tokenize(&gaze::PiiClass::Name, "Maria").unwrap();
        let last = session
            .tokenize(&gaze::PiiClass::Name, "Schneider")
            .unwrap();
        let mut transaction = session.begin_transaction();
        pipeline
            .discover_staged_rule_sources(
                &mut transaction,
                "Maria Schneider; Maria van Schneider",
                &[gaze::LocaleTag::Global],
                super::super::default_dictionaries(),
            )
            .unwrap();
        for gap in [" ", "  ", "\t", "\u{2003}"] {
            super::verify_rule_coverage(&pipeline, &transaction, &format!("{first}{gap}{last}"))
                .unwrap();
        }
        assert!(matches!(
            super::verify_rule_coverage(&pipeline, &transaction, &format!("{first} van {last}")),
            Err(super::DispatchError::Protection(
                gaze::ProtectionError::Residual
            ))
        ));
        assert!(matches!(
            super::verify_rule_coverage(&pipeline, &transaction, "Maria Schneider"),
            Err(super::DispatchError::Protection(
                gaze::ProtectionError::Residual
            ))
        ));
    }
}
