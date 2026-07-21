use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gaze_types::inspection::{
    InspectionEmissionIdV1, InspectionEpochV1, InspectionStageDomainV1, LogicalInspectionIdV1,
};
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::auth::{decode_secondary_header, AuthRegistry};
use crate::ipc::{reject_immediate_trailing, DecodedInspectionFrame};
use crate::security_headers::SECURITY_HEADERS;
use crate::store::{EventStore, ResponseLease, RevealRegistry, SensitiveResponseBuffer};
use crate::{
    CanonicalAuthorizationV1, ClientLimits, DashboardError, DashboardErrorCode, DashboardHttp1Gate,
    IpcLimits, LoopbackBind, PairingEnvelopeV1, PairingSecret, RetentionLimits,
    ValidatedDashboardRequestV1,
};

const CHILD_PURGE_REQUEST: u8 = 0x20;
const PARENT_PURGE: u8 = 0x10;
const PARENT_ROTATE: u8 = 0x11;
const PARENT_SHUTDOWN: u8 = 0x12;
const CHILD_PURGED: u8 = 0x90;
const CHILD_STOPPED: u8 = 0x92;

/// Verified no-application-dump readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoDumpReadiness {
    /// Core dump soft and hard limits are both zero and the constant panic hook is installed.
    Verified,
    /// The current platform has no reviewed safe implementation.
    Unsupported,
    /// The operating-system control could not be set or verified.
    Unavailable,
}

impl NoDumpReadiness {
    /// Installs and verifies no-dump readiness before any token or sensitive frame exists.
    #[must_use]
    pub fn install_and_verify() -> Self {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use rustix::process::{getrlimit, setrlimit, Resource, Rlimit};
            let zero = Rlimit {
                current: Some(0),
                maximum: Some(0),
            };
            if setrlimit(Resource::Core, zero).is_err() || getrlimit(Resource::Core) != zero {
                return Self::Unavailable;
            }
            std::panic::set_hook(Box::new(|_| {
                eprintln!("dashboard_child_terminated");
            }));
            Self::Verified
        }
        #[cfg(any(not(unix), target_os = "macos"))]
        {
            Self::Unsupported
        }
    }
}

/// Already-connected child-side local handles.
pub struct ChildInheritedHandles {
    control: UnixStream,
    inspection: UnixStream,
}

impl ChildInheritedHandles {
    /// Connects only the two crate-owned channels supplied by
    /// [`crate::SpawnedDashboardChild::spawn`].
    pub fn connect_from_environment() -> Result<Self, DashboardError> {
        let control_path = std::env::var_os("GAZE_DASHBOARD_CONTROL_SOCKET_V1")
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let inspection_path = std::env::var_os("GAZE_DASHBOARD_INSPECTION_SOCKET_V1")
            .ok_or_else(|| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        std::env::remove_var("GAZE_DASHBOARD_CONTROL_SOCKET_V1");
        std::env::remove_var("GAZE_DASHBOARD_INSPECTION_SOCKET_V1");
        let control = UnixStream::connect(control_path)
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let inspection = UnixStream::connect(inspection_path)
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        Self::new(control, inspection)
    }

    /// Accepts only connected local Unix sockets. Descriptors 0/1/2, terminals, character
    /// devices, and regular files are unrepresentable through this constructor.
    fn new(control: UnixStream, inspection: UnixStream) -> Result<Self, DashboardError> {
        control
            .peer_addr()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        inspection
            .peer_addr()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        Ok(Self {
            control,
            inspection,
        })
    }
}

/// Sensitive child configuration.
#[derive(Clone, Copy)]
pub struct ChildConfig {
    bind: LoopbackBind,
    retention: RetentionLimits,
    clients: ClientLimits,
    ipc: IpcLimits,
}

impl ChildConfig {
    /// Creates a child configuration from bounded types.
    #[must_use]
    pub const fn new(
        bind: LoopbackBind,
        retention: RetentionLimits,
        clients: ClientLimits,
        ipc: IpcLimits,
    ) -> Self {
        Self {
            bind,
            retention,
            clients,
            ipc,
        }
    }
}

/// Sensitive child process entrypoint.
pub struct DashboardChildEntrypoint;

impl DashboardChildEntrypoint {
    /// Runs the sensitive listener/auth/store/response runtime until parent shutdown.
    pub fn run(handles: ChildInheritedHandles, config: ChildConfig) -> Result<(), DashboardError> {
        if NoDumpReadiness::install_and_verify() != NoDumpReadiness::Verified {
            return Err(DashboardError::new(DashboardErrorCode::NoDumpUnavailable));
        }

        let listener = TcpListener::bind(config.bind.socket_addr())
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidLoopbackBind))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;
        let authority = match listener.local_addr() {
            Ok(SocketAddr::V4(authority))
                if authority.ip().is_loopback() && authority.port() != 0 =>
            {
                authority
            }
            _ => return Err(DashboardError::new(DashboardErrorCode::InvalidLoopbackBind)),
        };

