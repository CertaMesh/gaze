#![cfg_attr(docsrs, feature(doc_cfg))]

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::Range;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Shared detector contract for text-only PII detection.
pub trait Detector: Send + Sync {
    /// Detect PII spans in the supplied input string.
    fn detect(&self, input: &str) -> Vec<Detection>;
}

/// The category of a detected PII span.
///
/// Built-in variants: `Email`, `Name`, `Location`, `Organization`. Tenant-specific PII
/// (case references, titles, internal codes) is carried as `PiiClass::Custom(String)`.
/// **There is no `Phone` variant** -- phone detection is provided by recognizers in
/// `gaze-recognizers` and surfaces as either a `Custom("phone")` class or a class
/// defined by a rulepack.
///
/// `PiiClass` is exhaustive. Match every variant explicitly so new built-in classes
/// force call sites to review their handling at compile time:
///
/// ```rust
/// use gaze_types::PiiClass;
///
/// fn label(class: &PiiClass) -> &'static str {
///     match class {
///         PiiClass::Email        => "email",
///         PiiClass::Name         => "name",
///         PiiClass::Location     => "location",
///         PiiClass::Organization => "org",
///         PiiClass::Custom(_)    => "pii",
///     }
/// }
/// ```
///
/// Policy TOML uses the lowercase forms `email` / `name` / `location` / `organization`,
/// and tenant classes are spelled like `custom:case_ref` (lowercase, snake_case).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PiiClass {
    /// Email address class.
    Email,
    /// Person name class.
    Name,
    /// Location class.
    Location,
    /// Organization class.
    Organization,
    /// Tenant- or policy-defined class.
    Custom(String),
}

/// Built-in class labels in stable display order.
pub const BUILTIN_CLASS_NAMES: &[&str] = &["Email", "Name", "Location", "Organization"];

impl PiiClass {
    /// Parses a policy class name into the shared class vocabulary.
    pub fn from_policy_name(input: &str) -> Option<Self> {
        match input {
            "email" => Some(Self::Email),
            "name" => Some(Self::Name),
            "location" => Some(Self::Location),
            "organization" => Some(Self::Organization),
            custom if custom.starts_with("custom:") => {
                let name = custom.trim_start_matches("custom:");
                (!name.trim().is_empty()).then(|| Self::custom(name))
            }
            _ => None,
        }
    }

    /// Returns the built-in class variants.
    pub fn builtin_variants() -> &'static [PiiClass] {
        &[
            PiiClass::Email,
            PiiClass::Name,
            PiiClass::Location,
            PiiClass::Organization,
        ]
    }

    /// Builds a normalized custom class name.
    pub fn custom(name: &str) -> Self {
        let mut normalized = String::new();
        let mut pending_underscore = false;
        for ch in name.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                if pending_underscore && !normalized.is_empty() {
                    normalized.push('_');
                }
                normalized.push(ch.to_ascii_lowercase());
                pending_underscore = false;
            } else {
                pending_underscore = true;
            }
        }

        Self::Custom(normalized)
    }

    /// Returns the normalized custom class name for custom classes.
    pub fn as_custom_name(&self) -> Option<&str> {
        match self {
            Self::Custom(name) => Some(name.as_str()),
            Self::Email | Self::Name | Self::Location | Self::Organization => None,
        }
    }

    /// Returns the audit/token display label for this class.
    pub fn class_name(&self) -> String {
        match self {
            Self::Email => BUILTIN_CLASS_NAMES[0].to_string(),
            Self::Name => BUILTIN_CLASS_NAMES[1].to_string(),
            Self::Location => BUILTIN_CLASS_NAMES[2].to_string(),
            Self::Organization => BUILTIN_CLASS_NAMES[3].to_string(),
            Self::Custom(name) => format!("Custom:{name}"),
        }
    }

    /// Returns the canonical audit/serde label for this class.
    pub fn to_canonical_str(&self) -> String {
        match self {
            Self::Email => "email".to_string(),
            Self::Name => "name".to_string(),
            Self::Location => "location".to_string(),
            Self::Organization => "organization".to_string(),
            Self::Custom(name) => format!("custom:{name}"),
        }
    }

    /// Parses the canonical audit/serde label for a PII class.
    pub fn from_canonical_str(value: &str) -> Option<Self> {
        match value {
            "email" | "Email" => Some(Self::Email),
            "name" | "Name" => Some(Self::Name),
            "location" | "Location" => Some(Self::Location),
            "organization" | "Organization" => Some(Self::Organization),
            custom if custom.starts_with("custom:") => {
                let name = &custom["custom:".len()..];
                (!name.is_empty()).then(|| Self::Custom(name.to_string()))
            }
            _ => None,
        }
    }
}

impl Serialize for PiiClass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_canonical_str())
    }
}

impl<'de> Deserialize<'de> for PiiClass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PiiClassVisitor;

        impl Visitor<'_> for PiiClassVisitor {
            type Value = PiiClass;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a canonical PII class string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                PiiClass::from_canonical_str(value)
                    .ok_or_else(|| E::custom(format!("unknown PII class {value}")))
            }
        }

        deserializer.deserialize_str(PiiClassVisitor)
    }
}

/// A candidate recognizer/class pair that lost ambiguity resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LosingCandidate {
    /// PII class proposed by the losing recognizer.
    pub class: PiiClass,
    /// Stable recognizer identifier for traceability.
    pub recognizer_id: String,
}

impl LosingCandidate {
    /// Builds a losing ambiguity candidate.
    pub fn new(class: PiiClass, recognizer_id: impl Into<String>) -> Self {
        Self {
            class,
            recognizer_id: recognizer_id.into(),
        }
    }
}

/// Structured metadata describing an ambiguity outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AmbiguityRecord {
    /// The family-level class assigned when disambiguation failed.
    pub ambiguity_class: PiiClass,
    /// Variants that could not be disambiguated.
    ///
    /// Producers must keep this list stable by sorting `recognizer_id` ascending.
    pub losing_candidates: Vec<LosingCandidate>,
    /// Why disambiguation failed.
    pub reason: AmbiguityReason,
}

impl AmbiguityRecord {
    /// Builds a structured ambiguity record.
    pub fn new(
        ambiguity_class: PiiClass,
        losing_candidates: Vec<LosingCandidate>,
        reason: AmbiguityReason,
    ) -> Self {
        Self {
            ambiguity_class,
            losing_candidates,
            reason,
        }
    }
}

/// Closed set of ambiguity outcomes recorded by the audit side-channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityReason {
    /// Span matched a multi-recognizer family and no anchor cue resolved it.
    NoAnchor,
    /// Multiple validator-stage recognizers remained viable for the same span.
    ValidatorIndeterminate,
    /// Span matched recognizers across two or more distinct PII class families.
    MultiFamilyMatch,
    /// Multiple variants had the same precedence and no discriminator resolved them.
    PrecedenceTie,
}

/// Closed validator failure reasons recorded by audit metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ValidatorFailReason {
    /// Luhn checksum validation failed.
    LuhnFailed,
    /// IBAN MOD-97 validation failed.
    IbanMod97Failed,
    /// Email RFC-style validation failed.
    EmailRfcFailed,
    /// E.164 phone validation failed.
    E164PhoneFailed,
}

/// A detected span and its class/source metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Detection {
    /// Byte span in the original input.
    pub span: Range<usize>,
    /// PII class assigned to the span.
    pub class: PiiClass,
    /// Detector source identifier.
    pub source: String,
}

impl Detection {
    /// Builds a detected PII span.
    pub fn new(span: Range<usize>, class: PiiClass, source: impl Into<String>) -> Self {
        Self {
            span,
            class,
            source: source.into(),
        }
    }
}

