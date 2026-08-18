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

use super::{OpenAiFilterOperatingPoint, SafetyNetFallback, SafetyNetMode};

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
