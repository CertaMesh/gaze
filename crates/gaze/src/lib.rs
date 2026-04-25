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

pub use context::{Context, Context as TypedContext, ContextDictionary, ContextError, RawContext};
pub use detector::{Detection, Detector, PiiClass, BUILTIN_CLASS_NAMES};
pub use dictionaries::{
    DictionaryBundle, DictionaryEntry, DictionaryLoadError, DictionarySource, DictionaryStats,
    RulepackDict,
};
pub use locale::{LocaleChain, LocaleError, LocaleTag};
pub use pipeline::{Error, Pipeline, PipelineBuilder, Result};
pub use policy::{
    DetectorKind, DetectorSpec, NerPolicy, Policy, PolicyError, RuleSpec, RulepackPolicy,
    SessionPolicy, SessionScope, DEFAULT_NER_THRESHOLD,
};
pub use redaction_log::{
    ConflictTier, DocumentKind, RedactionEntry, RedactionLogger, SqliteLogger,
};
pub use registry::{
    Candidate, Canonicalizer, DetectContext, Recognizer, RecognizerRegistry,
    RecognizerRegistryBuilder, ValidationResult, Validator,
};
pub use resolver::resolve_candidates;
pub use rule::{Action, ClassRule, ColumnRule, DefaultRule, Rule, RuleContext};
pub use rulepack::{
    ContextSpec, LocaleData, LocaleEmailHeaders, NormalizerSpec, RawMatch, RecognizerSpec,
    Rulepack, RulepackError, RulepackSource, ScoringSpec, SourceSpec, TokenSpec, ValidatorSpec,
};
pub use sandbox::{
    ExecPolicy, Sandbox, SandboxError, SandboxPlan, UntrustedExecRequest, ValidatedExecRequest,
};
pub use session::{Scope, SensitiveSnapshot, Session};
pub use types::{CleanDocument, RawDocument, Value};
