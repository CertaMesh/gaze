use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gaze_proxy::adapters::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use gaze_proxy::daemon::{self, AdapterConfig, DaemonConfig, DaemonPaths};
use gaze_proxy::ProxyConfig;
use url::Url;

use crate::error::CliError;
use crate::pipeline::build::{
    build_pipeline_from_policy, load_rulepacks, map_pipeline_error, map_policy_error,
    merged_rulepack_default_locales, resolve_ner_threshold,
};

pub(crate) struct ServeArgs {
    pub(crate) bind: SocketAddr,
    pub(crate) upstream_openai: Url,
    pub(crate) upstream_anthropic: Url,
    pub(crate) upstream_gemini: Url,
    pub(crate) policy: Option<PathBuf>,
    pub(crate) rulepack: String,
    pub(crate) session_ttl: String,
    pub(crate) foreground_daemon: bool,
    #[cfg(feature = "dashboard")]
    pub(crate) dashboard: super::proxy_dashboard::DashboardArgs,
}

pub(crate) struct StartArgs {
    pub(crate) bind: Option<SocketAddr>,
    pub(crate) upstream_openai: Option<Url>,
    pub(crate) upstream_anthropic: Option<Url>,
    pub(crate) upstream_gemini: Option<Url>,
    pub(crate) policy: Option<PathBuf>,
    pub(crate) rulepack: Option<String>,
    pub(crate) session_ttl: Option<String>,
    #[cfg(feature = "dashboard")]
    pub(crate) dashboard: super::proxy_dashboard::DashboardArgs,
}

pub(crate) struct StopArgs {
    pub(crate) force: bool,
    pub(crate) timeout: String,
}

pub(crate) struct RestartArgs {
    pub(crate) force: bool,
    pub(crate) timeout: String,
}

pub(crate) fn serve(args: ServeArgs) -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    if args.foreground_daemon {
        daemon::init_foreground_daemon(&paths, args.bind).map_err(map_proxy)?;
    }
    let mut config = proxy_config(
        args.bind,
        args.upstream_openai,
        args.upstream_anthropic,
        args.upstream_gemini,
    );
    config.session_ttl = parse_duration(&args.session_ttl)?;
    #[cfg(feature = "dashboard")]
    let mut _dashboard_launch = None;
    #[cfg(feature = "dashboard")]
    {
        use super::proxy_dashboard::{self, DashboardDecision};
        match proxy_dashboard::decide(&args.dashboard) {
            DashboardDecision::Off => {}
            DashboardDecision::Disabled(reason) => {
                eprintln!("gaze dashboard disabled: {reason}");
            }
            DashboardDecision::Enable(plan) => match proxy_dashboard::activate(plan) {
                Ok(active) => {
                    config = config.with_inspection(active.producer);
                    _dashboard_launch = Some(active.launch);
                    eprintln!("gaze dashboard active");
                }
                Err(reason) => eprintln!("gaze dashboard disabled: {reason}"),
            },
        }
    }
    // The locale chain the pipeline was assembled under must also reach the proxy: assembly
    // decides which recognizers are registered, `ProxyConfig` decides which may fire. Dropping
    // it here is what pinned proxied traffic to `[LocaleTag::Global]` and left locale-gated
    // recognizers inert for adopters who had configured a locale (solo todo #2403).
    let (pipeline, locale_chain) = build_pipeline(args.policy, &args.rulepack)?;
    config = config.with_locale_chain(locale_chain);
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|err| CliError::ProxyDetail(format!("runtime: {err}")))?;
    runtime
        .block_on(gaze_proxy::serve(config, Arc::new(pipeline)))
        .map_err(map_proxy)
}

pub(crate) fn start(args: StartArgs) -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    let mut config = daemon::read_or_default_config(&paths).map_err(map_proxy)?;
    #[cfg(feature = "dashboard")]
    let dashboard_args = args.dashboard.clone();
    apply_start_overrides(&mut config, args);
    let start_options = daemon::StartOptions::new(paths.clone(), config.clone());
    #[cfg(feature = "dashboard")]
    let start_options =
        start_options.with_extra_args(super::proxy_dashboard::relay_extra_args(&dashboard_args));
    let pid = daemon::start(start_options).map_err(map_proxy)?;
    println!(
        "gaze-proxy started (pid={pid}, bind={}, log={})",
        config.bind,
        paths.log_file.display()
    );
    Ok(())
}

pub(crate) fn stop(args: StopArgs) -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    daemon::stop(daemon::StopOptions::new(
        paths,
        parse_duration(&args.timeout)?,
        args.force,
    ))
    .map_err(map_proxy)?;
    println!("gaze-proxy stopped");
    Ok(())
}

pub(crate) fn status() -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    match daemon::status(&paths).map_err(map_proxy)? {
        Some(status) if status.running => {
            println!(
                "gaze-proxy running (pid={}, bind={})",
                status.pid,
                status.bind.unwrap_or_else(|| "unknown".to_string())
            );
            let config = daemon::read_or_default_config(&paths).map_err(map_proxy)?;
            println!("  adapters: openai -> {}", config.adapters.openai.upstream);
            println!(
                "            anthropic -> {}",
                config.adapters.anthropic.upstream
            );
            println!("            gemini -> {}", config.adapters.gemini.upstream);
        }
        Some(status) => {
            println!("gaze-proxy not running (stale pidfile pid={})", status.pid);
        }
        None => println!("gaze-proxy not running"),
    }
    Ok(())
}

pub(crate) fn logs(follow: bool) -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    daemon::logs(&paths, follow).map_err(map_proxy)
}