/// Observer-only post-clean check (Pass 3 in the detection pipeline).
///
/// Runs against already-tokenized output. May report suspected missed PII via
/// [`LeakReport`] but **must not** mutate the token manifest, the `CleanDocument`,
/// or the restore path. Safety nets are additive defense-in-depth, not a replacement
/// for Pass 1/2 detection.
///
/// Activate at runtime with `Pipeline::with_safety_net` (post-build) or
/// `PipelineBuilder::register_safety_net` (during build), or via the CLI
/// `--safety-net=<name>` flag.
///
/// If a safety net reports a suspected miss, the caller decides the response; the
/// pipeline never silently re-cleans based on safety net output.
pub trait SafetyNet: Send + Sync {
    /// Stable backend identifier used in telemetry and audit rows.
    fn id(&self) -> &str;

    /// Locale tags supported by this safety net. Empty means global.
    fn supported_locales(&self) -> &[LocaleTag];

    /// Checks clean text for possible PII that the manifest did not cover.
    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError>;
}

/// Context passed to a privacy safety net.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SafetyNetContext<'a> {
    /// Tokens emitted by the pseudonymization pipeline for this text segment.
    pub manifest: &'a Manifest,
    /// Active session-level locale chain. For `RawDocument::Structured`, locale
    /// gating uses this same session-level chain across all fields; structured
    /// fields do not carry per-field locale annotations.
    pub locale_chain: &'a [LocaleTag],
    /// Source document kind being checked.
    pub document_kind: DocumentKind,
    /// Optional audit session identifier.
    pub session_id: Option<&'a str>,
    /// Structured-document field path, such as `$.user.email`.
    pub field_path: Option<&'a str>,
}

impl<'a> SafetyNetContext<'a> {
    /// Builds safety-net context for one clean text segment.
    pub fn new(
        manifest: &'a Manifest,
        locale_chain: &'a [LocaleTag],
        document_kind: DocumentKind,
        session_id: Option<&'a str>,
        field_path: Option<&'a str>,
    ) -> Self {
        Self {
            manifest,
            locale_chain,
            document_kind,
            session_id,
            field_path,
        }
    }
}

/// A replacement emitted by the pseudonymization pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EmittedTokenSpan {
    /// Byte span in the clean text.
    pub clean_span: Range<usize>,
    /// Byte span in the raw text that produced the token.
    pub raw_span: Range<usize>,
    /// PII class represented by the emitted token.
    pub class: PiiClass,
}

impl EmittedTokenSpan {
    /// Builds an emitted token span.
    pub fn new(clean_span: Range<usize>, raw_span: Range<usize>, class: PiiClass) -> Self {
        Self {
            clean_span,
            raw_span,
            class,
        }
    }
}

/// Set of emitted token spans for one clean text segment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Manifest {
    /// Spans sorted by `clean_span.start`.
    pub spans: Vec<EmittedTokenSpan>,
}

impl Manifest {
    /// Builds a manifest from spans and sorts them by clean byte start.
    pub fn from_spans(mut spans: Vec<EmittedTokenSpan>) -> Self {
        spans.sort_by_key(|span| (span.clean_span.start, span.clean_span.end));
        Self { spans }
    }

    /// Diffs one safety-net suspect span against emitted token coverage.
    ///
    /// Returns `None` when the suspect span is continuously covered by emitted
    /// token spans of the same class. Internal gaps return
    /// `LeakKind::PartialBleed`. When multiple uncovered gaps exist, this method
    /// deterministically returns the first gap by byte offset; full gap
    /// enumeration is intentionally deferred to a future report format.
    pub fn diff_against(
        &self,
        suspect_span: &Range<usize>,
        suspect_class: &PiiClass,
    ) -> Option<LeakKind> {
        if suspect_span.is_empty() {
            return None;
        }

        let start_idx = self
            .spans
            .partition_point(|span| span.clean_span.end <= suspect_span.start);
        let overlapping = self.spans[start_idx..]
            .iter()
            .take_while(|span| span.clean_span.start < suspect_span.end)
            .filter(|span| ranges_overlap(&span.clean_span, suspect_span))
            .collect::<Vec<_>>();

        if overlapping.is_empty() {
            return Some(LeakKind::Uncovered);
        }

        let mut cursor = suspect_span.start;
        let mut first_mismatch = None::<&EmittedTokenSpan>;
        for span in overlapping {
            if span.clean_span.start > cursor {
                return Some(LeakKind::PartialBleed {
                    uncovered: cursor..span.clean_span.start.min(suspect_span.end),
                });
            }

            if span.clean_span.end > cursor {
                if first_mismatch.is_none() && &span.class != suspect_class {
                    first_mismatch = Some(span);
                }
                cursor = cursor.max(span.clean_span.end.min(suspect_span.end));
                if cursor >= suspect_span.end {
                    break;
                }
            }
        }

        if cursor < suspect_span.end {
            return Some(LeakKind::PartialBleed {
                uncovered: cursor..suspect_span.end,
            });
        }

        first_mismatch.map(|span| LeakKind::ClassMismatch {
            pipeline_class: span.class.clone(),
            safety_net_class: suspect_class.clone(),
        })
    }
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

/// Suspected leak reported by an observer-only safety net.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LeakSuspect {
    /// Byte span in clean text.
    pub span: Range<usize>,
    /// Mapped PII class for the suspect.
    pub class: PiiClass,
    /// Safety-net backend identifier.
    pub safety_net_id: String,
    /// Optional backend confidence score.
    pub score: Option<f32>,
    /// Leak classification after manifest correlation.
    pub kind: LeakKind,
    /// Raw backend label after validation/mapping, never source text.
    pub raw_label: String,
    /// Optional structured field path.
    pub field_path: Option<String>,
}

impl LeakSuspect {
    /// Builds a safety-net leak suspect.
    pub fn new(
        span: Range<usize>,
        class: PiiClass,
        safety_net_id: impl Into<String>,
        score: Option<f32>,
        kind: LeakKind,
        raw_label: impl Into<String>,
        field_path: Option<String>,
    ) -> Self {
        Self {
            span,
            class,
            safety_net_id: safety_net_id.into(),
            score,
            kind,
            raw_label: raw_label.into(),
            field_path,
        }
    }
}

/// The category of a suspected missed PII span.
///
/// `LeakKind` is `#[non_exhaustive]`. Match with a wildcard for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeakKind {
    /// No same-class emitted token overlaps the suspect span.
    Uncovered,
    /// The suspect is only partly covered; `uncovered` is the first gap.
    PartialBleed {
        /// First uncovered byte range in the suspect span.
        uncovered: Range<usize>,
    },
    /// The suspect is continuously covered, but by a different class.
    ClassMismatch {
        /// Class emitted by the pipeline.
        pipeline_class: PiiClass,
        /// Class reported by the safety net.
        safety_net_class: PiiClass,
    },
}

/// Bytes-free telemetry emitted by safety-net orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LeakReportTelemetry {
    /// Safety net skipped because the session-level locale chain did not match.
    LocaleSkipped {
        /// Safety-net backend identifier.
        safety_net_id: String,
        /// Document kind checked.
        document_kind: DocumentKind,
        /// Optional structured field path when skip was recorded per field.
        field_path: Option<String>,
    },
}

/// Aggregate leak report statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LeakReportStats {
    /// Number of suspects reported.
    pub suspect_count: usize,
    /// Number of uncovered suspects.
    pub uncovered_count: usize,
    /// Number of partial-bleed suspects.
    pub partial_bleed_count: usize,
    /// Number of class-mismatch suspects.
    pub class_mismatch_count: usize,
    /// Number of locale-skip telemetry events.
    pub locale_skipped_count: usize,
}

