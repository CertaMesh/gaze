#![cfg_attr(docsrs, feature(doc_cfg))]

mod anchor_resolver;
mod conflict;
mod context;
mod detector;
mod dictionaries;
pub mod locale;
mod normalize;
mod pipeline;
mod policy;
pub mod recognizer;
mod redaction_log;
pub mod registry;
pub mod resolver;
mod rule;
pub mod rulepack;
mod sandbox;
mod session;
pub mod token_shape;
mod types;
mod validator_veto;

pub use context::{
    Context, Context as TypedContext, ContextDictionary, ContextError, ContextFieldsRef,
};
pub use detector::{Detection, Detector, PiiClass, BUILTIN_CLASS_NAMES};
pub use dictionaries::{
    dictionary_bundle_from_context, DictionaryBundle, DictionaryBundleExt, DictionaryEntry,
    DictionaryLoadError, DictionarySource, DictionaryStats, RulepackDict,
};
pub use gaze_types::{
    CodecAuditRow, CodecCapabilitySet, CollisionMembership, DocumentExtension,
    DocumentExtensionBuilder, DocumentExtensionError, EmittedTokenSpan, ExtractionDensityPolicy,
    FallbackReason, LeakKind, LeakReport, LeakReportStats, LeakReportTelemetry, LeakSuspect,
    LocaleBasis, Manifest, OpenAiPrivateLabel, RedactionLogError, RedactionLogger, RestoreDecision,
    RestorePolicy, RestoreTelemetry, RestoredText, SafetyNet, SafetyNetContext, SafetyNetError,
    SafetyNetPiiClass, SafetyTier, TextOrigin, RESERVED_BUNDLED_FAMILIES,
    RESTORE_PHASE_FRESH_PII_SCAN, RESTORE_PHASE_MANIFEST_BYPASS_SCAN,
    RESTORE_PHASE_MANIFEST_LOOKUP, RESTORE_PHASE_UNKNOWN_TOKEN_SCAN,
};
pub use locale::{LocaleChain, LocaleError, LocaleTag};
pub use pipeline::{
    Error, GazeLocalProtectionTraceItem, Pipeline, PipelineBuilder, PipelineOptimizationConfig,
    PrefixCacheWriteMode, Result, SafetyNetDecision, SafetyNetFallback, SafetyNetMode,
    SafetyNetPolicy,
};
pub use policy::{
    validate_ner_locale, DetectorKind, DetectorSpec, NerPolicy, Policy, PolicyError, RuleSpec,
    RulepackPolicy, SessionPolicy, SessionScope, DEFAULT_NER_THRESHOLD,
    DEFAULT_POLICY_SCHEMA_VERSION, SUPPORTED_POLICY_SCHEMA_MAJOR_MINOR,
};
pub use redaction_log::{ConflictTier, DocumentKind, RedactionEntry};
pub use registry::{
    Candidate, Canonicalizer, DetectContext, FamilyPolicyTable, Recognizer, RecognizerRegistry,
    RecognizerRegistryBuilder, ValidationResult, Validator,
};
pub use resolver::{resolve_candidates, resolve_candidates_with_policy};
pub use rule::{Action, ClassRule, ColumnRule, DefaultRule, Rule, RuleContext};
pub use rulepack::{
    recognizer_composition_validator, AnchoredBoundary, ContextSpec, CuePosition, LocaleBucket,
    LocaleCueBundle, LocaleData, NameShape, NormalizerSpec, RawMatch, RecognizerSpec, Rulepack,
    RulepackError, RulepackSource, ScoringSpec, SourceSpec, TokenSpec, ValidatorSpec,
};
pub use sandbox::{
    ExecPolicy, Sandbox, SandboxError, SandboxPlan, UntrustedExecRequest, ValidatedExecRequest,
};
pub use session::{
    CommittedSessionSnapshot, RestoreError, RestoreEvent, RestoreEventKind,
    RestoredTextWithProvenance, Scope, SensitiveSnapshot, Session, SessionSnapshotEntry,
    SessionTransaction, SessionTransactionError,
};
pub use types::{CleanDocument, RawDocument, Value};
