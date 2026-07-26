//! Dashboard opt-in orchestration for `gaze proxy`.
//!
//! Owns the child spawn through the current executable's hidden
//! `_dashboard-child` mode, pairing-token delivery over the controlling
//! terminal or a validated inherited FIFO descriptor, the one-shot
//! registration-bound activation sequence, the daemon flag relay, and
//! sanitized dashboard status lines. The `--dashboard`-absent path constructs
//! nothing: no entropy call, token, sink, child, or listener.
//!
//! Any decision or activation failure disables only the dashboard with one
//! closed sanitized reason; the provider starts or continues unchanged.

use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::unix::fs::FileTypeExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use gaze_proxy::ProxyInspectionProducerV1;
use gaze_proxy_dashboard::{
    ChildConfig, ChildInheritedHandles, ClientLimits, DashboardChildEntrypoint, DashboardLaunch,
    DashboardPayloadAcceptance, DashboardStartupConfig, DashboardSupervisor, IpcLimits,
    LoopbackBind, OwnerRawRiskAcknowledgement, OwnerRestoredRiskAcknowledgement, PairingDelivery,
    RetentionLimits, SpawnedDashboardChild,
};

use crate::error::CliError;

const DEFAULT_MAX_EVENTS: usize = 64;
const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);

/// Section 2 dashboard flags carried by `gaze proxy serve` and relayed
/// verbatim by `gaze proxy start`.
#[derive(Clone, Debug, Default)]
pub(crate) struct DashboardArgs {
    pub(crate) dashboard: bool,
    pub(crate) capture_owner_raw: bool,
    pub(crate) acknowledge_owner_raw_risk: bool,
    pub(crate) capture_owner_restored: bool,
    pub(crate) acknowledge_owner_restored_risk: bool,
    pub(crate) bind: Option<SocketAddrV4>,
    pub(crate) ttl: Option<String>,
    pub(crate) max_events: Option<usize>,
    pub(crate) max_bytes: Option<usize>,
    pub(crate) pairing_fd: Option<i32>,
}

impl DashboardArgs {
    fn any_flag_present(&self) -> bool {
        self.dashboard
            || self.capture_owner_raw
            || self.acknowledge_owner_raw_risk
            || self.capture_owner_restored
            || self.acknowledge_owner_restored_risk
            || self.bind.is_some()
            || self.ttl.is_some()
            || self.max_events.is_some()
            || self.max_bytes.is_some()
            || self.pairing_fd.is_some()
    }
}

