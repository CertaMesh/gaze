use std::collections::HashMap;
use std::ops::Range;

use gaze_types::{
    LeakReport, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};

/// Deterministic safety-net implementation for tests.
///
/// `MockSafetyNet` is observer-only: it never mutates clean text and only emits
/// metadata suspects through the shared `SafetyNet` contract.
#[derive(Debug, Clone)]
pub struct MockSafetyNet {
    id: String,
    locales: Vec<LocaleTag>,
    reports: HashMap<Option<String>, LeakReport>,
    raw_suspects: HashMap<Option<String>, Vec<MockLeakSuspect>>,
    error: Option<SafetyNetError>,
}

impl MockSafetyNet {
    /// Builds a mock that returns the supplied report for text/doc-level checks.
    pub fn new(report: LeakReport) -> Self {
        Self::with_reports(HashMap::from([(None, report)]))
    }

    /// Builds a mock from per-field reports.
    ///
    /// Use `None` for text/doc-level checks and `Some("$.path".to_string())`
    /// for structured field checks.
    pub fn with_reports(reports: HashMap<Option<String>, LeakReport>) -> Self {
        Self {
            id: "mock-safety-net".to_string(),
            locales: vec![LocaleTag::Global],
            reports,
            raw_suspects: HashMap::new(),
            error: None,
        }
    }

    /// Builds a mock from raw suspect templates that are correlated against the
    /// runtime manifest using `Manifest::diff_against`.
    pub fn with_raw_suspects(raw_suspects: HashMap<Option<String>, Vec<MockLeakSuspect>>) -> Self {
        Self {
            id: "mock-safety-net".to_string(),
            locales: vec![LocaleTag::Global],
            reports: HashMap::new(),
            raw_suspects,
            error: None,
        }
    }

    /// Replaces the stable backend identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Replaces supported locales. An empty list means global.
    pub fn with_locales(mut self, locales: Vec<LocaleTag>) -> Self {
        self.locales = locales;
        self
    }

    /// Configures a typed error returned from every check.
    pub fn with_error(mut self, error: SafetyNetError) -> Self {
        self.error = Some(error);
        self
    }

    fn report_for_path(&self, field_path: Option<&str>) -> Option<&LeakReport> {
        let key = field_path.map(str::to_string);
        self.reports.get(&key)
    }

    fn raw_suspects_for_path(&self, field_path: Option<&str>) -> Option<&[MockLeakSuspect]> {
        let key = field_path.map(str::to_string);
        self.raw_suspects.get(&key).map(Vec::as_slice)
    }
}

impl SafetyNet for MockSafetyNet {
    fn id(&self) -> &str {
        &self.id
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn check(
        &self,
        _clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }

        if let Some(report) = self.report_for_path(context.field_path) {
            return Ok(report.suspects.clone());
        }

        let Some(raw_suspects) = self.raw_suspects_for_path(context.field_path) else {
            return Ok(Vec::new());
        };

        Ok(raw_suspects
            .iter()
            .filter_map(|raw| {
                let kind = context.manifest.diff_against(&raw.span, &raw.class)?;
                Some(LeakSuspect {
                    span: raw.span.clone(),
                    class: raw.class.clone(),
                    safety_net_id: self.id.clone(),
                    score: raw.score,
                    kind,
                    raw_label: raw.raw_label.clone(),
                    field_path: context.field_path.map(str::to_string),
                })
            })
            .collect())
    }
}

/// Raw suspect template for manifest-correlated mock reports.
#[derive(Debug, Clone, PartialEq)]
pub struct MockLeakSuspect {
    /// Byte span in clean text.
    pub span: Range<usize>,
    /// Mapped PII class for the suspect.
    pub class: PiiClass,
    /// Optional deterministic score.
    pub score: Option<f32>,
    /// Raw backend label, never source text.
    pub raw_label: String,
}

impl MockLeakSuspect {
    /// Creates a raw suspect template.
    pub fn new(span: Range<usize>, class: PiiClass, raw_label: impl Into<String>) -> Self {
        Self {
            span,
            class,
            score: None,
            raw_label: raw_label.into(),
        }
    }

    /// Attaches a deterministic score.
    pub fn with_score(mut self, score: f32) -> Self {
        self.score = Some(score);
        self
    }
}
