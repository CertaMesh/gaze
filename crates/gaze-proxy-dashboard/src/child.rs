use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gaze_types::inspection::{
    InspectionEmissionIdV1, InspectionEpochV1, InspectionStageDomainV1, LogicalInspectionIdV1,
};
use zeroize::Zeroizing;

use crate::auth::{decode_secondary_header, AuthRegistry};
use crate::ipc::DecodedInspectionFrame;
use crate::security_headers::SECURITY_HEADERS;
use crate::store::{EventStore, ResponseLease, RevealRegistry};
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
        #[cfg(unix)]
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
        #[cfg(not(unix))]
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
    /// Accepts only connected local Unix sockets. Descriptors 0/1/2, terminals, character
    /// devices, and regular files are unrepresentable through this constructor.
    pub fn new(control: UnixStream, inspection: UnixStream) -> Result<Self, DashboardError> {
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
        let inspection = handles.inspection;
        let frame_cap = config.ipc.frame_bytes();
        let inspection_thread = thread::Builder::new()
            .name("gaze-dashboard-child-ingress".to_owned())
            .spawn(move || {
                inspection_loop(inspection, inspection_state, inspection_stop, frame_cap);
            })
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;

        let server_state = state.clone();
        let server_stop = stop.clone();
        let server_control = control
            .try_clone()
            .map_err(|_| DashboardError::new(DashboardErrorCode::InvalidInheritedHandle))?;
        let max_responses = config.clients.active_payload_responses();
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
                    max_responses,
                );
            })
            .map_err(|_| DashboardError::new(DashboardErrorCode::ActivationFailed))?;

        let result = child_control_loop(&mut control, authority, &state);
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
    crate::DeliveredAckV1::read_from(control, nonce).map(|_| ())
}

fn inspection_loop(
    mut stream: UnixStream,
    state: Arc<Mutex<ChildState>>,
    stop: Arc<AtomicBool>,
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
            Err(_) => break,
        }
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > frame_cap {
            break;
        }
        let mut bytes = Zeroizing::new(vec![0_u8; length]);
        if stream.read_exact(bytes.as_mut()).is_err() {
            break;
        }
        let Ok(frame) = DecodedInspectionFrame::decode(bytes.as_ref(), frame_cap) else {
            break;
        };
        let _ = state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .store
            .admit(frame, Instant::now());
    }
}

