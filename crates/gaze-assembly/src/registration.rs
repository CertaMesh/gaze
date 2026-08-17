use gaze::{CollisionMembership, Detector, LocaleTag, PipelineBuilder, Recognizer, Rule};

/// [`PipelineBuilder`] wrapper that counts every recognizer it registers.
///
/// `build_pipeline`'s `NoRecognizers` guard reads [`Self::registered_recognizers`]
/// instead of re-deriving "would this recognizer register" from policy and
/// rulepack metadata. Every registration path in this crate goes through this
/// type, so the guard cannot drift from what actually got registered — a drift
/// that previously let a zero-recognizer pipeline build and preserve every
/// byte (audit 7201 S10-F1).
#[derive(Default)]
pub(crate) struct AssemblyBuilder {
    inner: PipelineBuilder,
    registered_recognizers: usize,
}

impl AssemblyBuilder {
    /// Recognizers registered so far (policy detectors, rulepack recognizers,
    /// context dictionaries, and NER all count; collision memberships, anchor
    /// cue bundles, and rules do not detect anything and are excluded).
    pub(crate) fn registered_recognizers(&self) -> usize {
        self.registered_recognizers
    }

    pub(crate) fn detector<D>(&mut self, detector: D)
    where
        D: Detector + 'static,
    {
        self.map(|builder| builder.detector(detector));
        self.registered_recognizers += 1;
    }

    pub(crate) fn recognizer<R>(&mut self, recognizer: R)
    where
        R: Recognizer + 'static,
    {
        self.map(|builder| builder.recognizer(recognizer));
        self.registered_recognizers += 1;
    }

    pub(crate) fn register_collision(
        &mut self,
        recognizer_id: impl Into<String>,
        membership: CollisionMembership,
    ) {
        self.map(|builder| builder.register_collision(recognizer_id, membership));
    }

    pub(crate) fn register_anchor_cue_bundle(
        &mut self,
        locale: LocaleTag,
        anchor_key: String,
        names: Vec<String>,
        window_chars: Option<u16>,
    ) {
        self.map(|builder| {
            builder.register_anchor_cue_bundle(locale, anchor_key, names, window_chars)
        });
    }

    pub(crate) fn rule<R>(&mut self, rule: R)
    where
        R: Rule + 'static,
    {
        self.map(|builder| builder.rule(rule));
    }

    pub(crate) fn into_inner(self) -> PipelineBuilder {
        self.inner
    }

    // `PipelineBuilder` methods consume `self`; take it out, apply, put it back.
    fn map(&mut self, apply: impl FnOnce(PipelineBuilder) -> PipelineBuilder) {
        let builder = std::mem::take(&mut self.inner);
        self.inner = apply(builder);
    }
}
