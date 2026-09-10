//! Private synthetic prototype. No production entry point or checked model provider exists yet.
use super::*;
use crate::normalize::NormalizedText;
use crate::rule::{ClassRule, DefaultRule};

const MAX_CANDIDATES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockError {
    UnsupportedScope,
    MissingBaseline,
    InvalidBatch,
    InvalidMap,
    UnsupportedAction,
    Incomplete,
    ProviderFailed,
    UnknownLabel,
}

impl From<LockError> for Error {
    fn from(error: LockError) -> Self {
        let code = match error {
            LockError::UnsupportedScope => "LOCK_UNSUPPORTED_SCOPE",
            LockError::MissingBaseline => "LOCK_MISSING_BASELINE",
            LockError::InvalidBatch => "LOCK_INVALID_BATCH",
            LockError::InvalidMap => "LOCK_INVALID_MAP",
            LockError::UnsupportedAction => "LOCK_UNSUPPORTED_ACTION",
            LockError::Incomplete => "LOCK_INCOMPLETE",
            LockError::ProviderFailed => "LOCK_PROVIDER_FAILED",
            LockError::UnknownLabel => "LOCK_UNKNOWN_LABEL",
        };
        gaze_types::DetectError::backend("synthetic.baseline_lock", code).into()
    }
}

#[derive(Clone)]
enum FrozenRule {
    Class(PiiClass, Action),
    Default(Action),
}

