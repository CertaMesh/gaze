//! Argument groups shared by more than one `gaze` subcommand.
//!
//! Each struct here is the single owner of its flags. Subcommands pull them in
//! with `#[command(flatten)]`, so a flag cannot exist on one verb and quietly
//! go missing on a sibling: adding a field here adds it everywhere, and
//! removing a `flatten` is caught by the parity tests in [`super`].
//!
//! Only flags whose declaration is *identical* across every consumer belong
//! here. Where two verbs describe the same flag differently, the divergence is
//! recorded by `clean_and_daemon_flag_divergence_is_exactly_the_reviewed_set`
//! rather than papered over by unifying help text, which would change the
//! published CLI surface.

use std::path::{Path, PathBuf};

use clap::Args;

use super::{OpenAiFilterDevice, SafetyNetKind};
use super::{OpenAiFilterOperatingPoint, SafetyNetBackend, SafetyNetFallback, SafetyNetMode};
use crate::pipeline::CleanOptions;

/// Detection and safety-net flags shared by text cleaning and corpus evaluation.
#[derive(Args, Debug)]
pub(crate) struct CleanPipelineArgs {
    /// Path to policy.toml. Without it, the bundled `core` rulepack runs.
    #[arg(long)]
    pub(crate) policy: Option<PathBuf>,
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
    /// Path to a typed Context JSON envelope. Input text remains raw text.
    #[arg(long)]
    pub(crate) context_json: Option<PathBuf>,
    /// Safety nets to run. Repeatable; "none" disables policy selection for this run.
    #[arg(long, value_enum)]
    pub(crate) safety_net: Vec<SafetyNetKind>,
    /// v0.8 backend selector. When set with one `--safety-net=<kind>`, this flag replaces it. Cannot select from a list.
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

impl CleanPipelineArgs {
    pub(crate) fn options<'a>(
        &'a self,
        format: &'a str,
        max_bytes: u64,
        audit_db: Option<&'a Path>,
    ) -> CleanOptions<'a> {
        CleanOptions {
            policy: self.policy.as_deref(),
            format,
            session_ttl: self.session_ttl,
            session_scope: self.session_scope.as_deref(),
            locale: &self.locale,
            ner_threshold: self.ner_threshold,
            ner_model_dir: self.ner_model_dir.clone(),
            ner_locale: self.ner_locale.as_deref(),
            rulepack_bundled: &self.rulepacks.rulepack_bundled,
            rulepack_paths: self.rulepacks.rulepack_paths.clone(),
            max_bytes,
            context_json: self.context_json.as_deref(),
            audit_db,
            safety_net: &self.safety_net,
            safety_net_backend: self.safety_net_backend,
            safety_net_registry: self.safety_net_registry.safety_net_registry,
            safety_net_add: &self.safety_net_registry.safety_net_add,
            openai_filter_command: self.openai_filter.openai_filter_command.as_deref(),
            openai_filter_checkpoint: self.openai_filter.openai_filter_checkpoint.as_deref(),
            openai_filter_operating_point: self.openai_filter.openai_filter_operating_point,
            openai_filter_device: self.openai_filter_device,
            opf_locales: &self.opf_registry.opf_locales,
            opf_command: self.opf_registry.opf_command.as_deref(),
            opf_checkpoint: self.opf_registry.opf_checkpoint.as_deref(),
            nym_model_dir: self.nym.nym_model_dir.as_deref(),
            nym_intra_threads: self.nym.nym_intra_threads,
            safety_net_timeout_ms: self.safety_net_limits.safety_net_timeout_ms,
            safety_net_input_limit_bytes: self.safety_net_limits.safety_net_input_limit_bytes,
            safety_net_mode: self.safety_net_limits.safety_net_mode,
            safety_net_fallback: self.safety_net_limits.safety_net_fallback,
        }
    }
}

/// OpenAI Privacy Filter subprocess location and operating point.
///
/// Shared by `gaze clean` and `gaze daemon`.
#[derive(Args, Debug)]
pub(crate) struct OpenAiFilterSubprocessArgs {
    /// Path to the local OpenAI Privacy Filter `opf` command.
    #[arg(long)]
    pub(crate) openai_filter_command: Option<PathBuf>,
    /// Path to the local OpenAI Privacy Filter checkpoint or model directory.
    #[arg(long)]
    pub(crate) openai_filter_checkpoint: Option<PathBuf>,
    /// OpenAI Privacy Filter operating point, when supported by the command.
    #[arg(long, value_enum)]
    pub(crate) openai_filter_operating_point: Option<OpenAiFilterOperatingPoint>,
}

