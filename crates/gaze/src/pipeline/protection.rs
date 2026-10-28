use super::*;

/// Strict boundary inputs. There is deliberately no tolerant or one-way mode.
/// Zero installed safety nets means primary-only protection, not PII completeness.
#[derive(Clone, Copy)]
pub struct ProtectionContext<'a> {
    locale_chain: &'a [crate::LocaleTag],
    dictionaries: &'a DictionaryBundle,
    field: Option<gaze_types::FieldContext<'a>>,
}

impl<'a> ProtectionContext<'a> {
    pub fn strict(
        locale_chain: &'a [crate::LocaleTag],
        dictionaries: &'a DictionaryBundle,
    ) -> Self {
        Self {
            locale_chain,
            dictionaries,
            field: None,
        }
    }
    /// Attach the validated scalar location for this complete input leaf.
    pub fn with_field(mut self, field: &'a gaze_types::FieldProvenance) -> Self {
        self.field = Some(gaze_types::FieldContext::WholeInput(field));
        self
    }
    /// Attach a fragment's location without claiming the complete scalar value.
    pub fn with_field_fragment(
        mut self,
        field: &'a gaze_types::FieldProvenance,
        offset: usize,
    ) -> Self {
        self.field = Some(gaze_types::FieldContext::Fragment { field, offset });
        self
    }
    pub fn locale_chain(self) -> &'a [crate::LocaleTag] {
        self.locale_chain
    }
    pub fn dictionaries(self) -> &'a DictionaryBundle {
        self.dictionaries
    }
}

/// Typed failures whose Debug and Display diagnostics contain no input values.
/// Structured field paths must follow a separate disclosure policy when serialized.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtectionError {
    #[error("empty primary configuration")]
    EmptyPrimary,
    #[error("unsupported safety-net locale coverage")]
    UnsupportedCoverage,
    #[error("primary protection failed")]
    Primary,
    #[error("safety-net execution failed")]
    SafetyNet,
    #[error("residual suspect rejected")]
    Residual,
    #[error("token provenance or reversibility rejected")]
    Provenance,
    /// A confirmed sensitive field has no safe reversible rendering. No producer yet.
    #[error("confirmed sensitive field cannot be transformed: {reason}")]
    Untransformable {
        /// Closed reason without the sensitive value.
        reason: gaze_types::UntransformableReason,
        /// Structural location. Diagnostics hide its segments.
        field_path: gaze_types::FieldPath,
    },
}

impl ProtectionError {
    /// The variant name, the stable spelling a boundary reports to its client.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EmptyPrimary => "EmptyPrimary",
            Self::UnsupportedCoverage => "UnsupportedCoverage",
            Self::Primary => "Primary",
            Self::SafetyNet => "SafetyNet",
            Self::Residual => "Residual",
            Self::Provenance => "Provenance",
            Self::Untransformable { .. } => "Untransformable",
        }
    }
}

/// Why an outbound boundary refused a text leaf: the typed [`ProtectionError`] plus the classes
/// behind it. Debug and Display hide field-path segments. Serialized paths require a
/// separate disclosure policy; class names come from policy and net label mappings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
#[non_exhaustive]
pub struct BoundaryRefusal {
    pub error: ProtectionError,
    /// Set when the Resolve step's `Strict` fallback refused.
    pub fallback_reason: Option<FallbackReason>,
    /// Sorted and deduplicated. Admission names the suspect it rejected; a Resolve refusal
    /// names every class the configured nets flagged in the leaf.
    pub suspect_classes: Vec<PiiClass>,
}

impl BoundaryRefusal {
    fn new(error: ProtectionError) -> Self {
        Self {
            error,
            fallback_reason: None,
            suspect_classes: Vec::new(),
        }
    }

    fn residual(classes: impl IntoIterator<Item = PiiClass>) -> Self {
        let mut suspect_classes = classes.into_iter().collect::<Vec<_>>();
        suspect_classes.sort();
        suspect_classes.dedup();
        Self {
            suspect_classes,
            ..Self::new(ProtectionError::Residual)
        }
    }