/// Pure launch decision. `Off` constructs nothing and prints nothing;
/// `Disabled` is a dashboard-only fail-closed outcome with one closed reason;
/// `Enable` defers every side effect (entropy, child, token) to [`activate`].
pub(crate) enum DashboardDecision {
    Off,
    Disabled(&'static str),
    Enable(EnablePlan),
}

/// Validated flag set with no constructed runtime state.
pub(crate) struct EnablePlan {
    owner_raw: bool,
    owner_restored: bool,
    configured_bind: Option<SocketAddrV4>,
    max_events: usize,
    max_bytes: usize,
    ttl: Duration,
    pairing_fd: Option<i32>,
}

pub(crate) fn decide(args: &DashboardArgs) -> DashboardDecision {
    if !args.any_flag_present() {
        return DashboardDecision::Off;
    }
    if !args.dashboard {
        return DashboardDecision::Disabled(
            "dashboard flags require --dashboard; dashboard stays off",
        );
    }
    if args.capture_owner_raw != args.acknowledge_owner_raw_risk {
        return DashboardDecision::Disabled(
            "owner-raw capture requires --dashboard-acknowledge-owner-raw-risk and vice versa",
        );
    }
    if args.capture_owner_restored != args.acknowledge_owner_restored_risk {
        return DashboardDecision::Disabled(
            "owner-restored capture requires --dashboard-acknowledge-owner-restored-risk and vice versa",
        );
    }
    if let Some(fd) = args.pairing_fd {
        if fd <= 2 {
            return DashboardDecision::Disabled(
                "pairing descriptor must be an inherited descriptor above stdio",
            );
        }
    }
    if let Some(authority) = args.bind {
        if !authority.ip().is_loopback() || authority.port() != 0 {
            return DashboardDecision::Disabled(
                "dashboard bind must be a literal loopback address with port 0",
            );
        }
    }
    let ttl = match args.ttl.as_deref() {
        None => DEFAULT_TTL,
        Some(raw) => match super::proxy::parse_duration(raw) {
            Ok(ttl) => ttl,
            Err(_) => return DashboardDecision::Disabled("invalid dashboard ttl"),
        },
    };
    let max_events = args.max_events.unwrap_or(DEFAULT_MAX_EVENTS);
    let max_bytes = args.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    if RetentionLimits::new(max_events, max_bytes, ttl).is_err() {
        return DashboardDecision::Disabled(
            "dashboard retention limits are zero or exceed the crate hard ceilings",
        );
    }
    DashboardDecision::Enable(EnablePlan {
        owner_raw: args.capture_owner_raw,
        owner_restored: args.capture_owner_restored,
        configured_bind: args.bind,
        max_events,
        max_bytes,
        ttl,
        pairing_fd: args.pairing_fd,
    })
}

/// Rebuilds the exact non-secret dashboard flag vector for the daemon serve
/// process. `gaze proxy start` relays these; `restart` intentionally does not
/// (dashboard activation is per-invocation and never persisted).
pub(crate) fn relay_extra_args(args: &DashboardArgs) -> Vec<String> {
    let mut extra = Vec::new();
    if args.dashboard {
        extra.push("--dashboard".to_owned());
    }
    if args.capture_owner_raw {
        extra.push("--dashboard-capture-owner-raw".to_owned());
    }
    if args.acknowledge_owner_raw_risk {
        extra.push("--dashboard-acknowledge-owner-raw-risk".to_owned());
    }
    if args.capture_owner_restored {
        extra.push("--dashboard-capture-owner-restored".to_owned());
    }
    if args.acknowledge_owner_restored_risk {
        extra.push("--dashboard-acknowledge-owner-restored-risk".to_owned());
    }
    if let Some(bind) = args.bind {
        extra.push("--dashboard-bind".to_owned());
        extra.push(bind.to_string());
    }
    if let Some(ttl) = &args.ttl {
        extra.push("--dashboard-ttl".to_owned());
        extra.push(ttl.clone());
    }
    if let Some(max_events) = args.max_events {
        extra.push("--dashboard-max-events".to_owned());
        extra.push(max_events.to_string());
    }
    if let Some(max_bytes) = args.max_bytes {
        extra.push("--dashboard-max-bytes".to_owned());
        extra.push(max_bytes.to_string());
    }
    if let Some(fd) = args.pairing_fd {
        extra.push("--dashboard-pairing-fd".to_owned());
        extra.push(fd.to_string());
    }
    extra
}

/// Successfully activated dashboard: the producer master hands to the proxy
/// only after commit, plus the launch handle whose drop path terminates and
/// reaps the child.
pub(crate) struct ActiveDashboard {
    pub(crate) producer: ProxyInspectionProducerV1,
    pub(crate) launch: DashboardLaunch,
}

/// Runs the exact one-shot activation sequence: spawn the child through the
/// current executable's hidden mode, complete nonce-bound pairing delivery,
/// extract the pending activation once, atomically install producer and
/// consumer through `gaze_proxy::install_proxy_inspection_v1`, and commit the
/// activated consumer against the retained binding before any socket, writer,
/// runtime, or admission side effect. Every failure returns one closed reason
/// and drops all partial dashboard state (kill/reap/zeroize via drop paths).
pub(crate) fn activate(plan: EnablePlan) -> Result<ActiveDashboard, &'static str> {
    let address = match plan.configured_bind {
        Some(authority) => {
            eprintln!(
                "gaze dashboard warning: configured loopback origin may be reused by other local software; the default fresh CSPRNG origin avoids browser state reuse"
            );
            *authority.ip()
        }
        None => fresh_loopback_literal()?,
    };
    // The child performs the actual TCP bind, so parent and child must agree
    // on the exact literal; both sides construct the same `LoopbackBind` from
    // this one address (`LoopbackBind::fresh_ephemeral_v4` keeps its drawn
    // address private and therefore cannot cross the process boundary).
    let bind = LoopbackBind::configured(SocketAddrV4::new(address, 0))
        .map_err(|_| "dashboard bind must be a literal loopback address with port 0")?;
    let retention = RetentionLimits::new(plan.max_events, plan.max_bytes, plan.ttl)
        .map_err(|_| "dashboard retention limits are zero or exceed the crate hard ceilings")?;
    let mut acceptance = DashboardPayloadAcceptance::provider_visible();
    if plan.owner_raw {
        acceptance = acceptance.with_owner_raw(OwnerRawRiskAcknowledgement::acknowledge_pii_risk());
    }
    if plan.owner_restored {
        acceptance = acceptance.with_owner_restored(
            OwnerRestoredRiskAcknowledgement::acknowledge_reidentification_risk(),
        );
    }
    let delivery = build_delivery(plan.pairing_fd)?;
    let command = child_command(address, plan.max_events, plan.max_bytes, plan.ttl)
        .map_err(|_| "dashboard child executable unavailable")?;
    let config = DashboardStartupConfig::Enabled {
        acceptance,
        bind,
        retention,
        clients: ClientLimits::conservative(),
        ipc: IpcLimits::conservative(),
    };
    let child = SpawnedDashboardChild::spawn(command)
        .map_err(|_| "dashboard child spawn or channel setup failed")?;
    let paired = DashboardSupervisor::prepare(config, child, delivery)
        .map_err(|_| "dashboard pairing failed")?;
    let (pending, consumer, descriptor) = paired
        .into_pending_activation()
        .map_err(|_| "dashboard pending activation extraction failed")?;
    let (producer, activated) = gaze_proxy::install_proxy_inspection_v1(descriptor, consumer)
        .map_err(|_| "dashboard inspection installation failed")?;
    let launch = pending
        .commit(activated)
        .map_err(|_| "dashboard activation commit failed")?;
    Ok(ActiveDashboard { producer, launch })
}