/// Signed document-context metadata carried inside a session snapshot envelope.
///
/// This extension is the v0.7 bridge for `gaze-document`: it is safe to serialize
/// inside the owner-only snapshot envelope, while agent-facing files keep using
/// non-sensitive mirrors. The single `schema_version` is bundle-level; sub-files
/// do not carry independent schema versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DocumentExtension {
    /// Bundle-level schema version shared by clean, layout, preview, report, and manifest files.
    pub schema_version: u16,
    /// SHA-256 of `clean.md` NFC-normalized bytes.
    pub clean_md_sha256: [u8; 32],
    /// SHA-256 of canonical `layout.json` bytes.
    pub layout_json_sha256: [u8; 32],
    /// SHA-256 of canonical `report.json` bytes.
    pub report_json_sha256: [u8; 32],
    /// SHA-256 of `preview-redacted.png` bytes when a preview is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_png_sha256: Option<[u8; 32]>,
    /// Page count reported for the source document.
    pub page_count: u32,
    /// Audit session id mirrored from the writing session for cross-pane correlation.
    pub audit_session_id: String,
    /// Signed clean.md byte spans for every emitted token.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clean_spans: Vec<EmittedTokenSpan>,
    /// Codec audit rows for the decode path that produced this document extension.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codec_audit: Vec<CodecAuditRow>,
}

impl DocumentExtension {
    /// Starts a document extension builder for one bundle schema version.
    pub fn builder(schema_version: u16) -> DocumentExtensionBuilder {
        DocumentExtensionBuilder {
            schema_version,
            clean_md_sha256: None,
            layout_json_sha256: None,
            report_json_sha256: None,
            preview_png_sha256: None,
            page_count: None,
            audit_session_id: None,
            clean_spans: Vec::new(),
            codec_audit: Vec::new(),
        }
    }
}

/// Builder for [`DocumentExtension`] that requires signed integrity-binding fields.
#[derive(Debug, Clone)]
#[must_use]
pub struct DocumentExtensionBuilder {
    schema_version: u16,
    clean_md_sha256: Option<[u8; 32]>,
    layout_json_sha256: Option<[u8; 32]>,
    report_json_sha256: Option<[u8; 32]>,
    preview_png_sha256: Option<[u8; 32]>,
    page_count: Option<u32>,
    audit_session_id: Option<String>,
    clean_spans: Vec<EmittedTokenSpan>,
    codec_audit: Vec<CodecAuditRow>,
}

impl DocumentExtensionBuilder {
    pub fn clean_md_sha256(mut self, hash: [u8; 32]) -> Self {
        self.clean_md_sha256 = Some(hash);
        self
    }

    pub fn layout_json_sha256(mut self, hash: [u8; 32]) -> Self {
        self.layout_json_sha256 = Some(hash);
        self
    }

    pub fn report_json_sha256(mut self, hash: [u8; 32]) -> Self {
        self.report_json_sha256 = Some(hash);
        self
    }

    pub fn preview_png_sha256(mut self, hash: [u8; 32]) -> Self {
        self.preview_png_sha256 = Some(hash);
        self
    }

    pub fn page_count(mut self, page_count: u32) -> Self {
        self.page_count = Some(page_count);
        self
    }

    pub fn audit_session_id(mut self, audit_session_id: impl Into<String>) -> Self {
        self.audit_session_id = Some(audit_session_id.into());
        self
    }

    pub fn clean_spans(mut self, clean_spans: Vec<EmittedTokenSpan>) -> Self {
        self.clean_spans = clean_spans;
        self
    }

    pub fn codec_audit(mut self, codec_audit: Vec<CodecAuditRow>) -> Self {
        self.codec_audit = codec_audit;
        self
    }

    pub fn build(self) -> Result<DocumentExtension, DocumentExtensionError> {
        Ok(DocumentExtension {
            schema_version: self.schema_version,
            clean_md_sha256: self
                .clean_md_sha256
                .ok_or(DocumentExtensionError::MissingField("clean_md_sha256"))?,
            layout_json_sha256: self
                .layout_json_sha256
                .ok_or(DocumentExtensionError::MissingField("layout_json_sha256"))?,
            report_json_sha256: self
                .report_json_sha256
                .ok_or(DocumentExtensionError::MissingField("report_json_sha256"))?,
            preview_png_sha256: self.preview_png_sha256,
            page_count: self
                .page_count
                .ok_or(DocumentExtensionError::MissingField("page_count"))?,
            audit_session_id: self
                .audit_session_id
                .ok_or(DocumentExtensionError::MissingField("audit_session_id"))?,
            clean_spans: self.clean_spans,
            codec_audit: self.codec_audit,
        })
    }
}

/// Errors returned while building a [`DocumentExtension`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DocumentExtensionError {
    #[error("missing document extension field: {0}")]
    MissingField(&'static str),
}

/// Provenance of text extracted from a document or transcript source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextOrigin {
    /// Text came from OCR over pixels.
    Ocr,
    /// Text came from an embedded text layer.
    EmbeddedText,
    /// Text came from an audio/video transcript.
    Transcript,
    /// Text came from multiple extraction paths.
    Hybrid,
}

/// Orthogonal document codec capabilities delivered or advertised by a codec.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CodecCapabilitySet {
    /// Codec can emit text.
    pub text: bool,
    /// Codec can emit layout geometry.
    pub layout: bool,
    /// Codec can emit confidence buckets.
    pub confidence: bool,
    /// Codec can emit timestamps.
    pub timestamps: bool,
}

impl CodecCapabilitySet {
    /// Text-only capability set.
    pub const TEXT_ONLY: Self = Self {
        text: true,
        layout: false,
        confidence: false,
        timestamps: false,
    };

    /// Builds a codec capability bitset.
    pub const fn new(text: bool, layout: bool, confidence: bool, timestamps: bool) -> Self {
        Self {
            text,
            layout,
            confidence,
            timestamps,
        }
    }

    /// Returns true when this set contains every requested capability bit.
    pub fn contains(self, requested: Self) -> bool {
        (!requested.text || self.text)
            && (!requested.layout || self.layout)
            && (!requested.confidence || self.confidence)
            && (!requested.timestamps || self.timestamps)
    }
}

/// Per-codec declaration for text extraction density checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExtractionDensityPolicy {
    /// Require at least this many extracted text bytes per source KiB.
    Required(f32),
    /// Explicit exemption with an audit-visible reason.
    Exempt { reason: String },
}

impl Default for ExtractionDensityPolicy {
    fn default() -> Self {
        Self::Exempt {
            reason: "calibration_pending".to_string(),
        }
    }
}

/// Metadata-only audit row emitted by a document codec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CodecAuditRow {
    /// Stable codec id, such as `gaze.codec.tesseract`.
    pub codec_id: String,
    /// Adapter crate version, distinct from engine provenance.
    pub codec_version: String,
    /// Accepted MIME type for the decode.
    pub accepted_mime: String,
    /// Capabilities advertised by the codec.
    pub advertised: CodecCapabilitySet,
    /// Capabilities delivered for this decode.
    pub delivered: CodecCapabilitySet,
    /// Text provenance reported by the codec.
    pub text_origin: TextOrigin,
    /// Codec-output schema version, decoupled from bundle schema version.
    pub codec_output_schema_version: u16,
    /// Hash of canonical codec options, never the options themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_hash_hex: Option<String>,
    /// Engine provenance string, without paths or raw source text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_provenance: Option<String>,
    /// Extraction density policy declared by the codec for this MIME.
    pub extraction_density_policy: ExtractionDensityPolicy,
}

impl CodecAuditRow {
    /// Builds a metadata-only codec audit row.
    pub fn new(
        codec_id: impl Into<String>,
        codec_version: impl Into<String>,
        accepted_mime: impl Into<String>,
        text_origin: TextOrigin,
    ) -> Self {
        Self {
            codec_id: codec_id.into(),
            codec_version: codec_version.into(),
            accepted_mime: accepted_mime.into(),
            advertised: CodecCapabilitySet::default(),
            delivered: CodecCapabilitySet::default(),
            text_origin,
            codec_output_schema_version: 1,
            options_hash_hex: None,
            engine_provenance: None,
            extraction_density_policy: ExtractionDensityPolicy::default(),
        }
    }
}