        let mut control = handles.control;
        control
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        control
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let secret = PairingSecret::generate()?;
        child_pair(&mut control, authority, &secret)?;

        let state = Arc::new(Mutex::new(ChildState {
            store: EventStore::new(config.retention, InspectionEpochV1::new(0)),
            auth: AuthRegistry::new(&secret, config.clients.page_sessions()),
            reveals: RevealRegistry::new(Duration::from_secs(30)),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let active_responses = Arc::new(AtomicUsize::new(0));

        let inspection_state = state.clone();
        let inspection_stop = stop.clone();
        let inspection_control = control
            .try_clone()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let inspection = handles.inspection;
        let frame_cap = config.ipc.frame_bytes();
        let inspection_thread = thread::Builder::new()
            .name("gaze-dashboard-child-ingress".to_owned())
            .spawn(move || {
                inspection_loop(
                    inspection,
                    inspection_state,
                    inspection_stop,
                    inspection_control,
                    frame_cap,
                );
            })
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;

        let server_state = state.clone();
        let server_stop = stop.clone();
        let server_control = control
            .try_clone()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let limits = ServerLimits {
            responses: config.clients.active_payload_responses(),
            connections: config.clients.active_http_connections(),
            followers: config.clients.followers(),
        };
        let server_active = active_responses.clone();
        let server_thread = thread::Builder::new()
            .name("gaze-dashboard-http".to_owned())
            .spawn(move || {
                server_loop(
                    listener,
                    authority,
                    server_state,
                    server_stop,
                    server_control,
                    server_active,
                    limits,
                );
            })
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;

        let result = child_control_loop(&mut control, authority, &state, &active_responses);
        stop.store(true, Ordering::Release);
        state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .purge_all(InspectionEpochV1::new(u64::MAX));
        let _ = inspection_thread.join();
        let _ = server_thread.join();
        result
    }
}

struct ChildState {
    store: EventStore,
    auth: AuthRegistry,
    reveals: RevealRegistry,
}

impl ChildState {
    fn purge_all(&mut self, epoch: InspectionEpochV1) {
        self.store.begin_epoch(epoch);
        self.auth.purge();
        self.reveals.purge();
    }
}

fn child_pair(
    control: &mut UnixStream,
    authority: SocketAddrV4,
    secret: &PairingSecret,
) -> Result<(), DashboardError> {
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce)
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    PairingEnvelopeV1::encode(nonce, authority, secret)
        .write_to(control)
        .and_then(|()| control.flush())
        .map_err(|_| DashboardError::new(DashboardErrorCode::PairingFailed))?;
    crate::DeliveredAckV1::read_from(control, nonce)?;
    reject_immediate_trailing(control)
}

fn inspection_loop(
    mut stream: UnixStream,
    state: Arc<Mutex<ChildState>>,
    stop: Arc<AtomicBool>,
    control: UnixStream,
    frame_cap: usize,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    while !stop.load(Ordering::Acquire) {
        let mut length = [0_u8; 4];
        match stream.read_exact(&mut length) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => {
                fatal_ingress(&state, &stop, &control);
                return;
            }
        }
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > frame_cap {
            fatal_ingress(&state, &stop, &control);
            return;
        }
        let mut bytes = Zeroizing::new(vec![0_u8; length]);
        if stream.read_exact(bytes.as_mut()).is_err() {
            fatal_ingress(&state, &stop, &control);
            return;
        }
        let Ok(frame) = DecodedInspectionFrame::decode(bytes.as_ref(), frame_cap) else {
            fatal_ingress(&state, &stop, &control);
            return;
        };
        let _ = state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .store
            .admit(frame, Instant::now());
    }
}