/// Pass-3 safety-net budget and failure handling.
///
/// Shared by `gaze clean` and `gaze daemon`. These four decide what happens to
/// a suspected residual leak, so a verb that silently lacked one would run a
/// weaker safety net than its sibling under the same policy — the reason this
/// group is owned in one place.
#[derive(Args, Debug)]
pub(crate) struct SafetyNetLimitArgs {
    /// Safety-net subprocess timeout in milliseconds.
    #[arg(long, default_value_t = super::DEFAULT_SAFETY_NET_TIMEOUT_MS)]
    pub(crate) safety_net_timeout_ms: u64,
    /// Maximum clean-text bytes submitted to the safety net.
    #[arg(long, default_value_t = super::DEFAULT_SAFETY_NET_INPUT_LIMIT_BYTES)]
    pub(crate) safety_net_input_limit_bytes: usize,
    /// Safety-net handling mode for suspected leaks.
    #[arg(long, value_enum, default_value_t = SafetyNetMode::Resolve)]
    pub(crate) safety_net_mode: SafetyNetMode,
    /// Fallback when safety-net resolve or redact cannot complete.
    #[arg(long, value_enum, default_value_t = SafetyNetFallback::Redact)]
    pub(crate) safety_net_fallback: SafetyNetFallback,
}

/// Locale-aware Pass-3 safety-net registry activation.
///
/// Shared by `gaze clean` and `gaze daemon`. Safety-net backend selection has
/// no policy.toml equivalent — [`gaze::Policy`] carries no safety-net section —
/// so a verb without these flags cannot run the multi-backend registry under
/// *any* configuration, only the single-backend path. That made the daemon
/// chokepoint structurally weaker than `clean` under an identical policy, which is why this group is owned in one place.
#[derive(Args, Debug)]
pub(crate) struct SafetyNetRegistryArgs {
    /// Enable locale-aware Pass-3 safety-net registry dispatch.
    #[arg(long)]
    pub(crate) safety_net_registry: bool,
    /// Add one backend to the locale-aware safety-net registry. Repeatable.
    #[arg(long, value_enum)]
    pub(crate) safety_net_add: Vec<SafetyNetBackend>,
}

/// OpenAI Privacy Filter registry-entry configuration.
///
/// Shared by `gaze clean` and `gaze daemon`. `--opf-locales` is the only way to
/// scope the OPF entry to a locale, so without it a registry is registered but
/// cannot be made locale-aware — the whole point of registry dispatch. The two
/// aliases keep a working `clean` registry command line valid when it is moved
/// to `daemon`.
#[derive(Args, Debug)]
pub(crate) struct OpfRegistryArgs {
    /// Locale list for the OpenAI Privacy Filter registry entry.
    #[arg(long, value_delimiter = ',')]
    pub(crate) opf_locales: Vec<String>,
    /// Alias for --openai-filter-command in registry examples.
    #[arg(long)]
    pub(crate) opf_command: Option<PathBuf>,
    /// Alias for --openai-filter-checkpoint in registry examples.
    #[arg(long)]
    pub(crate) opf_checkpoint: Option<PathBuf>,
}

/// Nym-small safety-net backend configuration.
///
/// Shared by `gaze clean` and `gaze daemon`. The allowlist and thresholds live in policy.toml
/// (`[safety_net.nym]`, op-B when absent); these flags only say where the pinned bundle is and
/// how many ONNX Runtime threads it may use.
#[derive(Args, Debug)]
pub(crate) struct NymArgs {
    /// Path to the pinned Nym-small int8 bundle from `gaze setup --safety-net nym` (default: GAZE_NYM_MODEL_DIR)
    #[arg(long)]
    pub(crate) nym_model_dir: Option<PathBuf>,
    /// ONNX Runtime intra-op threads for the Nym backend (default: 1)
    #[arg(long)]
    pub(crate) nym_intra_threads: Option<std::num::NonZeroUsize>,
}

/// policy.toml rulepack overrides.
///
/// Shared by `gaze clean` and `gaze daemon`. Rulepacks decide which recognizers
/// run, so this is a detection-surface control (axis 1). The override plumbing
/// already reached the daemon through `clean_overrides_from_options`; only the
/// flags were missing, leaving the wiring hardcoded to "no override".
#[derive(Args, Debug)]
pub(crate) struct RulepackOverrideArgs {
    /// Override policy.rulepacks.bundled. Comma-separated and repeatable; "none" disables all bundled packs.
    #[arg(long, value_delimiter = ',')]
    pub(crate) rulepack_bundled: Vec<String>,
    /// Override policy.rulepacks.paths. Repeatable.
    #[arg(long = "rulepack-path")]
    pub(crate) rulepack_paths: Vec<PathBuf>,
}