// Construction owns the policy proof; arbitrary Rule implementations are never inspected.
fn baseline_pipeline(
    mut builder: PipelineBuilder,
    rules: &[FrozenRule],
    kind: DocumentKind,
) -> Result<Pipeline> {
    if !builder.rules.is_empty()
        || builder.optimization_config.prefix_cache
        || !builder.safety_nets.is_empty()
        || kind != DocumentKind::Text
    {
        return Err(LockError::UnsupportedScope.into());
    }
    #[cfg(feature = "bundled-recognizers")]
    if builder.safety_net_registry.is_some() {
        return Err(LockError::UnsupportedScope.into());
    }
    if builder.recognizers.is_empty() {
        return Err(LockError::MissingBaseline.into());
    }
    for rule in rules {
        let action = match rule {
            FrozenRule::Class(_, action) | FrozenRule::Default(action) => *action,
        };
        if !matches!(action, Action::Tokenize | Action::Preserve) {
            return Err(LockError::UnsupportedAction.into());
        }
    }
    for rule in rules {
        builder = match rule {
            FrozenRule::Class(class, action) => {
                builder.rule(ClassRule::new(class.clone(), *action))
            }
            FrozenRule::Default(action) => builder.rule(DefaultRule::new(*action)),
        };
    }
    builder.build()
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedItem {
    candidate: Candidate,
    normalized: Range<usize>,
    raw: Range<usize>,
    action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Admitted,
    BaselineOverlap,
    SupplementalOverlap,
}

// Complete is a controlled synthetic assertion, not evidence of vendor window validation.
enum SupplementalBatch {
    Complete(Vec<Candidate>),
    Incomplete,
}

struct PreparedPlan {
    baseline: Vec<PreparedItem>,
    final_items: Vec<PreparedItem>,
    dispositions: Vec<Disposition>,
}

// Structural validation only. run_request owns authenticity by using normalize(raw) exclusively.
fn validate_map(raw: &str, normalized: &NormalizedText) -> Result<()> {
    if normalized.spans.len() != normalized.text.len() {
        return Err(LockError::InvalidMap.into());
    }
    let mut previous = None;
    for (offset, &(start, end)) in normalized.spans.iter().enumerate() {
        if start >= end
            || end > raw.len()
            || !raw.is_char_boundary(start)
            || !raw.is_char_boundary(end)
            || raw[start..end].chars().count() != 1
        {
            return Err(LockError::InvalidMap.into());
        }
        if let Some((old_start, old_end)) = previous {
            // A run repeats one raw character; successive runs cannot rewind or intersect.
            if (start, end) != (old_start, old_end) && start < old_end {
                return Err(LockError::InvalidMap.into());
            }
            if !normalized.text.is_char_boundary(offset) && (start, end) != (old_start, old_end) {
                return Err(LockError::InvalidMap.into());
            }
        }
        previous = Some((start, end));
    }
    Ok(())
}

fn prepare_batch(
    pipeline: &Pipeline,
    raw: &str,
    normalized: &NormalizedText,
    batch: Vec<Candidate>,
    supplemental: bool,
) -> Result<Vec<PreparedItem>> {
    if batch.len() > MAX_CANDIDATES {
        return Err(LockError::InvalidBatch.into());
    }
    let mut previous_end = 0;
    let mut items = Vec::with_capacity(batch.len());
    for candidate in batch {
        let span = &candidate.span;
        if !candidate.score.is_finite()
            || !(0.0..=1.0).contains(&candidate.score)
            || span.start >= span.end
            || span.end > normalized.text.len()
            || span.start < previous_end
            || !normalized.text.is_char_boundary(span.start)
            || !normalized.text.is_char_boundary(span.end)
        {
            return Err(LockError::InvalidBatch.into());
        }
        if supplemental
            && (candidate.source != "synthetic.supplement"
                || candidate.recognizer_id != "synthetic.supplement"
                || !(matches!(&candidate.class, PiiClass::Email | PiiClass::Name)
                    || matches!(&candidate.class, PiiClass::Custom(name) if name == "synthetic")))
        {
            return Err(LockError::UnknownLabel.into());
        }
        let mapped = normalized.spans[span.start].0..normalized.spans[span.end - 1].1;
        if mapped.start >= mapped.end || mapped.end > raw.len() {
            return Err(LockError::InvalidMap.into());
        }
        if !supplemental
            && items
                .last()
                .is_some_and(|item: &PreparedItem| item.raw.end > mapped.start)
        {
            return Err(LockError::InvalidMap.into());
        }
        let detection = Detection::new(
            mapped.clone(),
            candidate.class.clone(),
            candidate.source.clone(),
        );
        let action = pipeline.action_for(&detection, &build_context(None));
        if !matches!(action, Action::Tokenize | Action::Preserve) {
            return Err(LockError::UnsupportedAction.into());
        }
        previous_end = span.end;
        items.push(PreparedItem {
            normalized: span.clone(),
            raw: mapped,
            candidate,
            action,
        });
    }
    Ok(items)
}

fn prepare_plan(
    pipeline: &Pipeline,
    raw: &str,
    normalized: &NormalizedText,
    baseline: Vec<Candidate>,
    supplemental: SupplementalBatch,
) -> Result<PreparedPlan> {
    validate_map(raw, normalized)?;
    let SupplementalBatch::Complete(extras) = supplemental else {
        return Err(LockError::Incomplete.into());
    };
    let baseline = prepare_batch(pipeline, raw, normalized, baseline, false)?;
    let extras = prepare_batch(pipeline, raw, normalized, extras, true)?;
    let mut admitted: Vec<PreparedItem> = Vec::new();
    let mut dispositions = Vec::with_capacity(extras.len());
    let mut base_index = 0;
    for extra in extras {
        while base_index < baseline.len()
            && baseline[base_index].raw.end <= extra.raw.start
            && baseline[base_index].normalized.end <= extra.normalized.start
        {
            base_index += 1;
        }
        let overlaps_baseline = baseline.get(base_index).is_some_and(|base| {
            ranges_overlap(&base.raw, &extra.raw)
                || ranges_overlap(&base.normalized, &extra.normalized)
        });
        if overlaps_baseline {
            dispositions.push(Disposition::BaselineOverlap);
        } else if admitted
            .last()
            .is_some_and(|last| ranges_overlap(&last.raw, &extra.raw))
        {
            dispositions.push(Disposition::SupplementalOverlap);
        } else {
            dispositions.push(Disposition::Admitted);
            admitted.push(extra);
        }
    }
    let mut final_items = baseline.clone();
    final_items.extend(admitted);
    final_items.sort_by_key(|item| item.raw.start);
    Ok(PreparedPlan {
        baseline,
        final_items,
        dispositions,
    })
}

struct LockOutput {
    clean: CleanText,
    plan: PreparedPlan,
    trace: Vec<GazeLocalProtectionTraceItem>,
    session: Session,
}

#[allow(clippy::too_many_arguments)]
fn run_request(
    builder: PipelineBuilder,
    rules: &[FrozenRule],
    raw: &str,
    kind: DocumentKind,
    _cache_write: PrefixCacheWriteMode,
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
    supplement: impl FnOnce(
        &str,
        &DetectContext<'_>,
    ) -> std::result::Result<SupplementalBatch, LockError>,
) -> Result<LockOutput> {
    let pipeline = baseline_pipeline(builder, rules, kind)?;
    let normalized = normalize(raw);
    let ctx = DetectContext::new(locale_chain, dictionaries);
    let (baseline, vetoed) = pipeline
        .registry
        .detect_all_resolved(&normalized.text, &ctx)?;
    let extras = supplement(&normalized.text, &ctx)?;
    let plan = prepare_plan(&pipeline, raw, &normalized, baseline, extras)?;
    let session = Session::new_with_session_hex_for_tests(crate::Scope::Ephemeral, [1, 2, 3, 4])?;
    let mut target = ProtectionTarget::Live(&session);
    // All final items are validated before audit/token side effects. Preserve baseline audit behavior.
    let mapped_baseline = plan
        .baseline
        .iter()
        .map(|item| {
            let mut c = item.candidate.clone();
            c.span = item.raw.clone();
            c
        })
        .collect::<Vec<_>>();
    for loser in merged_losers(&mapped_baseline, &pipeline.registry) {
        pipeline.log_entry(
            &target,
            &loser,
            None,
            kind,
            pipeline.action_for(&loser.detection, &build_context(None)),
            true,
        )?;
    }
    for mut veto in vetoed {
        let Some(span) = translate_span(veto.candidate.span.clone(), &normalized.spans) else {
            return Err(LockError::InvalidMap.into());
        };
        veto.candidate.span = span;
        pipeline.log_vetoed_entry(&target, &veto, None, kind)?;
    }
    let detections = plan.final_items.iter().map(|item| {
        let mut candidate = item.candidate.clone();
        candidate.span = item.raw.clone();
        (
            indexed_detection_from_candidate(candidate, &pipeline.registry),
            Some(item.action),
        )
    });
    let mut trace = ProtectionTraceCollector::new(raw);
    let clean =
        pipeline.emit_text_plan(&mut target, raw, None, kind, detections, Some(&mut trace))?;
    let trace = trace.finish(&clean.manifest)?;
    Ok(LockOutput {
        clean,
        plan,
        trace,
        session,
    })
}

#[cfg(test)]
mod tests;