fn fatal_ingress(state: &Arc<Mutex<ChildState>>, stop: &AtomicBool, control: &UnixStream) {
    state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .purge_all(InspectionEpochV1::new(u64::MAX));
    stop.store(true, Ordering::Release);
    let _ = control.shutdown(Shutdown::Both);
}

fn child_control_loop(
    control: &mut UnixStream,
    authority: SocketAddrV4,
    state: &Arc<Mutex<ChildState>>,
    active_responses: &AtomicUsize,
) -> Result<(), DashboardError> {
    loop {
        let mut command = [0_u8; 1];
        control
            .read_exact(&mut command)
            .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
        match command[0] {
            PARENT_PURGE => {
                let mut epoch = [0_u8; 8];
                control
                    .read_exact(&mut epoch)
                    .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
                let epoch = InspectionEpochV1::new(u64::from_be_bytes(epoch));
                state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .purge_all(epoch);
                wait_for_responses(active_responses)?;
                let mut ack = [0_u8; 9];
                ack[0] = CHILD_PURGED;
                ack[1..9].copy_from_slice(&epoch.get().to_be_bytes());
                control
                    .write_all(&ack)
                    .and_then(|()| control.flush())
                    .map_err(|_| DashboardError::new(DashboardErrorCode::PurgeFailed))?;
            }
            PARENT_ROTATE => {
                let secret = PairingSecret::generate()?;
                state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .auth
                    .rotate(&secret);
                child_pair(control, authority, &secret)?;
            }
            PARENT_SHUTDOWN => {
                state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .purge_all(InspectionEpochV1::new(u64::MAX));
                wait_for_responses(active_responses)?;
                control
                    .write_all(&[CHILD_STOPPED])
                    .and_then(|()| control.flush())
                    .map_err(|_| DashboardError::new(DashboardErrorCode::FatalDisabled))?;
                return Ok(());
            }
            _ => return Err(DashboardError::new(DashboardErrorCode::FatalDisabled)),
        }
    }
}