    fn from_resolve_error(error: Error, report: &LeakReport) -> Self {
        match error {
            Error::Protection(error) => Self::new(error),
            Error::SafetyNetFallback(reason) => Self {
                fallback_reason: Some(reason),
                ..Self::residual(report.suspects.iter().map(|suspect| suspect.class.clone()))
            },
            Error::SafetyNet(_) | Error::SafetyNetSpanInvalid { .. } => {
                Self::new(ProtectionError::SafetyNet)
            }
            _ => Self::new(ProtectionError::Primary),
        }
    }
}

impl From<ProtectionError> for BoundaryRefusal {
    fn from(error: ProtectionError) -> Self {
        Self::new(error)
    }
}

/// The boundary's safety-net step: `gaze clean --safety-net-fallback strict`. A boundary never
/// deletes one-way, so whatever Resolve cannot tokenize is refused. Clean's default `Redact`
/// fallback also runs a second reversible tokenize batch for what the re-run flags; this one
/// refuses that text instead.
const BOUNDARY_DECISION: SafetyNetDecision = SafetyNetDecision::Resolve {
    on_residual: SafetyNetFallback::Strict,
};

impl Pipeline {
    /// Discover Rule sources before request protection, without learned detectors or nets.
    /// Output is discarded; staged Rule mappings remain available to every request field.
    pub fn discover_staged_rule_sources(
        &self,
        transaction: &mut SessionTransaction<'_>,
        input: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<(), BoundaryRefusal> {
        let mut pipeline = self.clone();
        pipeline.registry = Arc::new(self.registry.deterministic_only());
        if pipeline.registry.is_empty() {
            return Ok(());
        }
        pipeline.safety_nets.clear();
        #[cfg(feature = "bundled-recognizers")]
        {
            pipeline.safety_net_registry = None;
        }
        pipeline
            .resolve_boundary_text_transaction(transaction, input, locale_chain, dictionaries)
            .map(|_| ())
    }

    /// Find policy-protected repeats of staged Rule values in raw text coordinates.
    ///
    /// Boundary adapters can expand owned tokens, intersect these spans with the
    /// adjacent raw bytes, and preserve the original tokens. Hits may overlap and
    /// are never selected into a non-overlapping cover. This query runs no detectors
    /// or safety nets and changes no transaction state. Learned,
    /// validator-failed, and safety-net-only entries never seed this matcher.
    pub fn staged_rule_repeat_candidates(
        &self,
        transaction: &SessionTransaction<'_>,
        text: &str,
    ) -> Result<Vec<Candidate>> {
        let Some(matcher) = transaction.sweep_matcher_view()? else {
            return Ok(Vec::new());
        };
        let normalized = crate::normalize::normalize(text);
        let context = build_context(None);
        let hits = matcher
            .find(&normalized.text)
            .into_iter()
            .filter(|hit| {
                self.resolve_action(&hit.class, &context)
                    .action
                    .is_protective()
            })
            .collect::<Vec<_>>();
        hits.into_iter()
            .map(|hit| {
                let mut candidate = hit.candidate();
                candidate.span = crate::normalize::raw_range(hit.span, &normalized.spans)
                    .ok_or_else(|| {
                        clean_to_raw_mapping_error("unmappable boundary repeat geometry")
                    })?;
                Ok(candidate)
            })
            .collect()
    }

    /// Prove complete Rule coverage in the expanded original of a protected leaf.
    ///
    /// Every eligible occurrence, including overlaps and normalized spellings, must
    /// have every byte owned by a manifest token. This independently enumerates hits;
    /// it uses neither candidate selection nor the adapter's projection/union logic.
    /// No detector, model, mapping, evidence or matcher cache is changed.
    pub fn verify_staged_rule_coverage(
        &self,
        transaction: &SessionTransaction<'_>,
        protected: &str,
        max_expanded_bytes: usize,
    ) -> Result<()> {
        let original = transaction
            .restore_strict_text_bounded(protected, max_expanded_bytes)
            .map_err(|_| ProtectionError::Provenance)?;
        let mut covered = vec![false; original.len()];
        let (mut wire_cursor, mut original_cursor) = (0, 0);
        for range in transaction.restore_token_ranges(protected)? {
            original_cursor += range.start - wire_cursor;
            let value = transaction.restore_strict_text(&protected[range.clone()])?;
            let end = original_cursor + value.len();
            covered
                .get_mut(original_cursor..end)
                .ok_or_else(|| clean_to_raw_mapping_error("invalid owned coverage"))?
                .fill(true);
            original_cursor = end;
            wire_cursor = range.end;
        }
        let Some(matcher) = transaction.sweep_matcher_view()? else {
            return Ok(());
        };
        let normalized = crate::normalize::normalize(&original);
        let context = build_context(None);
        for hit in matcher.find(&normalized.text) {
            if !self
                .resolve_action(&hit.class, &context)
                .action
                .is_protective()
            {
                continue;
            }
            let raw = crate::normalize::raw_range(hit.span, &normalized.spans)
                .ok_or_else(|| clean_to_raw_mapping_error("unmappable Rule coverage"))?;
            if !covered[raw].iter().all(|byte| *byte) {
                return Err(Error::Protection(ProtectionError::Residual));
            }
        }
        Ok(())
    }

    /// Tokenize the raw intersection of a Rule repeat and an owned-token boundary.
    ///
    /// The fragment keeps the matched class and family and emits a metadata-only
    /// sweep audit event. It does not become an independent Rule source.
    pub fn tokenize_staged_rule_repeat_fragment(
        &self,
        transaction: &mut SessionTransaction<'_>,
        candidate: &Candidate,
        raw: &str,
    ) -> Result<String> {
        if candidate.recognizer_id != crate::sweep::SWEEP_ID
            || candidate.evidence != crate::EvidenceKind::Rule
            || self
                .resolve_action(&candidate.class, &build_context(None))
                .action
                != Action::Tokenize
        {
            return Err(Error::UnsupportedActionVariant);
        }
        let mut entry = RedactionEntry::new(
            crate::sweep::SWEEP_ID,
            candidate.class.clone(),
            Action::Tokenize,
            None,
            DocumentKind::Text,
            false,
            ConflictTier::ManifestSweep,
            crate::redaction_log::current_epoch_ms(),
            Some(transaction.audit_session_id().to_string()),
        )
        .with_recognizer_metadata(Some(crate::sweep::SWEEP_ID.into()), None);
        entry.provenance_stage = Some(crate::sweep::SWEEP_ID.into());
        entry.provenance_merged_from = Some("manifest_sweep:token_boundary".into());
        for logger in &self.redaction_loggers {
            logger.log(&entry)?;
        }
        transaction.tokenize_with_family(&candidate.token_family, &candidate.class, raw)
    }

    /// Resolve full-leaf safety-net findings after primary protection, keeping owned tokens.
    /// This reuses the core's bounded Resolve/Strict path and never promotes net values to Rule.
    pub fn resolve_staged_owned_text(
        &self,
        transaction: &mut SessionTransaction<'_>,
        input: &str,
        context: ProtectionContext<'_>,
        max_expanded_bytes: usize,
    ) -> std::result::Result<String, BoundaryRefusal> {
        #[cfg(feature = "bundled-recognizers")]
        let _inference_request = gaze_recognizers::inference_request();
        if self.safety_nets_len() == 0 {
            return Ok(input.to_owned());
        }
        self.validate_protection_context(context)?;
        let expected = transaction
            .restore_strict_text_bounded(input, max_expanded_bytes)
            .map_err(|_| ProtectionError::Provenance)?;
        let entries = transaction.snapshot_entries();
        let ranges = transaction
            .restore_token_ranges(input)
            .map_err(|_| ProtectionError::Provenance)?;
        let mut ledger = Ledger::default();
        let (mut clean_cursor, mut raw_cursor) = (0, 0);
        for range in ranges {
            let entry = entries
                .iter()
                .find(|entry| entry.token == input[range.clone()])
                .ok_or(ProtectionError::Provenance)?;
            raw_cursor += range.start - clean_cursor;
            let raw_end = raw_cursor + entry.raw.len();
            clean_cursor = range.end;
            ledger.existing_owned(EmittedTokenSpan::new(
                range,
                raw_cursor..raw_end,
                entry.class.clone(),
            ));
            raw_cursor = raw_end;
        }
        ledger.validate().map_err(|_| ProtectionError::Provenance)?;
        let mut clean = CleanText {
            text: input.to_owned(),
            manifest: ledger,
        };
        let mut report = self
            .run_safety_nets_in_context(
                &mut ProtectionTarget::Staged(transaction),
                &clean.text,
                clean.manifest.projection(),
                DocumentKind::Text,
                context.locale_chain,
                None,
                BOUNDARY_DECISION,
                context.dictionaries,
                SafetyNetExecution::Admission,
            )
            .map_err(|error| BoundaryRefusal::from_resolve_error(error, &LeakReport::default()))?
            .report;
        if reject_unprotected_suspects(input, clean.manifest.projection(), report.clone()).is_ok() {
            return Ok(clean.text);
        }
        self.apply_safety_net_policy(
            &mut ProtectionTarget::Staged(transaction),
            &mut clean,
            &mut report,
            DocumentKind::Text,
            context.locale_chain,
            None,
            BOUNDARY_DECISION,
            None,
        )
        .map_err(|error| BoundaryRefusal::from_resolve_error(error, &report))?;
        if transaction
            .restore_strict_text_bounded(&clean.text, max_expanded_bytes)
            .map_err(|_| ProtectionError::Provenance)?
            != expected
        {
            return Err(ProtectionError::Provenance.into());
        }
        self.admit_boundary_text_transaction(
            transaction,
            &clean.text,
            context.locale_chain,
            context.dictionaries,
        )?;
        Ok(clean.text)
    }

    /// Protects a text leaf for an outbound boundary such as `gaze-proxy`: the primary pipeline,
    /// then the configured safety nets through the Resolve step
    /// `gaze clean --safety-net-fallback strict` runs, so a net-flagged span becomes a
    /// restorable token instead of a refusal.
    ///
    /// Anything Resolve cannot tokenize is refused (`Strict` fallback, never a one-way
    /// deletion). This is not the admission proof: run [`Self::admit_boundary_text`] on the
    /// final text before it leaves. With no nets configured the output is the primary output.
    pub fn resolve_boundary_text(
        &self,
        session: &Session,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<String, BoundaryRefusal> {
        let mut target = ProtectionTarget::Live(session);
        self.resolve_boundary_text_target(
            &mut target,
            text,
            ProtectionContext::strict(locale_chain, dictionaries),
        )
    }

    /// [`Self::resolve_boundary_text`] against staged transaction state; commits nothing.
    pub fn resolve_boundary_text_transaction(
        &self,
        transaction: &mut SessionTransaction<'_>,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<String, BoundaryRefusal> {
        self.resolve_boundary_text_transaction_in_context(
            transaction,
            text,
            ProtectionContext::strict(locale_chain, dictionaries),
        )
    }

    /// Protect a staged leaf with its validated structured-field evidence.
    pub fn resolve_boundary_text_transaction_in_context(
        &self,
        transaction: &mut SessionTransaction<'_>,
        text: &str,
        context: ProtectionContext<'_>,
    ) -> std::result::Result<String, BoundaryRefusal> {
        let mut target = ProtectionTarget::Staged(transaction);
        self.resolve_boundary_text_target(&mut target, text, context)
    }

    fn resolve_boundary_text_target(
        &self,
        target: &mut ProtectionTarget<'_, '_>,
        text: &str,
        context: ProtectionContext<'_>,
    ) -> std::result::Result<String, BoundaryRefusal> {
        let mut report = LeakReport::default();
        self.clean_text_target(
            target,
            text,
            context.locale_chain,
            context.dictionaries,
            BOUNDARY_DECISION,
            &mut report,
            context.field,
        )
        .map(|clean| clean.text)
        .map_err(|error| BoundaryRefusal::from_resolve_error(error, &report))
    }

    /// [`Self::admit_safety_nets`] with the class of the rejected suspect in the error.
    pub fn admit_boundary_text(
        &self,
        session: &Session,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<(), BoundaryRefusal> {
        self.admit_boundary_text_transaction(
            &mut session.begin_transaction(),
            text,
            locale_chain,
            dictionaries,
        )
    }

    /// Validates the actual primary graph and installed safety-net locale coverage.
    /// Call before staging a structured operation, including operations with no strings.
    pub fn validate_protection_context(
        &self,
        context: ProtectionContext<'_>,
    ) -> std::result::Result<(), ProtectionError> {
        if self.registry.is_empty() {
            return Err(ProtectionError::EmptyPrimary);
        }
        let active = gaze_types::LocaleChain::from(context.locale_chain);
        if self
            .safety_nets
            .iter()
            .any(|net| !active.intersects(net.supported_locales()))
        {
            return Err(ProtectionError::UnsupportedCoverage);
        }
        #[cfg(feature = "bundled-recognizers")]
        if let Some(registry) = &self.safety_net_registry {
            if !registry.is_empty() {
                resolve_safety_net_models(registry, context.locale_chain, true).map_err(
                    |error| match error {
                        Error::Protection(error) => error,
                        _ => ProtectionError::SafetyNet,
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Protects a complete string leaf in caller-owned staging state; never commits.
    ///
    /// An error may leave mappings staged. Abandon the operation transaction on
    /// error; do not commit it or use earlier returned strings as proof. A returned
    /// string reflects this leaf's entry interpretation, not an immutable whole-
    /// operation proof. Multi-leaf callers must also reject cross-leaf collisions.
    /// Reconstructed safety-manifest raw coordinates refer to the expanded input
    /// interpretation: existing owned tokens stand for their stored raw bytes.
    /// Owned tokens retain their input interpretation. All installed applicable nets
    /// scan the complete final leaf, independently of observer optimization flags.
    pub fn protect_text_transaction(
        &self,
        transaction: &mut SessionTransaction<'_>,
        input: &str,
        context: ProtectionContext<'_>,
    ) -> std::result::Result<String, ProtectionError> {
        #[cfg(feature = "bundled-recognizers")]
        let _inference_request = gaze_recognizers::inference_request();
        self.validate_protection_context(context)?;
        let owned = transaction.snapshot_entries();
        let ranges = transaction
            .restore_token_ranges(input)
            .map_err(|_| ProtectionError::Provenance)?;
        let mut clean = String::with_capacity(input.len());
        let mut expected = String::with_capacity(input.len());
        let mut spans = Ledger::default();
        let mut cursor = 0;
        // Owned tokens split the leaf, so each gap is only part of the scalar.
        let split = !ranges.is_empty();
        let gap_context = |offset: usize| {
            if !split {
                return context;
            }
            let field = match context.field {
                Some(gaze_types::FieldContext::WholeInput(field)) => {
                    Some(gaze_types::FieldContext::Fragment { field, offset })
                }
                Some(gaze_types::FieldContext::Fragment {
                    field,
                    offset: base,
                }) => Some(gaze_types::FieldContext::Fragment {
                    field,
                    offset: base + offset,
                }),
                other => other,
            };
            ProtectionContext { field, ..context }
        };
        for range in ranges {
            self.protect_gap(
                transaction,
                &input[cursor..range.start],
                gap_context(cursor),
                &mut clean,
                &mut expected,
                &mut spans,
            )?;
            let entry = owned
                .iter()
                .find(|entry| entry.token == input[range.clone()])
                .ok_or(ProtectionError::Provenance)?;
            let clean_start = clean.len();
            let raw_start = expected.len();
            clean.push_str(&entry.token);
            expected.push_str(&entry.raw);
            spans.existing_owned(EmittedTokenSpan::new(
                clean_start..clean.len(),
                raw_start..expected.len(),
                entry.class.clone(),
            ));
            cursor = range.end;
        }
        self.protect_gap(
            transaction,
            &input[cursor..],
            gap_context(cursor),
            &mut clean,
            &mut expected,
            &mut spans,
        )?;
        spans.validate().map_err(|_| ProtectionError::Provenance)?;
        let manifest = spans.projection();
        let entries = transaction.snapshot_entries();
        let mut restored = String::new();
        let mut cursor = 0;
        for span in &manifest.spans {
            if span.clean_span.start < cursor {
                return Err(ProtectionError::Provenance);
            }
            let token = clean
                .get(span.clean_span.clone())
                .ok_or(ProtectionError::Provenance)?;
            let entry = entries
                .iter()
                .find(|entry| entry.token == token && entry.class == span.class)
                .ok_or(ProtectionError::Provenance)?;
            if expected.get(span.raw_span.clone()) != Some(entry.raw.as_str()) {
                return Err(ProtectionError::Provenance);
            }
            restored.push_str(&clean[cursor..span.clean_span.start]);
            restored.push_str(&entry.raw);
            cursor = span.clean_span.end;
        }
        restored.push_str(&clean[cursor..]);
        if restored != expected {
            return Err(ProtectionError::Provenance);
        }
        // Actual session restore must agree with provenance. A freshly minted spelling
        // in an originally literal gap must not acquire authority by coincidence.
        let actual_ranges = transaction
            .restore_token_ranges(&clean)
            .map_err(|_| ProtectionError::Provenance)?;
        if actual_ranges
            != manifest
                .spans
                .iter()
                .map(|span| span.clean_span.clone())
                .collect::<Vec<_>>()
        {
            return Err(ProtectionError::Provenance);
        }
        let mut target = ProtectionTarget::Staged(transaction);
        let report = self
            .run_safety_nets_in_context(
                &mut target,
                &clean,
                manifest,
                DocumentKind::Text,
                context.locale_chain,
                None,
                SafetyNetDecision::Observe { strict: true },
                context.dictionaries,
                SafetyNetExecution::Strict,
            )
            .map_err(|error| match error {
                Error::Protection(error) => error,
                _ => ProtectionError::SafetyNet,
            })?;
        reject_unprotected_suspects(&clean, manifest, report.report)
            .map_err(|refusal| refusal.error)?;
        Ok(clean)
    }

    /// Admit already-transformed text against configured safety nets, using live token ownership.
    /// This read-only snapshot does not roll back mappings already published by the caller.
    /// See [`Self::admit_safety_nets_transaction`] for the coverage and proof limits.
    pub fn admit_safety_nets(
        &self,
        session: &Session,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<(), ProtectionError> {
        self.admit_safety_nets_transaction(
            &mut session.begin_transaction(),
            text,
            locale_chain,
            dictionaries,
        )
    }

    /// Admit a complete final text leaf against all applicable configured nets.
    ///
    /// This neither transforms text nor commits mappings. It is not a primary-policy or
    /// reversibility proof: callers retain responsibility for those and for literal-token
    /// collisions. Token coverage uses the target's actual restore boundaries and values;
    /// nets receive raw coordinates in the expanded token interpretation.
    ///
    /// Raw-gap, malformed suspects and net/registry errors reject. Verified token-contained
    /// reflags are safe even with class disagreement. Observer skip optimizations do not
    /// apply. No nets or locale-skipped custom nets leave coverage gaps; model-registry
    /// resolution still fails closed. Public clean policy and strict protection are unchanged.
    pub fn admit_safety_nets_transaction(
        &self,
        transaction: &mut SessionTransaction<'_>,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<(), ProtectionError> {
        self.admit_boundary_text_transaction(transaction, text, locale_chain, dictionaries)
            .map_err(|refusal| refusal.error)
    }

    /// [`Self::admit_safety_nets_transaction`] with the class of the rejected suspect in the
    /// error.
    pub fn admit_boundary_text_transaction(
        &self,
        transaction: &mut SessionTransaction<'_>,
        text: &str,
        locale_chain: &[crate::LocaleTag],
        dictionaries: &DictionaryBundle,
    ) -> std::result::Result<(), BoundaryRefusal> {
        if self.safety_nets_len() == 0 {
            return Ok(());
        }
        let entries = transaction.snapshot_entries();
        let ranges = transaction
            .restore_token_ranges(text)
            .map_err(|_| ProtectionError::Provenance)?;
        let mut spans = Ledger::default();
        let mut clean_cursor = 0;
        let mut raw_cursor = 0;
        for range in ranges {
            let entry = entries
                .iter()
                .find(|entry| entry.token == text[range.clone()])
                .ok_or(ProtectionError::Provenance)?;
            raw_cursor += range.start - clean_cursor;
            let raw_end = raw_cursor + entry.raw.len();
            clean_cursor = range.end;
            spans.existing_owned(EmittedTokenSpan::new(
                range,
                raw_cursor..raw_end,
                entry.class.clone(),
            ));
            raw_cursor = raw_end;
        }
        spans.validate().map_err(|_| ProtectionError::Provenance)?;
        let manifest = spans.projection();
        let report = self
            .run_safety_nets_in_context(
                &mut ProtectionTarget::Staged(transaction),
                text,
                manifest,
                DocumentKind::Text,
                locale_chain,
                None,
                SafetyNetDecision::Observe { strict: true },
                dictionaries,
                SafetyNetExecution::Admission,
            )
            .map_err(|error| match error {
                Error::Protection(error) => error,
                _ => ProtectionError::SafetyNet,
            })?;
        reject_unprotected_suspects(text, manifest, report.report)
    }

    #[allow(clippy::too_many_arguments)]
    fn protect_gap(
        &self,
        transaction: &mut SessionTransaction<'_>,
        input: &str,
        context: ProtectionContext<'_>,
        clean: &mut String,
        expected: &mut String,
        spans: &mut Ledger,
    ) -> std::result::Result<(), ProtectionError> {
        let clean_offset = clean.len();
        let raw_offset = expected.len();
        expected.push_str(input);
        if input.is_empty() {
            return Ok(());
        }
        let mut target = ProtectionTarget::Staged(transaction);
        let result = self
            .redact_text_with_manifest_uncached(
                &mut target,
                input,
                None,
                DocumentKind::Text,
                context.locale_chain,
                context.dictionaries,
                None,
                context.field,
            )
            .map_err(|_| ProtectionError::Primary)?;
        clean.push_str(&result.text);
        spans
            .append(result.manifest, raw_offset, clean_offset)
            .map_err(|_| ProtectionError::Provenance)?;
        Ok(())
    }
}

// Shared by strict reversible protection and net-only admission. Neither may accept raw gaps.
fn reject_unprotected_suspects(
    clean: &str,
    manifest: &Manifest,
    report: LeakReport,
) -> std::result::Result<(), BoundaryRefusal> {
    for suspect in report.suspects {
        if suspect.span.start >= suspect.span.end || clean.get(suspect.span.clone()).is_none() {
            return Err(BoundaryRefusal::residual([suspect.class]));
        }
        // Coverage is geometric: even a backend class disagreement entirely inside
        // verified token bytes cannot expose raw data. Any raw gap still rejects.
        let mut cursor = suspect.span.start;
        for span in &manifest.spans {
            if span.clean_span.end <= cursor {
                continue;
            }
            if span.clean_span.start > cursor {
                break;
            }
            cursor = span.clean_span.end;
            if cursor >= suspect.span.end {
                break;
            }
        }
        if cursor < suspect.span.end {
            return Err(BoundaryRefusal::residual([suspect.class]));
        }
    }
    Ok(())
}
