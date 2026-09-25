use std::path::PathBuf;

use clap::Args as ClapArgs;

use super::shared_args::{
    NymArgs, OpenAiFilterSubprocessArgs, OpfRegistryArgs, RulepackOverrideArgs, SafetyNetLimitArgs,
    SafetyNetRegistryArgs,
};
use super::{OpenAiFilterDevice, SafetyNetBackend, SafetyNetKind};
use crate::error::CliError;
use crate::io::DEFAULT_MAX_BYTES;
use crate::pipeline::{run_clean, CleanOptions};

/// The complete `gaze clean` flag surface.
///
/// This struct is the only declaration of these flags: `Cmd::Clean` flattens
/// it, so parsing and the runtime call share one definition instead of a
/// declaration plus a hand-written destructure/restructure pair.
#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    /// Path to policy.toml. Without it, the bundled `core` rulepack runs.
    #[arg(long)]
    pub(crate) policy: Option<PathBuf>,
    /// Output format. Only `json` is supported today.
    #[arg(long, default_value = "json")]
    pub(crate) format: String,
    /// Override the persistent session TTL in seconds.
    #[arg(long)]
    pub(crate) session_ttl: Option<u64>,
    /// Override policy \[session].scope.
    #[arg(long)]
    pub(crate) session_scope: Option<String>,
    /// Active locale fallback chain, comma separated and priority ordered.
    #[arg(long, value_delimiter = ',')]
    pub(crate) locale: Vec<String>,
    /// Override policy \[ner] threshold. Must be between 0.0 and 1.0 inclusive.
    #[arg(long)]
    pub(crate) ner_threshold: Option<f32>,
    /// Override policy \[ner].model_dir.
    #[arg(long)]
    pub(crate) ner_model_dir: Option<PathBuf>,
    /// Override policy \[ner].locale.
    #[arg(long)]
    pub(crate) ner_locale: Option<String>,
    #[command(flatten)]
    pub(crate) rulepacks: RulepackOverrideArgs,
    /// Max stdin size in bytes. stdin longer than this exits 1 InputTooLarge.
    #[arg(long, default_value_t = DEFAULT_MAX_BYTES)]
    pub(crate) max_bytes: u64,
    /// Path to a typed Context JSON envelope. stdin remains raw text.
    #[arg(long)]
    pub(crate) context_json: Option<PathBuf>,
    /// Optional SQLite redaction-log database path.
    #[arg(long)]
    pub(crate) audit_db: Option<PathBuf>,
    /// Safety nets to run. Repeatable; "none" disables policy selection for this run.
    #[arg(long, value_enum)]
    pub(crate) safety_net: Vec<SafetyNetKind>,
    /// v0.8 backend selector. When set with
    /// one `--safety-net=<kind>`, this flag replaces it. Cannot select from a list.
    #[arg(long, value_enum)]
    pub(crate) safety_net_backend: Option<SafetyNetBackend>,
    #[command(flatten)]
    pub(crate) safety_net_registry: SafetyNetRegistryArgs,
    #[command(flatten)]
    pub(crate) openai_filter: OpenAiFilterSubprocessArgs,
    /// Device selection for the OpenAI safety-net subprocess (auto|cpu|cuda|mps). Default: auto (let opf decide).
    #[arg(long, value_enum, default_value_t = OpenAiFilterDevice::Auto)]
    pub(crate) openai_filter_device: OpenAiFilterDevice,
    #[command(flatten)]
    pub(crate) opf_registry: OpfRegistryArgs,
    #[command(flatten)]
    pub(crate) nym: NymArgs,
    #[command(flatten)]
    pub(crate) safety_net_limits: SafetyNetLimitArgs,
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
        rulepack_bundled: &args.rulepacks.rulepack_bundled,
        rulepack_paths: args.rulepacks.rulepack_paths,
        max_bytes: args.max_bytes,
        context_json: args.context_json.as_deref(),
        audit_db: args.audit_db.as_deref(),
        safety_net: &args.safety_net,
        safety_net_backend: args.safety_net_backend,
        safety_net_registry: args.safety_net_registry.safety_net_registry,
        safety_net_add: &args.safety_net_registry.safety_net_add,
        openai_filter_command: args.openai_filter.openai_filter_command.as_deref(),
        openai_filter_checkpoint: args.openai_filter.openai_filter_checkpoint.as_deref(),
        openai_filter_operating_point: args.openai_filter.openai_filter_operating_point,
        openai_filter_device: args.openai_filter_device,
        opf_locales: &args.opf_registry.opf_locales,
        opf_command: args.opf_registry.opf_command.as_deref(),
        opf_checkpoint: args.opf_registry.opf_checkpoint.as_deref(),
        nym_model_dir: args.nym.nym_model_dir.as_deref(),
        nym_intra_threads: args.nym.nym_intra_threads,
        safety_net_timeout_ms: args.safety_net_limits.safety_net_timeout_ms,
        safety_net_input_limit_bytes: args.safety_net_limits.safety_net_input_limit_bytes,
        safety_net_mode: args.safety_net_limits.safety_net_mode,
        safety_net_fallback: args.safety_net_limits.safety_net_fallback,
    })
}