fn wait_for_responses(active_responses: &AtomicUsize) -> Result<(), DashboardError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while active_responses.load(Ordering::Acquire) != 0 {
        if Instant::now() >= deadline {
            return Err(DashboardError::new(DashboardErrorCode::PurgeFailed));
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ServerLimits {
    responses: usize,
    connections: usize,
    followers: usize,
}

#[derive(Clone)]
struct ConnectionBudget {
    active_responses: Arc<AtomicUsize>,
    active_followers: Arc<AtomicUsize>,
    max_responses: usize,
    max_followers: usize,
}

fn server_loop(
    listener: TcpListener,
    authority: SocketAddrV4,
    state: Arc<Mutex<ChildState>>,
    stop: Arc<AtomicBool>,
    mut control: UnixStream,
    active_responses: Arc<AtomicUsize>,
    limits: ServerLimits,
) {
    let active_connections = Arc::new(AtomicUsize::new(0));
    let budget = ConnectionBudget {
        active_responses,
        active_followers: Arc::new(AtomicUsize::new(0)),
        max_responses: limits.responses,
        max_followers: limits.followers,
    };
    let mut workers = Vec::new();
    while !stop.load(Ordering::Acquire) {
        if !reap_finished_workers(&mut workers) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !matches!(peer, SocketAddr::V4(value) if value.ip().is_loopback()) {
                    continue;
                }
                if !acquire_slot(&active_connections, limits.connections) {
                    let _ = stream.write_all(crate::CONSTANT_REJECTION_RESPONSE);
                    continue;
                }
                let connection_slot = CountedSlot {
                    active: active_connections.clone(),
                };
                let state = state.clone();
                let budget = budget.clone();
                let control = match control.try_clone() {
                    Ok(control) => control,
                    Err(_) => break,
                };
                match thread::Builder::new()
                    .name("gaze-dashboard-http-connection".to_owned())
                    .spawn(move || {
                        handle_connection(
                            stream,
                            authority,
                            state,
                            control,
                            budget,
                            connection_slot,
                        );
                    }) {
                    Ok(worker) => workers.push(worker),
                    Err(_) => break,
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
    if !stop.swap(true, Ordering::AcqRel) {
        state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .purge_all(InspectionEpochV1::new(u64::MAX));
        let _ = control.shutdown(Shutdown::Both);
    }
    let _ = control.flush();
}

fn reap_finished_workers(workers: &mut Vec<thread::JoinHandle<()>>) -> bool {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            if worker.join().is_err() {
                return false;
            }
        } else {
            index += 1;
        }
    }
    true
}

fn handle_connection(
    mut stream: TcpStream,
    authority: SocketAddrV4,
    state: Arc<Mutex<ChildState>>,
    mut control: UnixStream,
    budget: ConnectionBudget,
    _connection_slot: CountedSlot,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let Ok(request) = read_bounded_request(&mut stream, Instant::now() + Duration::from_secs(2))
    else {
        let _ = stream.write_all(crate::CONSTANT_REJECTION_RESPONSE);
        return;
    };
    let host = authority.to_string();
    let origin = format!("http://{authority}");
    let Ok(validated) =
        DashboardHttp1Gate::validate(request.as_slice(), host.as_bytes(), origin.as_bytes())
    else {
        let _ = stream.write_all(crate::CONSTANT_REJECTION_RESPONSE);
        return;
    };
    match validated.route() {
        ValidatedDashboardRequestV1::Shell => {
            let _ = stream.write_all(&DashboardHttp1Gate::shell_response());
        }
        ValidatedDashboardRequestV1::Stylesheet => {
            let _ = stream.write_all(&DashboardHttp1Gate::stylesheet_response());
        }
        ValidatedDashboardRequestV1::Script => {
            let _ = stream.write_all(&DashboardHttp1Gate::script_response());
        }
        ValidatedDashboardRequestV1::PairSession => {
            let result = validated
                .authorization()
                .ok_or(())
                .and_then(|value| CanonicalAuthorizationV1::parse(value).map_err(|_| ()))
                .and_then(|authorization| {
                    state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .auth
                        .pair(&authorization)
                        .map_err(|_| ())
                });
            match result {
                Ok(response) => {
                    let body = response.encode_for_one_response();
                    let _ = write_binary(&mut stream, 200, body.as_ref());
                }
                Err(()) => {
                    let _ = write_constant(&mut stream, 401, b"authentication failed\n");
                }
            }
        }
        route => {
            let Some((page, csrf, generation)) = authenticate(&validated, &state) else {
                let _ = write_constant(&mut stream, 401, b"authentication failed\n");
                return;
            };
            drop(page);
            drop(csrf);
            match route {
                ValidatedDashboardRequestV1::Snapshot => {
                    let child = state.lock().unwrap_or_else(|poison| poison.into_inner());
                    let body = snapshot_body(&child);
                    let _ = write_json(&mut stream, 200, &body);
                }
                ValidatedDashboardRequestV1::Follow => {
                    if !acquire_slot(&budget.active_followers, budget.max_followers) {
                        let _ = write_constant(&mut stream, 429, b"request rejected\n");
                        return;
                    }
                    let _follower = CountedSlot {
                        active: budget.active_followers,
                    };
                    let child = state.lock().unwrap_or_else(|poison| poison.into_inner());
                    let body = snapshot_body(&child);
                    let _ = write_json(&mut stream, 200, &body);
                }
                ValidatedDashboardRequestV1::Purge => {
                    let _ = control.write_all(&[CHILD_PURGE_REQUEST]);
                    let _ = control.flush();
                    let _ = write_constant(&mut stream, 202, b"purge requested\n");
                }
                ValidatedDashboardRequestV1::ProviderVisible
                | ValidatedDashboardRequestV1::RevealOwnerRaw
                | ValidatedDashboardRequestV1::RevealOwnerRestored => {
                    if acquire_slot(&budget.active_responses, budget.max_responses) {
                        let _guard = ResponseSlot {
                            active: budget.active_responses,
                        };
                        serve_sensitive(&mut stream, route, validated.body(), &state, generation);
                    } else {
                        let _ = write_constant(&mut stream, 429, b"request rejected\n");
                    }
                }
                _ => {
                    let _ = write_constant(&mut stream, 404, b"request rejected\n");
                }
            }
        }
    }
}

type AuthenticatedSession = (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>, u64);

fn authenticate(
    request: &crate::server::ValidatedRequest,
    state: &Arc<Mutex<ChildState>>,
) -> Option<AuthenticatedSession> {
    let page = decode_secondary_header(request.page_session()?)?;
    let csrf = decode_secondary_header(request.csrf()?)?;
    let child = state.lock().unwrap_or_else(|poison| poison.into_inner());
    child
        .auth
        .validate_session(page.as_ref(), csrf.as_ref())
        .then_some((page, csrf, child.auth.generation()))
}

fn serve_sensitive(
    stream: &mut TcpStream,
    route: ValidatedDashboardRequestV1,
    body: &[u8],
    state: &Arc<Mutex<ChildState>>,
    auth_generation: u64,
) {
    let Some((logical, emission, stage)) = parse_exact_selection(body) else {
        let _ = write_constant(stream, 422, b"request rejected\n");
        return;
    };
    let route_matches = matches!(
        (route, stage),
        (
            ValidatedDashboardRequestV1::ProviderVisible,
            InspectionStageDomainV1::ProviderRequest | InspectionStageDomainV1::ProviderResponse
        ) | (
            ValidatedDashboardRequestV1::RevealOwnerRaw,
            InspectionStageDomainV1::OwnerRequest
        ) | (
            ValidatedDashboardRequestV1::RevealOwnerRestored,
            InspectionStageDomainV1::OwnerRestoredResponse
        )
    );
    if !route_matches {
        let _ = write_constant(stream, 403, b"request rejected\n");
        return;
    }
    let now = Instant::now();
    let mut child = state.lock().unwrap_or_else(|poison| poison.into_inner());
    let reveal_deadline = match route {
        ValidatedDashboardRequestV1::ProviderVisible => now + Duration::from_secs(30),
        _ => {
            let epoch = child.store.epoch();
            let Some(_authorized_deadline) =
                child
                    .reveals
                    .authorize(auth_generation, epoch, logical, emission, stage, now)
            else {
                let _ = write_constant(stream, 403, b"request rejected\n");
                return;
            };
            let key = (auth_generation, epoch, logical, emission, stage);
            let Some(deadline) = child.reveals.consume(key, now) else {
                let _ = write_constant(stream, 403, b"request rejected\n");
                return;
            };
            deadline
        }
    };
    let Some(lease) = child.store.lease(
        logical,
        emission,
        stage,
        now,
        reveal_deadline,
        auth_generation,
    ) else {
        let _ = write_constant(stream, 404, b"request rejected\n");
        return;
    };
    let (domain_tag, stage_tag) = response_tags(route, stage);
    let Some(payload) = child.store.encode_payload_for_response(
        &lease,
        auth_generation,
        now,
        domain_tag,
        stage_tag,
    ) else {
        let _ = write_constant(stream, 404, b"request rejected\n");
        return;
    };
    write_leased(stream, &mut child.store, &lease, auth_generation, &payload);
}

fn write_leased(
    stream: &mut TcpStream,
    store: &mut EventStore,
    lease: &ResponseLease,
    auth_generation: u64,
    body: &SensitiveResponseBuffer,
) {
    write_leased_with_clock(stream, store, lease, auth_generation, body, Instant::now);
}

fn write_leased_with_clock<W, N>(
    stream: &mut W,
    store: &mut EventStore,
    lease: &ResponseLease,
    auth_generation: u64,
    body: &SensitiveResponseBuffer,
    mut now: N,
) where
    W: Write,
    N: FnMut() -> Instant,
{
    let valid = store.lease_valid(lease, auth_generation, now());
    if !valid {
        return;
    }
    let header = response_header(200, "application/octet-stream", body.as_slice().len());
    if stream.write_all(header.as_bytes()).is_err() {
        return;
    }
    for chunk in body.as_slice().chunks(1024) {
        let valid = store.lease_valid(lease, auth_generation, now());
        if !valid || stream.write_all(chunk).is_err() {
            return;
        }
    }
}

type ExactSelection = (
    LogicalInspectionIdV1,
    InspectionEmissionIdV1,
    InspectionStageDomainV1,
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExactSelectionWire {
    logical_id: u64,
    emission_id: u64,
    stage: ExactStageWire,
}

#[derive(Deserialize)]
enum ExactStageWire {
    OwnerRequest,
    ProviderRequest,
    ProviderResponse,
    OwnerRestoredResponse,
}

fn parse_exact_selection(body: &[u8]) -> Option<ExactSelection> {
    let wire: ExactSelectionWire = serde_json::from_slice(body).ok()?;
    let logical = LogicalInspectionIdV1::new(wire.logical_id);
    let emission = InspectionEmissionIdV1::new(wire.emission_id);
    let stage = match wire.stage {
        ExactStageWire::OwnerRequest => InspectionStageDomainV1::OwnerRequest,
        ExactStageWire::ProviderRequest => InspectionStageDomainV1::ProviderRequest,
        ExactStageWire::ProviderResponse => InspectionStageDomainV1::ProviderResponse,
        ExactStageWire::OwnerRestoredResponse => InspectionStageDomainV1::OwnerRestoredResponse,
    };
    Some((logical, emission, stage))
}

fn response_tags(route: ValidatedDashboardRequestV1, stage: InspectionStageDomainV1) -> (u8, u8) {
    let domain = match route {
        ValidatedDashboardRequestV1::ProviderVisible => 1,
        ValidatedDashboardRequestV1::RevealOwnerRaw => 2,
        ValidatedDashboardRequestV1::RevealOwnerRestored => 3,
        _ => unreachable!(),
    };
    let stage = match stage {
        InspectionStageDomainV1::OwnerRequest => 1,
        InspectionStageDomainV1::ProviderRequest => 2,
        InspectionStageDomainV1::ProviderResponse => 3,
        InspectionStageDomainV1::OwnerRestoredResponse => 4,
    };
    (domain, stage)
}

fn acquire_slot(active: &AtomicUsize, max: usize) -> bool {
    active
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            (value < max).then_some(value + 1)
        })
        .is_ok()
}

struct ResponseSlot {
    active: Arc<AtomicUsize>,
}

struct CountedSlot {
    active: Arc<AtomicUsize>,
}

impl Drop for CountedSlot {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Drop for ResponseSlot {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn request_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
        return false;
    };
    let head = &request[..header_end];
    let mut declared = None;
    for line in head.split(|byte| *byte == b'\n').skip(1) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if line[..colon].eq_ignore_ascii_case(b"content-length") {
            let value = line[colon + 1..]
                .iter()
                .copied()
                .filter(|byte| !matches!(byte, b' ' | b'\t'))
                .collect::<Vec<_>>();
            declared = std::str::from_utf8(&value)
                .ok()
                .and_then(|value| value.parse::<usize>().ok());
        }
    }
    let expected = header_end
        .checked_add(4)
        .and_then(|value| value.checked_add(declared.unwrap_or(0)));
    expected.is_some_and(|expected| request.len() >= expected)
}