/// A suspected missed PII span reported by a [`SafetyNet`].
///
/// The safety net is not authoritative; a `LeakReport` is a signal, not a confirmed
/// leak. False positives are expected. Review reports and adjust policy or recognizer
/// thresholds.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct LeakReport {
    /// Suspected leaks, containing metadata only.
    pub suspects: Vec<LeakSuspect>,
    /// Bytes-free telemetry events.
    pub telemetry: Vec<LeakReportTelemetry>,
    /// Aggregated counts for callers that do not need full suspect metadata.
    pub stats: LeakReportStats,
    /// Optional replay hash.
    ///
    /// Replay determinism is guaranteed only when command path, checkpoint,
    /// operating point, min score, and decode parameters are fixed externally.
    pub replay_hash: Option<String>,
}

impl LeakReport {
    /// Builds a report from suspects and telemetry.
    pub fn from_parts(
        suspects: Vec<LeakSuspect>,
        telemetry: Vec<LeakReportTelemetry>,
    ) -> LeakReport {
        let mut stats = LeakReportStats {
            suspect_count: suspects.len(),
            locale_skipped_count: telemetry
                .iter()
                .filter(|event| matches!(event, LeakReportTelemetry::LocaleSkipped { .. }))
                .count(),
            ..LeakReportStats::default()
        };
        for suspect in &suspects {
            match suspect.kind {
                LeakKind::Uncovered => stats.uncovered_count += 1,
                LeakKind::PartialBleed { .. } => stats.partial_bleed_count += 1,
                LeakKind::ClassMismatch { .. } => stats.class_mismatch_count += 1,
            }
        }
        LeakReport {
            suspects,
            telemetry,
            stats,
            replay_hash: None,
        }
    }

    /// Merges another report into this report.
    pub fn extend(&mut self, other: LeakReport) {
        self.suspects.extend(other.suspects);
        self.telemetry.extend(other.telemetry);
        *self = LeakReport::from_parts(
            std::mem::take(&mut self.suspects),
            std::mem::take(&mut self.telemetry),
        );
    }
}

/// Closed set of upstream OpenAI Privacy Filter labels accepted by Gaze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum OpenAiPrivateLabel {
    /// `private_person`.
    PrivatePerson,
    /// `private_address`.
    PrivateAddress,
    /// `private_email`.
    PrivateEmail,
    /// `private_phone`.
    PrivatePhone,
    /// `private_url`.
    PrivateUrl,
    /// `private_date`.
    PrivateDate,
    /// `account_number`.
    AccountNumber,
    /// `secret`.
    Secret,
}

impl OpenAiPrivateLabel {
    /// Returns the raw upstream label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrivatePerson => "private_person",
            Self::PrivateAddress => "private_address",
            Self::PrivateEmail => "private_email",
            Self::PrivatePhone => "private_phone",
            Self::PrivateUrl => "private_url",
            Self::PrivateDate => "private_date",
            Self::AccountNumber => "account_number",
            Self::Secret => "secret",
        }
    }
}

/// Closed safety-net PII vocabulary before mapping into `PiiClass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SafetyNetPiiClass {
    /// Email address.
    Email,
    /// Person name.
    Name,
    /// Location or address.
    Location,
    /// Phone number.
    Phone,
    /// URL.
    Url,
    /// Date.
    Date,
    /// Account number.
    AccountNumber,
    /// Secret.
    Secret,
}

impl SafetyNetPiiClass {
    /// Maps the safety-net class into the shared pipeline class vocabulary.
    pub fn to_pii_class(self) -> PiiClass {
        match self {
            Self::Email => PiiClass::Email,
            Self::Name => PiiClass::Name,
            Self::Location => PiiClass::Location,
            Self::Phone => PiiClass::custom("phone"),
            Self::Url => PiiClass::custom("url"),
            Self::Date => PiiClass::custom("date"),
            Self::AccountNumber => PiiClass::custom("account_number"),
            Self::Secret => PiiClass::custom("secret"),
        }
    }
}

/// Exhaustive, closed error set for safety-net execution.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SafetyNetError {
    /// Safety net was explicitly requested but is unavailable.
    #[error("safety net unavailable: {reason}")]
    Unavailable {
        /// Sanitized reason.
        reason: String,
    },
    /// Required model weights or checkpoint are missing.
    #[error("safety net weights missing: {path}")]
    WeightsMissing {
        /// Sanitized path or identifier.
        path: String,
    },
    /// Backend model could not be loaded or reached.
    #[error("safety net model unavailable: {reason}")]
    ModelUnavailable {
        /// Sanitized reason.
        reason: String,
    },
    /// Input exceeded configured backend limit.
    #[error("safety net input too large: limit={limit}, actual={actual}")]
    InputTooLarge {
        /// Configured byte limit.
        limit: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// Backend runtime failed.
    #[error("safety net runtime failed: {message}")]
    Runtime {
        /// Sanitized diagnostic message.
        message: String,
    },
    /// Backend returned invalid output.
    #[error("safety net invalid output: {message}")]
    InvalidOutput {
        /// Sanitized diagnostic message.
        message: String,
    },
}

/// Disposition applied to a detected PII span.
///
/// | Variant | Restorable | Output shape |
/// |---------|------------|--------------|
/// | `Tokenize` | Yes | Opaque token: `<hex:Class_N>` |
/// | `FormatPreserve` | Yes | Realistic-looking pseudonym (e.g., `email1.hex@gaze-fake.invalid`) |
/// | `Redact` | No | Literal `[REDACTED]` -- original value is gone |
/// | `Generalize` | No | Class label (e.g., `[Email]`) -- original value is gone |
/// | `Preserve` | - | Passes through unchanged |
///
/// `Action` is `#[non_exhaustive]`. Use a wildcard arm in exhaustive matches.
/// When restore is required, use `Tokenize` or `FormatPreserve` -- `Redact` and
/// `Generalize` are irreversible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Replace PII with a reversible token.
    Tokenize,
    /// Replace PII with a non-restorable redaction marker.
    Redact,
    /// Replace PII with a reversible format-preserving token.
    FormatPreserve,
    /// Replace PII with a broader category.
    Generalize,
    /// Preserve the original value.
    Preserve,
}

/// Conflict resolution tier that selected or rejected a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConflictTier {
    /// No conflict resolution was needed.
    None,
    /// Class priority decided the conflict.
    ClassPriority,
    /// Rule priority decided the conflict.
    RulePriority,
    /// Candidate score decided the conflict.
    Score,
    /// Span length decided the conflict.
    SpanLength,
    /// Validator result decided the conflict.
    Validator,
    /// Recognizer identifier decided the conflict.
    RecognizerId,
    /// Candidate was merged with another candidate.
    Merged,
}

/// Source document kind for metadata-only audit logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentKind {
    /// Structured key/value document.
    Structured,
    /// Plain text document.
    Text,
}

/// One row of redaction metadata emitted to a [`RedactionLogger`].
///
/// Fields identify the PII class, action taken, session ID, source document kind,
/// conflict-resolution metadata, and timestamp. Does **not** contain the original PII
/// value, the token string, or any identifiable content beyond what a compliance audit
/// requires.
///
/// `RedactionEntry` is `#[non_exhaustive]`; adopters must construct via the public
/// constructor or destructure with a wildcard pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RedactionEntry {
    /// Detector or recognizer source identifier.
    pub source: String,
    /// PII class affected by the decision.
    pub class: PiiClass,
    /// Policy action applied to the span.
    pub action: Action,
    /// Optional structured field name.
    pub field_name: Option<String>,
    /// Source document kind.
    pub document_kind: DocumentKind,
    /// Whether this entry records a loser in conflict resolution.
    pub conflict_loser: bool,
    /// Conflict tier that decided the outcome.
    pub decided_by: ConflictTier,
    /// Creation timestamp in epoch milliseconds.
    pub created_at: i64,
    /// Optional session identifier.
    pub session_id: Option<String>,
    /// Optional validator failure reason for a vetoed candidate.
    pub validator_fail_reason: Option<ValidatorFailReason>,
    /// Optional ambiguity metadata for a family-level fallback.
    pub ambiguity_record: Option<AmbiguityRecord>,
}

