use super::*;

/// Strict boundary inputs. There is deliberately no tolerant or one-way mode.
/// Zero installed safety nets means primary-only protection, not PII completeness.
#[derive(Clone, Copy)]
pub struct ProtectionContext<'a> {
    locale_chain: &'a [crate::LocaleTag],
    dictionaries: &'a DictionaryBundle,
}

impl<'a> ProtectionContext<'a> {
    pub fn strict(
        locale_chain: &'a [crate::LocaleTag],
        dictionaries: &'a DictionaryBundle,
    ) -> Self {
        Self {
            locale_chain,
            dictionaries,
        }
    }
    pub fn locale_chain(self) -> &'a [crate::LocaleTag] {
        self.locale_chain
    }
    pub fn dictionaries(self) -> &'a DictionaryBundle {
        self.dictionaries
    }
}

/// Class-only failures safe to expose at an untrusted boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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
}

impl Pipeline {
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
                let locale = context
                    .locale_chain
                    .first()
                    .cloned()
                    .unwrap_or(crate::LocaleTag::Global);
                let models = registry
                    .resolve(&locale, ModelStage::Pass3SafetyNet)
                    .map_err(|error| match error {
                        ModelError::NoLocaleModelCoverage { .. }
                        | ModelError::LocaleNotSupported(_) => ProtectionError::UnsupportedCoverage,
                        _ => ProtectionError::SafetyNet,
                    })?;
                if models.is_empty() {
                    return Err(ProtectionError::UnsupportedCoverage);
                }
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
        self.validate_protection_context(context)?;
        let owned = transaction.snapshot_entries();
        let regex = transaction
            .restore_regex()
            .map_err(|_| ProtectionError::Provenance)?;
        let ranges = regex
            .as_ref()
            .map(|re| re.find_iter(input).map(|m| m.range()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut clean = String::with_capacity(input.len());
        let mut expected = String::with_capacity(input.len());
        let mut spans = Vec::new();
        let mut cursor = 0;
        for range in ranges {
            self.protect_gap(
                transaction,
                &input[cursor..range.start],
                context,
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
            spans.push(EmittedTokenSpan::new(
                clean_start..clean.len(),
                raw_start..expected.len(),
                entry.class.clone(),
            ));
            cursor = range.end;
        }
        self.protect_gap(
            transaction,
            &input[cursor..],
            context,
            &mut clean,
            &mut expected,
            &mut spans,
        )?;
        let manifest = Manifest::from_spans(spans);
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
            .restore_regex()
            .map_err(|_| ProtectionError::Provenance)?
            .map(|re| {
                re.find_iter(&clean)
                    .map(|found| found.range())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if actual_ranges
            != manifest
                .spans
                .iter()
                .map(|span| span.clean_span.clone())
                .collect::<Vec<_>>()
        {
            return Err(ProtectionError::Provenance);
        }
        let mut target = ProtectionTarget::Staged(transaction, PrefixCacheWriteMode::Suppress);
        let report = self
            .run_safety_nets_in_context(
                &mut target,
                &clean,
                &manifest,
                DocumentKind::Text,
                context.locale_chain,
                None,
                SafetyNetDecision::Observe { strict: true },
                context.dictionaries,
                true,
            )
            .map_err(|error| match error {
                Error::Protection(error) => error,
                _ => ProtectionError::SafetyNet,
            })?;
        for suspect in report.suspects {
            if suspect.span.start >= suspect.span.end || clean.get(suspect.span.clone()).is_none() {
                return Err(ProtectionError::Residual);
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
                return Err(ProtectionError::Residual);
            }
        }
        Ok(clean)
    }

    #[allow(clippy::too_many_arguments)]
    fn protect_gap(
        &self,
        transaction: &mut SessionTransaction<'_>,
        input: &str,
        context: ProtectionContext<'_>,
        clean: &mut String,
        expected: &mut String,
        spans: &mut Vec<EmittedTokenSpan>,
    ) -> std::result::Result<(), ProtectionError> {
        let clean_offset = clean.len();
        let raw_offset = expected.len();
        expected.push_str(input);
        if input.is_empty() {
            return Ok(());
        }
        let mut target = ProtectionTarget::Staged(transaction, PrefixCacheWriteMode::Suppress);
        let result = self
            .redact_text_with_manifest_uncached(
                &mut target,
                input,
                None,
                DocumentKind::Text,
                context.locale_chain,
                context.dictionaries,
                None,
            )
            .map_err(|_| ProtectionError::Primary)?;
        clean.push_str(&result.text);
        spans.extend(result.manifest.into_iter().map(|span| {
            EmittedTokenSpan::new(
                span.clean_span.start + clean_offset..span.clean_span.end + clean_offset,
                span.raw_span.start + raw_offset..span.raw_span.end + raw_offset,
                span.class,
            )
        }));
        Ok(())
    }
}