fn read_bounded_request(
    reader: &mut impl Read,
    deadline: Instant,
) -> Result<Zeroizing<Vec<u8>>, ()> {
    let mut request = Zeroizing::new(Vec::with_capacity(1_024));
    let mut chunk = [0_u8; 1_024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => {
                return request_complete(request.as_slice())
                    .then_some(request)
                    .ok_or(())
            }
            Ok(count) => {
                request.extend_from_slice(&chunk[..count]);
                if request.len() > crate::MAX_HTTP_REQUEST_BYTES {
                    return Err(());
                }
                if request_complete(request.as_slice()) {
                    return Ok(request);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(());
                }
            }
            Err(_) => return Err(()),
        }
    }
}

fn snapshot_body(child: &ChildState) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "runtime": {
            "lifecycle": "Running",
            "epoch": child.store.epoch().get(),
            "queueTelemetry": { "state": "Unavailable", "reason": "NotMeasured" }
        },
        "capture": { "tier": "ProviderVisible" },
        "counters": child.store.safe_counters(),
        "events": child.store.safe_snapshot_events()
    }))
    .unwrap_or_else(|_| br#"{"version":1,"runtime":{"lifecycle":"Disabled","disabledCode":"ProjectionFailedClosed"},"events":[]}"#.to_vec())
}