impl RedactionEntry {
    /// Builds a metadata-only redaction log entry.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: impl Into<String>,
        class: PiiClass,
        action: Action,
        field_name: Option<String>,
        document_kind: DocumentKind,
        conflict_loser: bool,
        decided_by: ConflictTier,
        created_at: i64,
        session_id: Option<String>,
    ) -> Self {
        Self {
            source: source.into(),
            class,
            action,
            field_name,
            document_kind,
            conflict_loser,
            decided_by,
            created_at,
            session_id,
            validator_fail_reason: None,
            ambiguity_record: None,
        }
    }

    /// Attaches a validator failure reason to this metadata row.
    pub fn with_validator_fail_reason(mut self, reason: ValidatorFailReason) -> Self {
        self.validator_fail_reason = Some(reason);
        self
    }

    /// Attaches an ambiguity record to this metadata row.
    pub fn with_ambiguity_record(mut self, record: AmbiguityRecord) -> Self {
        self.ambiguity_record = Some(record);
        self
    }
}

/// Closed error set for redaction log sinks.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum RedactionLogError {
    /// SQLite-backed redaction log sink failed.
    #[error("sqlite redaction log error: {0}")]
    Sqlite(String),
    /// Non-SQLite redaction log sink failed.
    #[error("backend redaction log error: {0}")]
    Backend(String),
}

/// Trait for audit sinks that receive redaction metadata.
///
/// Implement this for custom audit backends (remote telemetry, structured JSON logs).
/// For SQLite-backed persistence, use `gaze_audit::SqliteLogger`.
///
/// # Contract
///
/// The logger receives **metadata only**: class, action, session ID, timestamp, and
/// other bytes-free audit labels. It never receives the original PII value or the token
/// value. A custom impl that augments entries with raw document text violates the audit
/// isolation contract and will be flagged by the `gaze_module_isolation` Dylint lint
/// when it lives in the wrong crate.
///
/// # Example
///
/// ```rust
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use gaze_types::{RedactionEntry, RedactionLogError, RedactionLogger};
///
/// #[derive(Default)]
/// struct CountLogger(AtomicUsize);
///
/// impl RedactionLogger for CountLogger {
///     fn log(&self, _entry: &RedactionEntry) -> Result<(), RedactionLogError> {
///         self.0.fetch_add(1, Ordering::Relaxed);
///         Ok(())
///     }
/// }
/// ```
pub trait RedactionLogger: Send + Sync {
    /// Records a metadata-only redaction entry.
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError>;
}

/// Locale tag recognized by policy and recognizers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LocaleTag {
    /// Locale-independent recognizer or policy.
    Global,
    /// German as used in Germany.
    DeDe,
    /// German as used in Austria.
    DeAt,
    /// German as used in Switzerland.
    DeCh,
    /// English as used in the United States.
    EnUs,
    /// English as used in Great Britain.
    EnGb,
    /// English as used in Ireland.
    EnIe,
    /// English as used in Australia.
    EnAu,
    /// English as used in Canada.
    EnCa,
    /// Any other canonical BCP-47-like tag.
    Other(String),
}

/// Locale parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocaleError {
    /// Locale tag is unsupported or invalid.
    Unsupported,
}

impl fmt::Display for LocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocaleError::Unsupported => f.write_str("unsupported locale"),
        }
    }
}

impl std::error::Error for LocaleError {}

/// Ordered locale fallback chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleChain(Vec<LocaleTag>);

impl LocaleTag {
    /// Global locale constant.
    pub const GLOBAL: LocaleTag = LocaleTag::Global;

    /// Parses a locale tag from policy or CLI input.
    pub fn parse(s: &str) -> Result<LocaleTag, LocaleError> {
        let raw = s.trim().replace('_', "-");
        let normalized = raw.to_ascii_lowercase();
        match normalized.as_str() {
            "global" | "*" => Ok(LocaleTag::Global),
            "de-de" => Ok(LocaleTag::DeDe),
            "de-at" => Ok(LocaleTag::DeAt),
            "de-ch" => Ok(LocaleTag::DeCh),
            "en-us" => Ok(LocaleTag::EnUs),
            "en-gb" => Ok(LocaleTag::EnGb),
            "en-ie" => Ok(LocaleTag::EnIe),
            "en-au" => Ok(LocaleTag::EnAu),
            "en-ca" => Ok(LocaleTag::EnCa),
            "" => Err(LocaleError::Unsupported),
            _ if is_bcp47_parseable(&raw) => Ok(LocaleTag::Other(canonical_other(&raw))),
            _ => Err(LocaleError::Unsupported),
        }
    }

    /// Returns the canonical string form of the locale tag.
    pub fn as_str(&self) -> &str {
        match self {
            LocaleTag::Global => "global",
            LocaleTag::DeDe => "de-DE",
            LocaleTag::DeAt => "de-AT",
            LocaleTag::DeCh => "de-CH",
            LocaleTag::EnUs => "en-US",
            LocaleTag::EnGb => "en-GB",
            LocaleTag::EnIe => "en-IE",
            LocaleTag::EnAu => "en-AU",
            LocaleTag::EnCa => "en-CA",
            LocaleTag::Other(tag) => tag.as_str(),
        }
    }
}

impl LocaleChain {
    /// Builds a locale chain and appends global fallback when absent.
    pub fn from_tags(mut tags: Vec<LocaleTag>) -> LocaleChain {
        ensure_global(&mut tags);
        LocaleChain(tags)
    }

    /// Parses a comma-separated CLI locale chain.
    pub fn from_cli(raw: &str) -> Result<LocaleChain, LocaleError> {
        let tags = raw
            .split(',')
            .map(LocaleTag::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LocaleChain::from_tags(tags))
    }

    /// Merges policy and CLI locale preferences.
    pub fn merge_policy_and_cli(
        policy: Option<&[LocaleTag]>,
        cli: Option<&[LocaleTag]>,
    ) -> LocaleChain {
        Self::merge_cli_policy_rulepack_default(cli, policy, None)
    }

    /// Merges CLI, policy, rulepack, and default locale preferences.
    pub fn merge_cli_policy_rulepack_default(
        cli: Option<&[LocaleTag]>,
        policy: Option<&[LocaleTag]>,
        rulepack_defaults: Option<&[LocaleTag]>,
    ) -> LocaleChain {
        let tags = cli
            .filter(|tags| !tags.is_empty())
            .or_else(|| policy.filter(|tags| !tags.is_empty()))
            .or_else(|| rulepack_defaults.filter(|tags| !tags.is_empty()))
            .map(|tags| tags.to_vec())
            .unwrap_or_else(|| vec![LocaleTag::Global]);
        LocaleChain::from_tags(tags)
    }

    /// Returns true when a recognizer can run under this locale chain.
    pub fn intersects(&self, recognizer_locales: &[LocaleTag]) -> bool {
        if recognizer_locales.is_empty() {
            return true;
        }
        recognizer_locales.iter().any(|recognizer_locale| {
            *recognizer_locale == LocaleTag::Global
                || self.0.iter().any(|active| active == recognizer_locale)
        })
    }

    /// Returns the locale tags in chain order.
    pub fn as_slice(&self) -> &[LocaleTag] {
        &self.0
    }

    /// Returns the locale chain as canonical strings.
    pub fn to_strings(&self) -> Vec<String> {
        self.0.iter().map(ToString::to_string).collect()
    }
}

impl From<&[LocaleTag]> for LocaleChain {
    fn from(tags: &[LocaleTag]) -> Self {
        let mut owned = tags.to_vec();
        ensure_global(&mut owned);
        LocaleChain(owned)
    }
}

