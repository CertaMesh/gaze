use std::path::PathBuf;

use super::{
    OpenAiFilterDevice, OpenAiFilterOperatingPoint, SafetyNetBackend, SafetyNetFallback,
    SafetyNetKind, SafetyNetMode,
};
use crate::error::CliError;
use crate::pipeline::{run_clean, CleanOptions};

pub(crate) struct Args {
    pub(crate) policy: Option<PathBuf>,
    pub(crate) format: String,
    pub(crate) session_ttl: Option<u64>,
    pub(crate) session_scope: Option<String>,
    pub(crate) locale: Vec<String>,
    pub(crate) ner_threshold: Option<f32>,
    pub(crate) ner_model_dir: Option<PathBuf>,
    pub(crate) ner_locale: Option<String>,
    pub(crate) rulepack_bundled: Vec<String>,
    pub(crate) rulepack_paths: Vec<PathBuf>,
    pub(crate) max_bytes: u64,
    pub(crate) context_json: Option<PathBuf>,
    pub(crate) audit_db: Option<PathBuf>,
    pub(crate) safety_net: Option<SafetyNetKind>,
    pub(crate) safety_net_backend: SafetyNetBackend,
    pub(crate) openai_filter_command: Option<PathBuf>,
    pub(crate) openai_filter_checkpoint: Option<PathBuf>,
    pub(crate) openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
    pub(crate) openai_filter_device: OpenAiFilterDevice,
    pub(crate) kiji_distilbert_command: Option<PathBuf>,
    pub(crate) kiji_distilbert_model_dir: Option<PathBuf>,
    pub(crate) safety_net_timeout_ms: u64,
    pub(crate) safety_net_input_limit_bytes: usize,
    pub(crate) safety_net_mode: SafetyNetMode,
    pub(crate) safety_net_fallback: SafetyNetFallback,
}

pub(crate) fn run(args: Args) -> std::result::Result<(), CliError> {
    run_clean(CleanOptions {
        policy: args.policy.as_deref(),
        format: &args.format,
        session_ttl: args.session_ttl,
        session_scope: args.session_scope.as_deref(),
        locale: &args.locale,
        ner_threshold: args.ner_threshold,
        ner_model_dir: args.ner_model_dir,
        ner_locale: args.ner_locale.as_deref(),
        rulepack_bundled: &args.rulepack_bundled,
        rulepack_paths: args.rulepack_paths,
        max_bytes: args.max_bytes,
        context_json: args.context_json.as_deref(),
        audit_db: args.audit_db.as_deref(),
        safety_net: args.safety_net,
        safety_net_backend: args.safety_net_backend,
        openai_filter_command: args.openai_filter_command.as_deref(),
        openai_filter_checkpoint: args.openai_filter_checkpoint.as_deref(),
        openai_filter_operating_point: args.openai_filter_operating_point,
        openai_filter_device: args.openai_filter_device,
        kiji_distilbert_command: args.kiji_distilbert_command.as_deref(),
        kiji_distilbert_model_dir: args.kiji_distilbert_model_dir.as_deref(),
        safety_net_timeout_ms: args.safety_net_timeout_ms,
        safety_net_input_limit_bytes: args.safety_net_input_limit_bytes,
        safety_net_mode: args.safety_net_mode,
        safety_net_fallback: args.safety_net_fallback,
    })
}