fn child_control_loop(
    control: &mut UnixStream,
    authority: SocketAddrV4,
    state: &Arc<Mutex<ChildState>>,
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

fn server_loop(
    listener: TcpListener,
    authority: SocketAddrV4,
    state: Arc<Mutex<ChildState>>,
    stop: Arc<AtomicBool>,
    mut control: UnixStream,
    active_responses: Arc<AtomicUsize>,
    max_responses: usize,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !matches!(peer, SocketAddr::V4(value) if value.ip().is_loopback()) {
                    continue;
                }
                let state = state.clone();
                let active_responses = active_responses.clone();
                let control = match control.try_clone() {
                    Ok(control) => control,
                    Err(_) => break,
                };
                let _ = thread::Builder::new()
                    .name("gaze-dashboard-http-connection".to_owned())
                    .spawn(move || {
                        handle_connection(
                            stream,
                            authority,
                            state,
                            control,
                            active_responses,
                            max_responses,
                        );
                    });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    let _ = control.flush();
}

fn handle_connection(
    mut stream: TcpStream,
    authority: SocketAddrV4,
    state: Arc<Mutex<ChildState>>,
    mut control: UnixStream,
    active_responses: Arc<AtomicUsize>,
    max_responses: usize,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                request.extend_from_slice(&chunk[..count]);
                if request.len() > crate::MAX_HTTP_REQUEST_BYTES {
                    let _ = stream.write_all(crate::CONSTANT_REJECTION_RESPONSE);
                    return;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => return,
        }
    }
    let host = authority.to_string();
    let origin = format!("http://{authority}");
    let Ok(validated) = DashboardHttp1Gate::validate(&request, host.as_bytes(), origin.as_bytes())
    else {
        let _ = stream.write_all(crate::CONSTANT_REJECTION_RESPONSE);
        return;
    };
    match validated.route() {
        ValidatedDashboardRequestV1::Shell => {
            let _ = stream.write_all(&DashboardHttp1Gate::shell_response());
        }
        ValidatedDashboardRequestV1::Stylesheet | ValidatedDashboardRequestV1::Script => {
            let _ = write_constant(&mut stream, 404, b"unavailable\n");
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
                ValidatedDashboardRequestV1::Snapshot | ValidatedDashboardRequestV1::Follow => {
                    let child = state.lock().unwrap_or_else(|poison| poison.into_inner());
                    let body = format!(
                        "{{\"version\":1,\"events\":{},\"retained_bytes\":{},\"queue_telemetry\":\"unavailable_not_measured\"}}",
                        child.store.logical_len(),
                        child.store.retained_bytes()
                    );
                    let _ = write_json(&mut stream, 200, body.as_bytes());
                }
                ValidatedDashboardRequestV1::Purge => {
                    let _ = control.write_all(&[CHILD_PURGE_REQUEST]);
                    let _ = control.flush();
                    let _ = write_constant(&mut stream, 202, b"purge requested\n");
                }
                ValidatedDashboardRequestV1::ProviderVisible
                | ValidatedDashboardRequestV1::RevealOwnerRaw
                | ValidatedDashboardRequestV1::RevealOwnerRestored => {
                    if acquire_response_slot(&active_responses, max_responses) {
                        let _guard = ResponseSlot {
                            active: active_responses,
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
    let auth = &state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .auth;
    auth.validate_session(page.as_ref(), csrf.as_ref())
        .then_some((page, csrf, auth.generation()))
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
    let Some(payload) = child
        .store
        .with_payload(&lease, auth_generation, now, |bytes| {
            Zeroizing::new(bytes.to_vec())
        })
    else {
        let _ = write_constant(stream, 404, b"request rejected\n");
        return;
    };
    drop(child);
    let encoded = match route {
        ValidatedDashboardRequestV1::ProviderVisible => encode_provider(payload.as_ref()),
        ValidatedDashboardRequestV1::RevealOwnerRaw => encode_owner_raw(payload.as_ref()),
        ValidatedDashboardRequestV1::RevealOwnerRestored => encode_owner_restored(payload.as_ref()),
        _ => unreachable!(),
    };
    write_leased(stream, state, &lease, auth_generation, encoded);
}

fn write_leased(
    stream: &mut TcpStream,
    state: &Arc<Mutex<ChildState>>,
    lease: &ResponseLease,
    auth_generation: u64,
    body: Zeroizing<Vec<u8>>,
) {
    let valid = state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .store
        .lease_valid(lease, auth_generation, Instant::now());
    if !valid {
        return;
    }
    let header = response_header(200, "application/octet-stream", body.len());
    if stream.write_all(header.as_bytes()).is_err() {
        return;
    }
    for chunk in body.chunks(1024) {
        let valid = state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .store
            .lease_valid(lease, auth_generation, Instant::now());
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

fn parse_exact_selection(body: &[u8]) -> Option<ExactSelection> {
    if body.len() != 17 {
        return None;
    }
    let logical = LogicalInspectionIdV1::new(u64::from_be_bytes(body[0..8].try_into().ok()?));
    let emission = InspectionEmissionIdV1::new(u64::from_be_bytes(body[8..16].try_into().ok()?));
    let stage = match body[16] {
        1 => InspectionStageDomainV1::OwnerRequest,
        2 => InspectionStageDomainV1::ProviderRequest,
        3 => InspectionStageDomainV1::ProviderResponse,
        4 => InspectionStageDomainV1::OwnerRestoredResponse,
        _ => return None,
    };
    Some((logical, emission, stage))
}

fn encode_provider(payload: &[u8]) -> Zeroizing<Vec<u8>> {
    encode_sensitive(1, payload)
}

fn encode_owner_raw(payload: &[u8]) -> Zeroizing<Vec<u8>> {
    encode_sensitive(2, payload)
}

fn encode_owner_restored(payload: &[u8]) -> Zeroizing<Vec<u8>> {
    encode_sensitive(3, payload)
}

fn encode_sensitive(domain: u8, payload: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut encoded = Zeroizing::new(Vec::with_capacity(6 + payload.len()));
    encoded.push(1);
    encoded.push(domain);
    encoded.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoded.extend_from_slice(payload);
    encoded
}

fn acquire_response_slot(active: &AtomicUsize, max: usize) -> bool {
    active
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            (value < max).then_some(value + 1)
        })
        .is_ok()
}

struct ResponseSlot {
    active: Arc<AtomicUsize>,
}

impl Drop for ResponseSlot {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
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