impl fmt::Display for LocaleTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The input document submitted for pseudonymization.
///
/// `RawDocument::Text(String)` for plain or semi-structured text (most LLM workflows).
/// `RawDocument::Structured(BTreeMap<String, Value>)` for JSON-shaped data where
/// column-aware rules apply -- `ColumnRule`s only take effect on structured input.
///
/// `Detection::span` and recognizer candidate spans use **byte** ranges, not char indices.
///
/// `RawDocument` is `#[non_exhaustive]`. Match with a wildcard arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RawDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// The pseudonymized output from `Pipeline::redact`.
///
/// Mirrors the shape of `RawDocument`: `CleanDocument::Text(String)` or
/// `CleanDocument::Structured(BTreeMap<String, Value>)`. Destructure with a `let`-else
/// or `match`; **there is no `.text()` accessor**.
///
/// ```rust
/// use gaze_types::CleanDocument;
///
/// fn unwrap_text(doc: CleanDocument) -> Option<String> {
///     if let CleanDocument::Text(t) = doc { Some(t) } else { None }
/// }
/// ```
///
/// Contains only tokens or redacted placeholders -- no original PII values.
/// Send this (or its inner string) to the LLM; never send the original `RawDocument`.
///
/// `CleanDocument` is `#[non_exhaustive]`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum CleanDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// Minimal structured value representation that avoids a serde_json dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Value {
    /// Null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// String value.
    String(String),
    /// Signed 64-bit integer value.
    I64(i64),
    /// Array value.
    Array(Vec<Value>),
    /// Object value.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Returns the inner string for string values.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            Self::Null | Self::Bool(_) | Self::I64(_) | Self::Array(_) | Self::Object(_) => None,
        }
    }

    /// Returns a scalar string representation used for structured safety-net checks.
    pub fn scalar_to_safety_net_string(&self) -> Option<String> {
        match self {
            Self::String(value) if !value.is_empty() => Some(value.clone()),
            Self::String(_) | Self::Null | Self::Array(_) | Self::Object(_) => None,
            Self::Bool(value) => Some(value.to_string()),
            Self::I64(value) => Some(value.to_string()),
        }
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

/// Value-only dictionary bundle shared with recognizers.
#[derive(Debug, Clone, Default)]
pub struct DictionaryBundle {
    entries: HashMap<String, DictionaryEntry>,
}

/// Value-only dictionary entry; compiled automatons live outside `gaze-types`.
#[derive(Debug, Clone)]
pub struct DictionaryEntry {
    terms: Vec<String>,
    case_sensitive: bool,
    source: DictionarySource,
}

/// Source of a dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionarySource {
    /// Dictionary supplied by request context.
    Cli,
    /// Dictionary supplied by a rulepack.
    Rulepack,
}

/// Dictionary metadata used for diagnostics and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DictionaryStats {
    /// Dictionary name.
    pub name: String,
    /// Number of configured terms.
    pub term_count: usize,
    /// Dictionary source.
    pub source: DictionarySource,
}

impl DictionaryStats {
    /// Builds dictionary diagnostics metadata.
    pub fn new(name: impl Into<String>, term_count: usize, source: DictionarySource) -> Self {
        Self {
            name: name.into(),
            term_count,
            source,
        }
    }
}

/// Dictionary declared by a rulepack.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RulepackDict {
    /// Dictionary name.
    pub name: String,
    /// Dictionary terms.
    pub terms: Vec<String>,
    /// Whether matching is case-sensitive.
    pub case_sensitive: bool,
}

impl RulepackDict {
    /// Builds a rulepack dictionary declaration.
    pub fn new(name: impl Into<String>, terms: Vec<String>, case_sensitive: bool) -> Self {
        Self {
            name: name.into(),
            terms,
            case_sensitive,
        }
    }
}

/// Error raised when constructing invalid dictionary entries.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionaryLoadError {
    /// Dictionary has no terms.
    Empty { name: String },
    /// ASCII-only case-insensitive matching cannot safely cover this entry.
    UnicodeInsensitiveUnsupported { name: String },
}

impl fmt::Display for DictionaryLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { name } => write!(f, "dictionary '{name}' has no terms"),
            Self::UnicodeInsensitiveUnsupported { name } => write!(
                f,
                "dictionary '{name}' uses unicode terms with case-insensitive matching, unsupported in v0.4.0; use case_sensitive = true"
            ),
        }
    }
}

impl std::error::Error for DictionaryLoadError {}

impl DictionaryBundle {
    /// Builds a bundle from rulepack dictionaries.
    pub fn from_rulepack_terms(terms: &[RulepackDict]) -> Self {
        let mut entries = HashMap::with_capacity(terms.len());
        for dictionary in terms {
            let entry = DictionaryEntry::new(
                &dictionary.name,
                dictionary.terms.clone(),
                dictionary.case_sensitive,
                DictionarySource::Rulepack,
            )
            .expect("Policy validates dictionary terms before bundle construction");
            entries.insert(dictionary.name.clone(), entry);
        }
        Self { entries }
    }

    /// Builds a bundle from pre-built dictionary entries.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, DictionaryEntry)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Merges two bundles, preferring entries from the second bundle on name conflicts.
    pub fn merge(a: Self, b: Self) -> Self {
        let mut entries = a.entries;
        entries.extend(b.entries);
        Self { entries }
    }

    /// Returns a dictionary by name.
    pub fn get(&self, name: &str) -> Option<&DictionaryEntry> {
        self.entries.get(name)
    }

    /// Returns sorted dictionary stats.
    pub fn stats(&self) -> Vec<DictionaryStats> {
        let mut stats = self
            .entries
            .iter()
            .map(|(name, entry)| DictionaryStats {
                name: name.clone(),
                term_count: entry.terms.len(),
                source: entry.source,
            })
            .collect::<Vec<_>>();
        stats.sort_by(|a, b| a.name.cmp(&b.name));
        stats
    }
}

impl DictionaryEntry {
    /// Creates a validated value-only dictionary entry.
    pub fn new(
        name: &str,
        terms: Vec<String>,
        case_sensitive: bool,
        source: DictionarySource,
    ) -> Result<Self, DictionaryLoadError> {
        if terms.is_empty() {
            return Err(DictionaryLoadError::Empty {
                name: name.to_string(),
            });
        }
        if !case_sensitive && terms.iter().any(|term| !term.is_ascii()) {
            return Err(DictionaryLoadError::UnicodeInsensitiveUnsupported {
                name: name.to_string(),
            });
        }
        Ok(Self {
            terms,
            case_sensitive,
            source,
        })
    }

    /// Returns whether matching is case-sensitive.
    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// Returns configured dictionary terms.
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

#[cfg(test)]
mod document_extension_tests {
    use super::*;

    fn audit_row() -> CodecAuditRow {
        let mut row = CodecAuditRow::new(
            "gaze.codec.tesseract",
            "gaze-codec-tesseract@0.7.1",
            "image/png",
            TextOrigin::Ocr,
        );
        row.advertised = CodecCapabilitySet::new(true, true, true, false);
        row.delivered = CodecCapabilitySet::new(true, true, false, false);
        row.extraction_density_policy = ExtractionDensityPolicy::Required(1.0);
        row
    }

    fn extension_builder() -> DocumentExtensionBuilder {
        DocumentExtension::builder(1)
            .clean_md_sha256([1; 32])
            .layout_json_sha256([2; 32])
            .report_json_sha256([3; 32])
            .page_count(2)
            .audit_session_id("018f0000-0000-7000-8000-000000000000")
    }

    #[test]
    fn document_extension_round_trips_with_bundle_root_schema_version() {
        let mut row = audit_row();
        row.options_hash_hex = Some("00".repeat(32));
        row.engine_provenance = Some("tesseract@5.3.4".to_string());
        let extension = extension_builder()
            .preview_png_sha256([4; 32])
            .clean_spans(vec![EmittedTokenSpan::new(0..8, 0..12, PiiClass::Email)])
            .codec_audit(vec![row])
            .build()
            .expect("document extension");

        let json = serde_json::to_value(&extension).expect("serialize document extension");

        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["clean_md_sha256"].as_array().expect("hash").len(), 32);
        assert_eq!(
            json["layout_json_sha256"].as_array().expect("hash").len(),
            32
        );
        assert_eq!(
            json["report_json_sha256"].as_array().expect("hash").len(),
            32
        );
        assert_eq!(
            json["preview_png_sha256"].as_array().expect("hash").len(),
            32
        );
        assert_eq!(json["page_count"], 2);
        assert_eq!(
            json["audit_session_id"],
            "018f0000-0000-7000-8000-000000000000"
        );
        assert_eq!(json["clean_spans"].as_array().expect("spans").len(), 1);
        assert!(json.get("clean_schema_version").is_none());
        assert!(json.get("layout_schema_version").is_none());
        assert!(json.get("report_schema_version").is_none());
        assert!(json.get("manifest_schema_version").is_none());

