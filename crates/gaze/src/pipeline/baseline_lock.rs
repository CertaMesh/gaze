//! Baseline-preserving request preparation; live access is sealed behind an experimental feature.
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
        gaze_types::DetectError::backend("benchmark.baseline_lock", code).into()
    }
}

#[derive(Clone)]
enum FrozenRule {
    Class(PiiClass, Action),
    Default(Action),
}

// Construction owns the policy proof; arbitrary Rule implementations are never inspected.
#[cfg(test)]
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
pub enum Disposition {
    Admitted,
    BaselineOverlap,
    SupplementalOverlap,
}

// Private completeness: synthetic assertions exist only in tests; live callers use try_detect.
enum SupplementalBatch {
    Complete(Vec<Candidate>),
    #[cfg(test)]
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
        if supplemental && !valid_supplement_source(&candidate) {
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
    #[cfg(test)]
    let extras = match supplemental {
        SupplementalBatch::Complete(extras) => extras,
        SupplementalBatch::Incomplete => return Err(LockError::Incomplete.into()),
    };
    #[cfg(not(test))]
    let SupplementalBatch::Complete(extras) = supplemental;
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

#[cfg(test)]
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
    run_bound_request(
        &pipeline,
        raw,
        [1, 2, 3, 4],
        locale_chain,
        dictionaries,
        supplement,
    )
}

fn run_bound_request(
    pipeline: &Pipeline,
    raw: &str,
    session_hex: [u8; 4],
    locale_chain: &[crate::LocaleTag],
    dictionaries: &DictionaryBundle,
    supplement: impl FnOnce(
        &str,
        &DetectContext<'_>,
    ) -> std::result::Result<SupplementalBatch, LockError>,
) -> Result<LockOutput> {
    let kind = DocumentKind::Text;
    let normalized = normalize(raw);
    let ctx = DetectContext::new(locale_chain, dictionaries);
    let (baseline, vetoed) = pipeline
        .registry
        .detect_all_resolved(&normalized.text, &ctx)?;
    let extras = supplement(&normalized.text, &ctx)?;
    let plan = prepare_plan(pipeline, raw, &normalized, baseline, extras)?;
    let session = Session::new_with_session_hex_for_tests(crate::Scope::Ephemeral, session_hex)?;
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

fn valid_supplement_source(candidate: &Candidate) -> bool {
    #[cfg(test)]
    if candidate.source == "synthetic.supplement" && candidate.recognizer_id == candidate.source {
        return matches!(&candidate.class, PiiClass::Email | PiiClass::Name)
            || matches!(&candidate.class, PiiClass::Custom(name) if name == "synthetic");
    }
    #[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
    {
        candidate.recognizer_id == candidate.source
            && valid_redact_source(&candidate.source, &candidate.class)
    }
    #[cfg(not(all(feature = "experimental-benchmark-baseline-lock", unix)))]
    false
}

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
fn valid_redact_source(source: &str, class: &PiiClass) -> bool {
    let Some(label) = source.strip_prefix("redact-patched-coreml-v1:") else {
        return false;
    };
    label == label.to_ascii_lowercase()
        && gaze_recognizers::redact_live::label_class(&label.to_ascii_uppercase()).as_ref()
            == Ok(class)
}

/// Closed, ordered benchmark rules, validated before assembly or provider construction.
#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
pub struct BenchmarkLockPolicy(Vec<FrozenRule>);

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
impl TryFrom<Vec<crate::RuleSpec>> for BenchmarkLockPolicy {
    type Error = Error;
    fn try_from(rules: Vec<crate::RuleSpec>) -> Result<Self> {
        let mut closed = Vec::with_capacity(rules.len());
        for rule in rules {
            let (class, action) = match rule {
                crate::RuleSpec::Class { class, action } => (Some(class), action),
                crate::RuleSpec::Default { action } => (None, action),
                _ => return Err(LockError::UnsupportedScope.into()),
            };
            if !matches!(action, Action::Tokenize | Action::Preserve) {
                return Err(LockError::UnsupportedAction.into());
            }
            closed.push(match class {
                Some(class) => FrozenRule::Class(class, action),
                None => FrozenRule::Default(action),
            });
        }
        Ok(Self(closed))
    }
}

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
impl BenchmarkLockPolicy {
    /// Consume the independently assembled ruleless baseline, preserving its owned configuration.
    pub fn bind(self, mut pipeline: Pipeline) -> Result<BenchmarkBaselineLock> {
        if !pipeline.rules.is_empty()
            || pipeline.optimization_config.prefix_cache
            || !pipeline.safety_nets.is_empty()
            || pipeline.safety_net_registry.is_some()
        {
            return Err(LockError::UnsupportedScope.into());
        }
        if pipeline.registry.is_empty() {
            return Err(LockError::MissingBaseline.into());
        }
        for rule in self.0 {
            let rule: Arc<dyn Rule> = match rule {
                FrozenRule::Class(class, action) => Arc::new(ClassRule::new(class, action)),
                FrozenRule::Default(action) => Arc::new(DefaultRule::new(action)),
            };
            pipeline.rules.push(rule);
        }
        Ok(BenchmarkBaselineLock(pipeline))
    }
}

/// Sealed text-only baseline. No mutable pipeline or externally owned request session.
#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
pub struct BenchmarkBaselineLock(Pipeline);

/// Published only after full validation and successful shared emission/trace finalization.
#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
pub struct BenchmarkLockedText {
    pub text: String,
    pub manifest: Vec<EmittedTokenSpan>,
    pub trace: Vec<GazeLocalProtectionTraceItem>,
    pub session: Session,
    pub dispositions: Vec<Disposition>,
}

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
impl BenchmarkBaselineLock {
    pub fn clean_redact_text(
        &self,
        raw: &str,
        session_hex: [u8; 4],
        locale_chain: &[crate::LocaleTag],
        detector: &gaze_recognizers::redact_live::RedactDetector,
    ) -> Result<BenchmarkLockedText> {
        let output = run_bound_request(
            &self.0,
            raw,
            session_hex,
            locale_chain,
            &DictionaryBundle::default(),
            |normalized, _| {
                let detections = detector.try_detect(normalized).map_err(|error| {
                    if error.message == "incomplete_window" {
                        LockError::Incomplete
                    } else {
                        LockError::ProviderFailed
                    }
                })?;
                validate_redact_detections(normalized, &detections)?;
                Ok(SupplementalBatch::Complete(
                    detections
                        .into_iter()
                        .map(candidate_from_legacy_detection)
                        .collect(),
                ))
            },
        )?;
        Ok(BenchmarkLockedText {
            text: output.clean.text,
            manifest: output.clean.manifest,
            trace: output.trace,
            session: output.session,
            dispositions: output.plan.dispositions,
        })
    }

    pub fn restore_with_telemetry(
        &self,
        session: &Session,
        text: &str,
    ) -> Result<(RestoredText, RestoreTelemetry)> {
        self.0.restore_with_telemetry(session, text)
    }
}

#[cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]
fn validate_redact_detections(
    text: &str,
    detections: &[Detection],
) -> std::result::Result<(), LockError> {
    if detections.len() > MAX_CANDIDATES {
        return Err(LockError::InvalidBatch);
    }
    let mut end = 0;
    for detection in detections {
        if !valid_redact_source(&detection.source, &detection.class) {
            return Err(LockError::UnknownLabel);
        }
        let span = &detection.span;
        if span.start < end
            || span.start >= span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            return Err(LockError::InvalidBatch);
        }
        end = span.end;
    }
    Ok(())
}
