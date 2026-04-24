mod detector;
pub mod locale;
mod normalize;
mod pipeline;
mod policy;
pub mod recognizer;
mod redaction_log;
pub mod resolver;
pub mod registry;
pub mod rulepack;
mod rule;
mod sandbox;
mod session;
pub mod token_shape;
mod types;

pub use detector::{Detection, Detector, PiiClass, BUILTIN_CLASS_NAMES};
pub use locale::{LocaleChain, LocaleError, LocaleTag};
pub use pipeline::{Error, Pipeline, PipelineBuilder, Result};
pub use policy::{
    DetectorKind, DetectorSpec, NerPolicy, Policy, PolicyError, RuleSpec, SessionPolicy,
    SessionScope,
};
pub use redaction_log::{DocumentKind, RedactionEntry, RedactionLogger, SqliteLogger};
pub use resolver::resolve_candidates;
pub use rulepack::{
    ContextSpec, NormalizerSpec, RawMatch, RecognizerSpec, Rulepack, RulepackError,
    RulepackSource, ScoringSpec, SourceSpec, TokenSpec, ValidatorSpec,
};
pub use registry::{
    Candidate, Canonicalizer, DetectContext, DictionaryBundle, Recognizer, RecognizerRegistry,
    RecognizerRegistryBuilder, ValidationResult, Validator,
};
pub use rule::{Action, ClassRule, ColumnRule, Context, DefaultRule, Rule};
pub use sandbox::{
    ExecPolicy, Sandbox, SandboxError, SandboxPlan, UntrustedExecRequest, ValidatedExecRequest,
};
pub use session::{Scope, SensitiveSnapshot, Session};
pub use types::{CleanDocument, RawDocument, Value};