        let decoded: DocumentExtension =
            serde_json::from_value(json).expect("deserialize document extension");
        assert_eq!(decoded, extension);
    }

    #[test]
    fn document_extension_carries_full_integrity_set() {
        let extension = DocumentExtension::builder(1)
            .clean_md_sha256([10; 32])
            .layout_json_sha256([11; 32])
            .report_json_sha256([12; 32])
            .preview_png_sha256([13; 32])
            .page_count(7)
            .audit_session_id("018f0000-0000-7000-8000-000000000001")
            .clean_spans(vec![EmittedTokenSpan::new(5..14, 20..34, PiiClass::Name)])
            .codec_audit(vec![audit_row()])
            .build()
            .expect("document extension");

        let json = serde_json::to_string(&extension).expect("serialize document extension");
        let decoded: DocumentExtension =
            serde_json::from_str(&json).expect("deserialize document extension");

        assert_eq!(decoded, extension);
        assert_eq!(decoded.clean_md_sha256, [10; 32]);
        assert_eq!(decoded.layout_json_sha256, [11; 32]);
        assert_eq!(decoded.report_json_sha256, [12; 32]);
        assert_eq!(decoded.preview_png_sha256, Some([13; 32]));
        assert_eq!(decoded.page_count, 7);
        assert_eq!(
            decoded.audit_session_id,
            "018f0000-0000-7000-8000-000000000001"
        );
        assert_eq!(decoded.clean_spans.len(), 1);
        assert_eq!(decoded.codec_audit.len(), 1);
    }

    #[test]
    fn document_extension_builder_requires_integrity_fields() {
        assert_eq!(
            DocumentExtension::builder(1).build(),
            Err(DocumentExtensionError::MissingField("clean_md_sha256"))
        );
        assert_eq!(
            DocumentExtension::builder(1)
                .clean_md_sha256([1; 32])
                .layout_json_sha256([2; 32])
                .report_json_sha256([3; 32])
                .page_count(1)
                .build(),
            Err(DocumentExtensionError::MissingField("audit_session_id"))
        );
    }

    #[test]
    fn codec_audit_row_round_trips_without_raw_pii_fields() {
        let row = audit_row();
        let json = serde_json::to_string(&row).expect("serialize codec audit row");

        assert!(json.contains("\"codec_id\""));
        assert!(!json.contains("alice@example.invalid"));
        assert!(!json.contains("\"raw\""));
        assert_eq!(
            serde_json::from_str::<CodecAuditRow>(&json).expect("deserialize codec audit row"),
            row
        );
    }

    #[test]
    fn text_origin_round_trips() {
        for origin in [
            TextOrigin::Ocr,
            TextOrigin::EmbeddedText,
            TextOrigin::Transcript,
            TextOrigin::Hybrid,
        ] {
            let json = serde_json::to_string(&origin).expect("serialize text origin");
            let decoded: TextOrigin = serde_json::from_str(&json).expect("deserialize text origin");
            assert_eq!(decoded, origin);
        }
    }

    #[test]
    fn codec_capability_set_round_trips_and_contains_requested_bits() {
        let delivered = CodecCapabilitySet::new(true, true, false, false);

        let json = serde_json::to_string(&delivered).expect("serialize capabilities");
        let decoded: CodecCapabilitySet =
            serde_json::from_str(&json).expect("deserialize capabilities");

        assert_eq!(decoded, delivered);
        assert!(decoded.contains(CodecCapabilitySet::TEXT_ONLY));
        assert!(!decoded.contains(CodecCapabilitySet::new(true, true, true, false)));
    }

    #[test]
    fn extraction_density_policy_round_trips_closed_variants() {
        for policy in [
            ExtractionDensityPolicy::Required(1.25),
            ExtractionDensityPolicy::Exempt {
                reason: "text_only".to_string(),
            },
        ] {
            let json = serde_json::to_string(&policy).expect("serialize density policy");
            let decoded: ExtractionDensityPolicy =
                serde_json::from_str(&json).expect("deserialize density policy");
            assert_eq!(decoded, policy);
        }
    }

    #[test]
    fn manifest_stats_round_trip_for_document_report_mirrors() {
        let manifest =
            Manifest::from_spans(vec![EmittedTokenSpan::new(0..15, 0..19, PiiClass::Email)]);
        let stats = LeakReportStats {
            suspect_count: 1,
            uncovered_count: 0,
            partial_bleed_count: 0,
            class_mismatch_count: 0,
            locale_skipped_count: 0,
        };

        let manifest_json = serde_json::to_string(&manifest).expect("serialize manifest");
        let stats_json = serde_json::to_string(&stats).expect("serialize stats");

        assert_eq!(
            serde_json::from_str::<Manifest>(&manifest_json).expect("deserialize manifest"),
            manifest
        );
        assert_eq!(
            serde_json::from_str::<LeakReportStats>(&stats_json).expect("deserialize stats"),
            stats
        );
    }
}

#[cfg(test)]
mod dictionary_tests {
    use super::*;

    #[test]
    fn dictionary_entry_rejects_empty_terms() {
        let err = DictionaryEntry::new("empty", Vec::new(), true, DictionarySource::Cli)
            .expect_err("empty dictionaries must fail closed");

        assert!(matches!(err, DictionaryLoadError::Empty { name } if name == "empty"));
    }

    #[test]
    fn dictionary_entry_rejects_non_ascii_case_insensitive_terms() {
        let err = DictionaryEntry::new(
            "songs",
            vec!["Beyonce".to_string(), "Caf\u{00e9}".to_string()],
            false,
            DictionarySource::Cli,
        )
        .expect_err("unicode case-insensitive dictionaries must fail closed");

        assert!(matches!(
            err,
            DictionaryLoadError::UnicodeInsensitiveUnsupported { name } if name == "songs"
        ));
    }
}

#[cfg(test)]
mod redaction_logger_tests {
    use super::*;

    struct CapturingLogger;

    impl RedactionLogger for CapturingLogger {
        fn log(&self, _entry: &RedactionEntry) -> Result<(), RedactionLogError> {
            Ok(())
        }
    }

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    #[test]
    fn redaction_log_error_display_is_stable() {
        assert_eq!(
            RedactionLogError::Sqlite("write failed".to_string()).to_string(),
            "sqlite redaction log error: write failed"
        );
        assert_eq!(
            RedactionLogError::Backend("sink failed".to_string()).to_string(),
            "backend redaction log error: sink failed"
        );
    }

    #[test]
    fn redaction_logger_trait_object_is_send_sync() {
        assert_send_sync::<dyn RedactionLogger>();
    }

    #[test]
    fn local_logger_can_implement_redaction_logger() {
        let logger = CapturingLogger;
        let entry = RedactionEntry {
            source: "unit-test".to_string(),
            class: PiiClass::Email,
            action: Action::Tokenize,
            field_name: None,
            document_kind: DocumentKind::Text,
            conflict_loser: false,
            decided_by: ConflictTier::None,
            created_at: 0,
            session_id: None,
            validator_fail_reason: None,
            ambiguity_record: None,
        };

        let trait_object: &dyn RedactionLogger = &logger;
        trait_object.log(&entry).expect("log entry");
    }
}

#[cfg(test)]
mod safety_net_manifest_tests {
    use super::*;

    fn span(start: usize, end: usize, class: PiiClass) -> EmittedTokenSpan {
        EmittedTokenSpan {
            clean_span: start..end,
            raw_span: start..end,
            class,
        }
    }

    fn diff(manifest: Manifest, suspect: Range<usize>, class: PiiClass) -> Option<LeakKind> {
        manifest.diff_against(&suspect, &class)
    }

