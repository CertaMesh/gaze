use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Shared detector contract for text-only PII detection.
pub trait Detector: Send + Sync {
    /// Detect PII spans in the supplied input string.
    fn detect(&self, input: &str) -> Vec<Detection>;
}

/// PII class vocabulary used by detectors, recognizers, policy, and audit metadata.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
            _ => None,
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
}

/// A detected span and its class/source metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// Byte span in the original input.
    pub span: Range<usize>,
    /// PII class assigned to the span.
    pub class: PiiClass,
    /// Detector source identifier.
    pub source: String,
}

/// Policy action vocabulary for handling detected PII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentKind {
    /// Structured key/value document.
    Structured,
    /// Plain text document.
    Text,
}

/// Metadata-only audit entry for a redaction decision.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// Locale tag recognized by policy and recognizers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// Input document before pseudonymization.
#[derive(Debug, Clone)]
pub enum RawDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// Cleaned document after pseudonymization.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CleanDocument {
    /// Structured document values.
    Structured(BTreeMap<String, Value>),
    /// Plain text document.
    Text(String),
}

/// Minimal structured value representation that avoids a serde_json dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Value {
    /// String value.
    String(String),
    /// Signed 64-bit integer value.
    I64(i64),
}

impl Value {
    /// Returns the inner string for string values.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            Self::I64(_) => None,
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
pub enum DictionarySource {
    /// Dictionary supplied by request context.
    Cli,
    /// Dictionary supplied by a rulepack.
    Rulepack,
}

/// Dictionary metadata used for diagnostics and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryStats {
    /// Dictionary name.
    pub name: String,
    /// Number of configured terms.
    pub term_count: usize,
    /// Dictionary source.
    pub source: DictionarySource,
}

/// Dictionary declared by a rulepack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulepackDict {
    /// Dictionary name.
    pub name: String,
    /// Dictionary terms.
    pub terms: Vec<String>,
    /// Whether matching is case-sensitive.
    pub case_sensitive: bool,
}

/// Error raised when constructing invalid dictionary entries.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Context supplied to recognizers during detection.
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