/// Draws a fresh CSPRNG literal in `127.0.0.0/8` (port 0 is applied by the
/// caller), mirroring the plan's browser-mode default origin shape. `127.0.0.0`
/// itself is avoided so the literal always names a bindable host address.
fn fresh_loopback_literal() -> Result<Ipv4Addr, &'static str> {
    let mut octets = [0_u8; 3];
    getrandom::fill(&mut octets).map_err(|_| "dashboard entropy unavailable")?;
    if octets == [0, 0, 0] {
        octets[2] = 1;
    }
    Ok(Ipv4Addr::new(127, octets[0], octets[1], octets[2]))
}

/// Bind address plus non-secret retention limits for the hidden child mode.
/// The two socket paths travel through crate-owned environment variables set
/// by `SpawnedDashboardChild::spawn`; no secret crosses the argument list.
fn child_command(
    address: Ipv4Addr,
    max_events: usize,
    max_bytes: usize,
    ttl: Duration,
) -> io::Result<Command> {
    let current_exe = std::env::current_exe()?;
    let mut command = Command::new(current_exe);
    command
        .args(["proxy", "_dashboard-child"])
        .arg("--bind-addr")
        .arg(address.to_string())
        .arg("--max-events")
        .arg(max_events.to_string())
        .arg("--max-bytes")
        .arg(max_bytes.to_string())
        .arg("--ttl-secs")
        .arg(ttl.as_secs().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    Ok(command)
}

/// Pairing delivery over the controlling terminal or a validated inherited
/// FIFO descriptor. Token bytes are written directly from the supervisor's
/// borrowed buffer; this type keeps no copy.
struct CliPairingDelivery {
    output: std::fs::File,
}

impl PairingDelivery for CliPairingDelivery {
    fn deliver(&mut self, authority: SocketAddrV4, token: &[u8]) -> io::Result<()> {
        self.output.write_all(b"gaze dashboard: http://")?;
        self.output.write_all(authority.to_string().as_bytes())?;
        self.output
            .write_all(b"/\ngaze dashboard authorization: GazeDashboardV1 ")?;
        self.output.write_all(token)?;
        self.output.write_all(b"\n")?;
        self.output.flush()
    }
}

/// Opens the pairing output. Without `--dashboard-pairing-fd` the delivery
/// target is the controlling terminal only; stdout/stderr are never used so a
/// daemonized parent cannot write the token into a log file. An inherited
/// descriptor must be above stdio and stat as a FIFO through `/dev/fd`;
/// terminals and other character devices, regular files, directories,
/// sockets, and unopenable or read-only descriptors are rejected closed.
fn build_delivery(pairing_fd: Option<i32>) -> Result<CliPairingDelivery, &'static str> {
    match pairing_fd {
        None => {
            let tty = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/tty")
                .map_err(|_| {
                    "no controlling terminal for pairing display; pass --dashboard-pairing-fd"
                })?;
            Ok(CliPairingDelivery { output: tty })
        }
        Some(fd) => {
            if fd <= 2 {
                return Err("pairing descriptor must be an inherited descriptor above stdio");
            }
            let path = format!("/dev/fd/{fd}");
            let metadata = std::fs::metadata(&path)
                .map_err(|_| "pairing descriptor is not open in this process")?;
            if !metadata.file_type().is_fifo() {
                return Err("pairing descriptor must be a FIFO");
            }
            let output = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .map_err(|_| "pairing descriptor is not writable")?;
            Ok(CliPairingDelivery { output })
        }
    }
}