pub(crate) fn restart(args: RestartArgs) -> Result<(), CliError> {
    let paths = DaemonPaths::resolve().map_err(map_proxy)?;
    let config = daemon::read_or_default_config(&paths).map_err(map_proxy)?;
    let pid = daemon::restart(
        daemon::StartOptions::new(paths.clone(), config.clone()),
        parse_duration(&args.timeout)?,
        args.force,
    )
    .map_err(map_proxy)?;
    println!(
        "gaze-proxy restarted (pid={pid}, bind={}, log={})",
        config.bind,
        paths.log_file.display()
    );
    Ok(())
}

pub(crate) fn install_launchd() -> Result<(), CliError> {
    Err(CliError::ProxyDetail(
        "launchd integration is reserved for v0.8.x; use `gaze proxy start`".to_string(),
    ))
}

pub(crate) fn uninstall_launchd() -> Result<(), CliError> {
    Err(CliError::ProxyDetail(
        "launchd integration is reserved for v0.8.x; use `gaze proxy stop`".to_string(),
    ))
}

pub(crate) fn install_systemd_user() -> Result<(), CliError> {
    Err(CliError::ProxyDetail(
        "systemd user integration is reserved for v0.8.x; use `gaze proxy start`".to_string(),
    ))
}

pub(crate) fn uninstall_systemd_user() -> Result<(), CliError> {
    Err(CliError::ProxyDetail(
        "systemd user integration is reserved for v0.8.x; use `gaze proxy stop`".to_string(),
    ))
}

/// Builds the proxy pipeline and returns the locale chain it was assembled under.
///
/// The chain is the canonical 4-tier resolution (CLI > policy > rulepack default > system
/// default). `gaze proxy` has no `--locale` flag, so the CLI tier is empty and a policy
/// `locale = [...]` is the adopter's lever; with no policy the bundled defaults resolve to
/// `[LocaleTag::Global]`. Both values are returned together because the proxy needs them
/// together — see [`gaze_proxy::ProxyConfig::with_locale_chain`].
fn build_pipeline(
    policy: Option<PathBuf>,
    rulepack: &str,
) -> Result<(gaze::Pipeline, gaze::LocaleChain), CliError> {
    if let Some(path) = policy {
        let policy = gaze::Policy::load_for_cli(&path).map_err(map_policy_error)?;
        let rulepacks = load_rulepacks(&policy).map_err(map_pipeline_error)?;
        let rulepack_default_locales = merged_rulepack_default_locales(&rulepacks);
        let locale_chain = gaze::LocaleChain::merge_cli_policy_rulepack_default(
            None,
            policy.locale.as_deref(),
            Some(&rulepack_default_locales),
        );
        let pipeline = build_pipeline_from_policy(
            &policy,
            &rulepacks,
            None,
            &locale_chain,
            resolve_ner_threshold(None, Some(&policy)),
        )?;
        return Ok((pipeline, locale_chain));
    }
    let mut config = gaze_assembly::CorePipelineConfig::new();
    if rulepack != "core" {
        config = config.with_bundled_rulepack(rulepack);
    }
    let core = config
        .build()
        .map_err(|err| CliError::ProxyDetail(format!("pipeline: {err}")))?;
    let locale_chain = core.locale_chain().clone();
    Ok((core.into_pipeline(), locale_chain))
}

fn proxy_config(
    bind: SocketAddr,
    upstream_openai: Url,
    upstream_anthropic: Url,
    upstream_gemini: Url,
) -> ProxyConfig {
    ProxyConfig::new(
        bind,
        vec![
            Arc::new(OpenAiAdapter::new(upstream_openai)),
            Arc::new(AnthropicAdapter::new(upstream_anthropic)),
            Arc::new(GeminiAdapter::new(upstream_gemini)),
        ],
    )
}

fn apply_start_overrides(config: &mut DaemonConfig, args: StartArgs) {
    if let Some(bind) = args.bind {
        config.bind = bind;
    }
    if let Some(policy) = args.policy {
        config.policy = Some(policy);
    }
    if let Some(rulepack) = args.rulepack {
        config.rulepack = Some(rulepack);
    }
    if let Some(session_ttl) = args.session_ttl {
        config.session_ttl = session_ttl;
    }
    config.adapters = AdapterConfig::new(
        args.upstream_openai
            .unwrap_or_else(|| config.adapters.openai.upstream.clone()),
        args.upstream_anthropic
            .unwrap_or_else(|| config.adapters.anthropic.upstream.clone()),
        args.upstream_gemini
            .unwrap_or_else(|| config.adapters.gemini.upstream.clone()),
    );
}

pub(crate) fn parse_duration(input: &str) -> Result<Duration, CliError> {
    let trimmed = input.trim();
    let parse_number = |suffix: &str| {
        trimmed
            .strip_suffix(suffix)
            .and_then(|value| value.parse::<u64>().ok())
    };
    if let Some(minutes) = parse_number("m") {
        Ok(Duration::from_secs(minutes * 60))
    } else if let Some(seconds) = parse_number("s") {
        Ok(Duration::from_secs(seconds))
    } else if let Some(hours) = parse_number("h") {
        Ok(Duration::from_secs(hours * 60 * 60))
    } else if let Ok(seconds) = trimmed.parse::<u64>() {
        Ok(Duration::from_secs(seconds))
    } else {
        Err(CliError::ProxyDetail(format!(
            "invalid duration `{input}`; use Ns, Nm, Nh, or seconds"
        )))
    }
}

fn map_proxy(err: gaze_proxy::ProxyError) -> CliError {
    CliError::ProxyDetail(err.to_string())
}
