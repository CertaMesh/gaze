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

use std::path::PathBuf;

use clap::Args;

use super::{
    KijiDistilbertPrecision, OpenAiFilterOperatingPoint, SafetyNetBackend, SafetyNetFallback,
    SafetyNetMode,
};

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
/// chokepoint structurally weaker than `clean` under an identical policy
/// (solo todo #3004), which is why this group is owned in one place.
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

/// Kiji DistilBERT ONNX precision selection.
///
/// Shared by `gaze clean` and `gaze daemon`. Precision has no policy.toml
/// equivalent either, so hardcoding it pinned the daemon to fp32 with no way to
/// ask for the int8 build. One field, but owned here for the same reason as the
/// groups above: a re-declaration on one verb only is how the two drift apart.
#[derive(Args, Debug)]
pub(crate) struct KijiPrecisionArgs {
    /// Kiji DistilBERT ONNX precision. Default: fp32.
    #[arg(long, value_enum, default_value_t = KijiDistilbertPrecision::Fp32)]
    pub(crate) kiji_distilbert_precision: KijiDistilbertPrecision,
}

/// policy.toml rulepack overrides.
///
/// Shared by `gaze clean` and `gaze daemon`. Rulepacks decide which recognizers
/// run, so this is a detection-surface control (axis 1). The override plumbing
/// already reached the daemon through `clean_overrides_from_options`; only the
/// flags were missing, leaving the wiring hardcoded to "no override".
#[derive(Args, Debug)]
pub(crate) struct RulepackOverrideArgs {
    /// Override policy.rulepacks.bundled. Comma-separated and repeatable.
    #[arg(long, value_delimiter = ',')]
    pub(crate) rulepack_bundled: Vec<String>,
    /// Override policy.rulepacks.paths. Repeatable.
    #[arg(long = "rulepack-path")]
    pub(crate) rulepack_paths: Vec<PathBuf>,
}