/// Hidden `_dashboard-child` arguments (all non-secret).
pub(crate) struct ChildModeArgs {
    pub(crate) bind_addr: Ipv4Addr,
    pub(crate) ttl_secs: u64,
    pub(crate) max_events: usize,
    pub(crate) max_bytes: usize,
}

/// Runs the sensitive dashboard child runtime. Exits data-free on any error:
/// the only diagnostic is the closed error code name.
pub(crate) fn run_child(args: ChildModeArgs) -> Result<(), CliError> {
    let handles = ChildInheritedHandles::connect_from_environment().map_err(child_error)?;
    let bind =
        LoopbackBind::configured(SocketAddrV4::new(args.bind_addr, 0)).map_err(child_error)?;
    let retention = RetentionLimits::new(
        args.max_events,
        args.max_bytes,
        Duration::from_secs(args.ttl_secs),
    )
    .map_err(child_error)?;
    let config = ChildConfig::new(
        bind,
        retention,
        ClientLimits::conservative(),
        IpcLimits::conservative(),
    );
    DashboardChildEntrypoint::run(handles, config).map_err(child_error)
}

fn child_error(error: gaze_proxy_dashboard::DashboardError) -> CliError {
    CliError::ProxyDetail(format!("dashboard child: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> DashboardArgs {
        DashboardArgs {
            dashboard: true,
            ..DashboardArgs::default()
        }
    }

    #[test]
    fn absent_flags_decide_off() {
        assert!(matches!(
            decide(&DashboardArgs::default()),
            DashboardDecision::Off
        ));
    }

    #[test]
    fn owner_flag_without_dashboard_disables_only_dashboard() {
        let args = DashboardArgs {
            capture_owner_raw: true,
            acknowledge_owner_raw_risk: true,
            ..DashboardArgs::default()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn owner_raw_without_acknowledgement_disables() {
        let args = DashboardArgs {
            capture_owner_raw: true,
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn acknowledgement_without_owner_raw_disables() {
        let args = DashboardArgs {
            acknowledge_owner_raw_risk: true,
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn owner_restored_without_acknowledgement_disables() {
        let args = DashboardArgs {
            capture_owner_restored: true,
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn both_owner_domains_require_both_acknowledgements() {
        let args = DashboardArgs {
            capture_owner_raw: true,
            acknowledge_owner_raw_risk: true,
            capture_owner_restored: true,
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn stdio_pairing_descriptors_are_rejected() {
        for fd in [0, 1, 2] {
            let args = DashboardArgs {
                pairing_fd: Some(fd),
                ..base_args()
            };
            assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
        }
    }

    #[test]
    fn non_loopback_bind_is_rejected() {
        let args = DashboardArgs {
            bind: Some("192.0.2.1:0".parse().unwrap()),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn loopback_bind_with_nonzero_port_is_rejected() {
        let args = DashboardArgs {
            bind: Some("127.0.0.1:8080".parse().unwrap()),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn retention_over_crate_ceiling_is_rejected() {
        let args = DashboardArgs {
            max_events: Some(1_025),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
        let args = DashboardArgs {
            max_bytes: Some(64 * 1024 * 1024 + 1),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
        let args = DashboardArgs {
            ttl: Some("2h".to_owned()),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn zero_retention_limit_is_rejected() {
        let args = DashboardArgs {
            max_events: Some(0),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn invalid_ttl_string_is_rejected() {
        let args = DashboardArgs {
            ttl: Some("soon".to_owned()),
            ..base_args()
        };
        assert!(matches!(decide(&args), DashboardDecision::Disabled(_)));
    }

    #[test]
    fn valid_dashboard_flags_enable_without_construction() {
        let decision = decide(&base_args());
        let DashboardDecision::Enable(plan) = decision else {
            panic!("expected enable");
        };
        assert_eq!(plan.max_events, DEFAULT_MAX_EVENTS);
        assert_eq!(plan.max_bytes, DEFAULT_MAX_BYTES);
        assert_eq!(plan.ttl, DEFAULT_TTL);
        assert!(plan.configured_bind.is_none());
        assert!(plan.pairing_fd.is_none());
        assert!(!plan.owner_raw);
        assert!(!plan.owner_restored);
    }

    #[test]
    fn relay_round_trips_every_present_flag() {
        let args = DashboardArgs {
            dashboard: true,
            capture_owner_raw: true,
            acknowledge_owner_raw_risk: true,
            capture_owner_restored: true,
            acknowledge_owner_restored_risk: true,
            bind: Some("127.0.0.1:0".parse().unwrap()),
            ttl: Some("1m".to_owned()),
            max_events: Some(8),
            max_bytes: Some(1_024),
            pairing_fd: Some(7),
        };
        let extra = relay_extra_args(&args);
        assert_eq!(
            extra,
            vec![
                "--dashboard",
                "--dashboard-capture-owner-raw",
                "--dashboard-acknowledge-owner-raw-risk",
                "--dashboard-capture-owner-restored",
                "--dashboard-acknowledge-owner-restored-risk",
                "--dashboard-bind",
                "127.0.0.1:0",
                "--dashboard-ttl",
                "1m",
                "--dashboard-max-events",
                "8",
                "--dashboard-max-bytes",
                "1024",
                "--dashboard-pairing-fd",
                "7",
            ]
        );
    }

    #[test]
    fn relay_of_absent_flags_is_empty() {
        assert!(relay_extra_args(&DashboardArgs::default()).is_empty());
    }

    #[test]
    fn fresh_loopback_literal_stays_in_loopback_block() {
        for _ in 0..64 {
            let address = fresh_loopback_literal().unwrap();
            assert_eq!(address.octets()[0], 127);
            assert!(address.is_loopback());
            assert_ne!(address, Ipv4Addr::new(127, 0, 0, 0));
        }
    }
}