    #[test]
    fn exact_same_class_coverage_is_not_a_leak() {
        let manifest = Manifest::from_spans(vec![span(0, 8, PiiClass::Email)]);

        assert_eq!(diff(manifest, 0..8, PiiClass::Email), None);
    }

    #[test]
    fn uncovered_outside_all_tokens_is_uncovered() {
        let manifest = Manifest::from_spans(vec![span(20, 30, PiiClass::Email)]);

        assert_eq!(
            diff(manifest, 0..10, PiiClass::Email),
            Some(LeakKind::Uncovered)
        );
    }

    #[test]
    fn single_internal_gap_returns_partial_bleed() {
        let manifest = Manifest::from_spans(vec![
            span(0, 5, PiiClass::Email),
            span(10, 15, PiiClass::Email),
        ]);

        assert_eq!(
            diff(manifest, 0..15, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 5..10 })
        );
    }

    #[test]
    fn multi_gap_returns_deterministic_first_uncovered_gap() {
        let manifest = Manifest::from_spans(vec![
            span(0, 3, PiiClass::Email),
            span(5, 7, PiiClass::Email),
            span(9, 12, PiiClass::Email),
        ]);

        // The first-gap-only rule is intentional for v0.6.1; full gap
        // enumeration is deferred until the report format can carry it.
        assert_eq!(
            diff(manifest, 0..12, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 3..5 })
        );
    }

    #[test]
    fn multi_class_overlap_reports_first_mismatch_deterministically() {
        let manifest = Manifest::from_spans(vec![
            span(0, 4, PiiClass::Name),
            span(4, 8, PiiClass::Location),
        ]);

        assert_eq!(
            diff(manifest, 0..8, PiiClass::Email),
            Some(LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Name,
                safety_net_class: PiiClass::Email,
            })
        );
    }

    #[test]
    fn adjacent_same_class_tokens_cover_continuously() {
        let manifest = Manifest::from_spans(vec![
            span(0, 5, PiiClass::Email),
            span(5, 10, PiiClass::Email),
        ]);

        assert_eq!(diff(manifest, 0..10, PiiClass::Email), None);
    }

    #[test]
    fn partial_bleed_at_start_end_and_middle() {
        let manifest = Manifest::from_spans(vec![span(3, 8, PiiClass::Email)]);

        assert_eq!(
            diff(manifest.clone(), 0..8, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 0..3 })
        );
        assert_eq!(
            diff(manifest.clone(), 3..10, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 8..10 })
        );

        let with_gap = Manifest::from_spans(vec![
            span(0, 3, PiiClass::Email),
            span(6, 10, PiiClass::Email),
        ]);
        assert_eq!(
            diff(with_gap, 0..10, PiiClass::Email),
            Some(LeakKind::PartialBleed { uncovered: 3..6 })
        );
    }

    #[test]
    fn byte_indices_are_not_character_indices() {
        let text = "ID: 😀 <Email_1>";
        let token_start = text.find("<Email_1>").expect("token start");
        assert_eq!(token_start, 9, "emoji is four bytes, not one char");
        let manifest = Manifest::from_spans(vec![span(token_start, text.len(), PiiClass::Email)]);

        assert_eq!(
            diff(manifest, token_start..text.len(), PiiClass::Email),
            None
        );
    }

    #[test]
    fn empty_suspect_range_is_not_a_leak() {
        let manifest = Manifest::default();

        assert_eq!(diff(manifest, 3..3, PiiClass::Email), None);
    }

    #[test]
    fn safety_net_error_display_is_variant_specific_and_bytes_free() {
        let cases = [
            SafetyNetError::Unavailable {
                reason: "not configured".to_string(),
            }
            .to_string(),
            SafetyNetError::WeightsMissing {
                path: "/models/opf".to_string(),
            }
            .to_string(),
            SafetyNetError::ModelUnavailable {
                reason: "load failed".to_string(),
            }
            .to_string(),
            SafetyNetError::InputTooLarge {
                limit: 1024,
                actual: 2048,
            }
            .to_string(),
            SafetyNetError::Runtime {
                message: "timeout".to_string(),
            }
            .to_string(),
            SafetyNetError::InvalidOutput {
                message: "bad json".to_string(),
            }
            .to_string(),
        ];

        for rendered in cases {
            assert!(!rendered.contains("alice@example.invalid"));
        }
    }
}

/// Shared recognizer contract for locale-aware PII candidates.
pub trait Recognizer: Send + Sync {
    /// Stable recognizer identifier.
    fn id(&self) -> &str;
    /// PII class supported by this recognizer.
    fn supported_class(&self) -> &PiiClass;
    /// Detects PII candidates in the supplied input and context.
    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Vec<Candidate>;
    /// Token family used for candidate token emission.
    fn token_family(&self) -> &str;
    /// Locales where this recognizer is active.
    fn locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
}

/// Candidate PII span emitted by a recognizer before final conflict resolution.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Candidate {
    /// Byte span in the original input.
    pub span: Range<usize>,
    /// PII class assigned to the span.
    pub class: PiiClass,
    /// Recognizer identifier.
    pub recognizer_id: String,
    /// Recognizer confidence score.
    pub score: f32,
    /// Rule or recognizer priority.
    pub priority: i32,
    /// Optional canonical representation for validation/merge logic.
    pub canonical_form: Option<String>,
    /// Token family used for output token shape.
    pub token_family: String,
    /// Candidate source label.
    pub source: String,
    /// Conflict tier that decided this candidate.
    pub decided_by: ConflictTier,
    /// Sources merged into this candidate.
    pub merged_sources: Vec<String>,
}

impl Candidate {
    /// Builds a recognizer candidate.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        span: Range<usize>,
        class: PiiClass,
        recognizer_id: impl Into<String>,
        score: f32,
        priority: i32,
        canonical_form: Option<String>,
        token_family: impl Into<String>,
        source: impl Into<String>,
        decided_by: ConflictTier,
        merged_sources: Vec<String>,
    ) -> Self {
        Self {
            span,
            class,
            recognizer_id: recognizer_id.into(),
            score,
            priority,
            canonical_form,
            token_family: token_family.into(),
            source: source.into(),
            decided_by,
            merged_sources,
        }
    }

    /// Returns this candidate with a translated span.
    pub fn with_span(mut self, span: Range<usize>) -> Self {
        self.span = span;
        self
    }
}

/// Context supplied to recognizers during detection.
#[non_exhaustive]
pub struct DetectContext<'a> {
    /// Active locale chain.
    pub locale_chain: &'a [LocaleTag],
    /// Active dictionary bundle.
    pub dictionaries: &'a DictionaryBundle,
    /// Reserved field-aware matching slot; intentionally unit in v0.5 Phase B.
    pub fields: &'a (),
    /// Whether a recognizer degraded due to unavailable optional capability.
    pub degraded: Cell<bool>,
}

impl<'a> DetectContext<'a> {
    /// Builds detection context for a recognizer pass.
    pub fn new(locale_chain: &'a [LocaleTag], dictionaries: &'a DictionaryBundle) -> Self {
        Self {
            locale_chain,
            dictionaries,
            fields: &(),
            degraded: Cell::new(false),
        }
    }
}

fn ensure_global(tags: &mut Vec<LocaleTag>) {
    if !tags.contains(&LocaleTag::Global) {
        tags.push(LocaleTag::Global);
    }
}

fn is_bcp47_parseable(raw: &str) -> bool {
    let mut parts = raw.split('-');
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=8).contains(&language.len()) || !language.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    parts.all(|part| {
        (2..=8).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_alphanumeric())
    })
}

fn canonical_other(raw: &str) -> String {
    let mut parts = raw.split('-');
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest = parts.map(|part| {
        if part.len() == 2 && part.chars().all(|ch| ch.is_ascii_alphabetic()) {
            part.to_ascii_uppercase()
        } else {
            part.to_ascii_lowercase()
        }
    });
    std::iter::once(language)
        .chain(rest)
        .collect::<Vec<_>>()
        .join("-")
}