fn write_constant(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    write_response(stream, status, "text/plain; charset=utf-8", body)
}

fn write_json(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    write_response(stream, status, "application/json", body)
}

fn write_binary(stream: &mut TcpStream, status: u16, body: &[u8]) -> std::io::Result<()> {
    write_response(stream, status, "application/octet-stream", body)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = response_header(status, content_type, body.len());
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)
}

fn response_header(status: u16, content_type: &str, length: usize) -> String {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\n{SECURITY_HEADERS}\r\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::DecodedInspectionFrame;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    enum FatalFrame {
        Eof,
        Zero,
        Oversize,
        Partial,
        Decode,
    }

    #[test]
    fn every_inspection_framing_fault_purges_and_stops_the_child() {
        for frame in [
            FatalFrame::Eof,
            FatalFrame::Zero,
            FatalFrame::Oversize,
            FatalFrame::Partial,
            FatalFrame::Decode,
        ] {
            assert_fatal_frame(frame);
        }
    }

    fn assert_fatal_frame(frame: FatalFrame) {
        let secret = PairingSecret::generate().unwrap();
        let limits = RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap();
        let state = Arc::new(Mutex::new(ChildState {
            store: EventStore::new(limits, InspectionEpochV1::new(0)),
            auth: AuthRegistry::new(&secret, 4),
            reveals: RevealRegistry::new(Duration::from_secs(30)),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (inspection, mut sender) = UnixStream::pair().unwrap();
        let (control, mut peer_control) = UnixStream::pair().unwrap();
        let thread_state = state.clone();
        let thread_stop = stop.clone();
        let worker = thread::spawn(move || {
            inspection_loop(inspection, thread_state, thread_stop, control, 4_096);
        });
        match frame {
            FatalFrame::Eof => drop(sender),
            FatalFrame::Zero => sender.write_all(&0_u32.to_be_bytes()).unwrap(),
            FatalFrame::Oversize => sender.write_all(&4_097_u32.to_be_bytes()).unwrap(),
            FatalFrame::Partial => {
                sender.write_all(&10_u32.to_be_bytes()).unwrap();
                sender.write_all(&[0_u8; 2]).unwrap();
                sender.shutdown(Shutdown::Write).unwrap();
            }
            FatalFrame::Decode => {
                sender.write_all(&54_u32.to_be_bytes()).unwrap();
                sender.write_all(&[0_u8; 54]).unwrap();
            }
        }
        let _ = peer_control.set_read_timeout(Some(Duration::from_secs(2)));
        let mut byte = [0_u8; 1];
        let _ = peer_control.read(&mut byte);
        worker.join().unwrap();
        assert!(stop.load(Ordering::Acquire));
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .store
                .epoch(),
            InspectionEpochV1::new(u64::MAX)
        );
    }

    #[test]
    fn ttl_cancellation_stops_before_the_next_application_chunk() {
        let now = Instant::now();
        let (mut store, lease, body) = response_fixture(now, b"<Email_1>".repeat(300));
        let deadline = now + Duration::from_secs(30);
        let mut times = VecDeque::from([now, now, deadline]);
        let mut output = Vec::new();
        write_leased_with_clock(&mut output, &mut store, &lease, 0, &body, || {
            times.pop_front().unwrap_or(deadline)
        });
        let header_end = output
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        assert_eq!(output.len() - header_end, 1_024);
        assert!(body.as_slice().len() > 1_024);
    }

    #[test]
    fn purge_cannot_linearize_while_an_application_write_permit_is_held() {
        let now = Instant::now();
        let secret = PairingSecret::generate().unwrap();
        let (store, lease, body) = response_fixture(now, b"<Email_1>".repeat(300));
        let state = Arc::new(Mutex::new(ChildState {
            store,
            auth: AuthRegistry::new(&secret, 4),
            reveals: RevealRegistry::new(Duration::from_secs(30)),
        }));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let writer_state = state.clone();
        let writer = thread::spawn(move || {
            let mut child = writer_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mut output = BlockingWriter {
                calls: 0,
                bytes: Vec::new(),
                entered: entered_tx,
                release: release_rx,
            };
            write_leased_with_clock(&mut output, &mut child.store, &lease, 0, &body, || now);
            output.bytes.len()
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let purge_state = state.clone();
        let (attempt_tx, attempt_rx) = mpsc::sync_channel(1);
        let (purged_tx, purged_rx) = mpsc::sync_channel(1);
        let purger = thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            purge_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .purge_all(InspectionEpochV1::new(1));
            purged_tx.send(()).unwrap();
        });
        attempt_rx.recv().unwrap();
        assert!(purged_rx.recv_timeout(Duration::from_millis(20)).is_err());
        release_tx.send(()).unwrap();
        assert!(writer.join().unwrap() > 1_024);
        purged_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        purger.join().unwrap();
        assert_eq!(
            state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .store
                .epoch(),
            InspectionEpochV1::new(1)
        );
    }

    #[test]
    fn connection_follower_and_payload_slots_enforce_exact_caps() {
        for cap in [4, 4, 2] {
            let active = Arc::new(AtomicUsize::new(0));
            let mut guards = Vec::new();
            for _ in 0..cap {
                assert!(acquire_slot(&active, cap));
                guards.push(CountedSlot {
                    active: active.clone(),
                });
            }
            assert!(!acquire_slot(&active, cap));
            drop(guards.pop());
            assert!(acquire_slot(&active, cap));
            active.fetch_sub(1, Ordering::AcqRel);
            drop(guards);
            assert_eq!(active.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn fragmented_requests_complete_exactly_and_slow_or_partial_clients_fail_closed() {
        let chunks = VecDeque::from([
            b"GET / HTTP/1.1\r\nHo".to_vec(),
            b"st: 127.0.0.1:43123\r\n".to_vec(),
            b"\r\n".to_vec(),
        ]);
        let mut fragmented = FragmentedReader { chunks };
        let request =
            read_bounded_request(&mut fragmented, Instant::now() + Duration::from_secs(1)).unwrap();
        assert_eq!(
            request.as_slice(),
            b"GET / HTTP/1.1\r\nHost: 127.0.0.1:43123\r\n\r\n"
        );

        let mut partial = std::io::Cursor::new(b"GET / HTTP/1.1\r\nHost:".to_vec());
        assert!(read_bounded_request(&mut partial, Instant::now()).is_err());
        let mut slow = TimeoutReader;
        assert!(read_bounded_request(&mut slow, Instant::now()).is_err());
        let mut oversize = std::io::Cursor::new(vec![b'x'; crate::MAX_HTTP_REQUEST_BYTES + 1]);
        assert!(
            read_bounded_request(&mut oversize, Instant::now() + Duration::from_secs(1)).is_err()
        );
    }

    fn response_fixture(
        now: Instant,
        payload: Vec<u8>,
    ) -> (EventStore, ResponseLease, SensitiveResponseBuffer) {
        let limits = RetentionLimits::new(4, 64 * 1024, Duration::from_secs(60)).unwrap();
        let mut store = EventStore::new(limits, InspectionEpochV1::new(0));
        let stage = InspectionStageDomainV1::ProviderResponse;
        let frame = DecodedInspectionFrame {
            logical_id: LogicalInspectionIdV1::new(1),
            emission_id: InspectionEmissionIdV1::new(1),
            sequence: gaze_types::inspection::InspectionSequenceV1::new(0),
            epoch: InspectionEpochV1::new(0),
            stage_domain: stage,
            safe_meta: br#"{"version":1}"#.to_vec(),
            projection: br#"{"measurement":{"state":"Omitted","reason":"NotApplicable"}}"#.to_vec(),
            payload: Zeroizing::new(payload),
        };
        assert!(matches!(
            store.admit(frame, now),
            crate::store::AdmissionOutcome::Inserted
        ));
        let deadline = now + Duration::from_secs(30);
        let lease = store
            .lease(
                LogicalInspectionIdV1::new(1),
                InspectionEmissionIdV1::new(1),
                stage,
                now,
                deadline,
                0,
            )
            .unwrap();
        let body = store
            .encode_payload_for_response(&lease, 0, now, 1, 3)
            .unwrap();
        (store, lease, body)
    }

    struct BlockingWriter {
        calls: usize,
        bytes: Vec<u8>,
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }

    struct FragmentedReader {
        chunks: VecDeque<Vec<u8>>,
    }

    impl Read for FragmentedReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            let Some(chunk) = self.chunks.pop_front() else {
                return Ok(0);
            };
            output[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }

    struct TimeoutReader;

    impl Read for TimeoutReader {
        fn read(&mut self, _output: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "synthetic timeout",
            ))
        }
    }

    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.calls == 2 {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
