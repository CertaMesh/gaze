#![cfg(unix)]
#![cfg_attr(target_os = "macos", allow(dead_code, unused_imports))]

use std::io::{self, Read, Write};
use std::net::{SocketAddrV4, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use gaze_inspection::{
    install_inspection_v1, InspectionBeginLogicalErrorV1, PendingInspectionProducerV1,
};
use gaze_proxy_dashboard::{
    ChildConfig, ChildInheritedHandles, ClientLimits, DashboardChildEntrypoint, DashboardControl,
    DashboardLifecycle, DashboardPayloadAcceptance, DashboardStartupConfig, DashboardSupervisor,
    IpcLimits, LoopbackBind, PairedDashboard, RetentionLimits, SpawnedDashboardChild,
};

// Subprocess-spawning tests share Unix-domain and TCP sockets and multiple
// threads with a short-lived child process.  Running them in parallel can
// cause the child's control socket to be torn down before the parent sends its
// purge command, producing a spurious PurgeFailed.  Serialising them is safe:
// they are inherently process-level tests, not unit tests.
#[cfg(not(target_os = "macos"))]
static SUBPROCESS_SERIAL: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "subprocess helper only"]
fn dashboard_child_helper() {
    let Some(_) = std::env::var_os("GAZE_DASHBOARD_CONTROL_SOCKET_V1") else {
        return;
    };
    if let Some(pid_file) = std::env::var_os("GAZE_TEST_CHILD_PID_FILE") {
        std::fs::write(pid_file, std::process::id().to_string()).unwrap();
    }
    let handles = ChildInheritedHandles::connect_from_environment().unwrap();
    let config = ChildConfig::new(
        LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
        RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap(),
        ClientLimits::conservative(),
        IpcLimits::new(4, 64 * 1024).unwrap(),
    );
    DashboardChildEntrypoint::run(handles, config).unwrap();
}

#[cfg(not(target_os = "macos"))]
fn spawn_paired_dashboard(pid_file: &Path) -> (PairedDashboard, u32, Vec<u8>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "dashboard_child_helper",
            "--nocapture",
        ])
        .env("GAZE_TEST_CHILD_PID_FILE", pid_file);
    let spawned = SpawnedDashboardChild::spawn(command).unwrap();
    let pid = std::fs::read_to_string(pid_file)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let acceptance = DashboardPayloadAcceptance::provider_visible();
    let config = DashboardStartupConfig::Enabled {
        acceptance,
        bind: LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
        retention: RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap(),
        clients: ClientLimits::conservative(),
        ipc: IpcLimits::new(4, 64 * 1024).unwrap(),
    };
    let (token_tx, token_rx) = std::sync::mpsc::channel();
    let paired = DashboardSupervisor::prepare(config, spawned, move |_authority, token: &[u8]| {
        assert_eq!(token.len(), 43);
        let _ = token_tx.send(token.to_vec());
        Ok::<(), io::Error>(())
    })
    .unwrap();
    let token = token_rx.recv().expect("pairing token delivered");
    (paired, pid, token)
}

#[cfg(not(target_os = "macos"))]
fn assert_process_reaped(pid: u32) {
    let child = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .output()
        .expect("child liveness check");
    assert!(
        !child.status.success(),
        "acknowledged shutdown did not reap child {pid}"
    );
}

#[cfg(not(target_os = "macos"))]
fn runtime_diagnostic(control: &DashboardControl, pid: u32) -> String {
    let child = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success());
    format!(
        "lifecycle={:?}, status={:?}, child_alive={child:?}",
        control.lifecycle(),
        control.status()
    )
}

#[test]
#[cfg(not(target_os = "macos"))]
fn matched_activation_owns_serialized_purge_shutdown_and_child_reap() {
    let _guard = SUBPROCESS_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, _token) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();

    control.purge().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(1));
    let logical = producer.begin_logical_blocking();
    assert!(
        logical.is_ok(),
        "begin_logical={:?}; {}",
        logical.as_ref().err(),
        runtime_diagnostic(&control, pid)
    );
    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    assert!(matches!(
        producer.begin_logical_blocking(),
        Err(InspectionBeginLogicalErrorV1::Disabled)
    ));
    drop(launch);
    assert_process_reaped(pid);
}

#[cfg(not(target_os = "macos"))]
fn spawn_paired_dashboard_with_authority(
    pid_file: &Path,
) -> (PairedDashboard, u32, std::net::SocketAddrV4) {
    use std::sync::{Arc, Mutex};
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "dashboard_child_helper",
            "--nocapture",
        ])
        .env("GAZE_TEST_CHILD_PID_FILE", pid_file);
    let spawned = SpawnedDashboardChild::spawn(command).unwrap();
    let pid = std::fs::read_to_string(pid_file)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let acceptance = DashboardPayloadAcceptance::provider_visible();
    let config = DashboardStartupConfig::Enabled {
        acceptance,
        bind: LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
        retention: RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap(),
        clients: ClientLimits::conservative(),
        ipc: IpcLimits::new(4, 64 * 1024).unwrap(),
    };
    let authority_cell = Arc::new(Mutex::new(None::<std::net::SocketAddrV4>));
    let authority_capture = authority_cell.clone();
    let paired = DashboardSupervisor::prepare(config, spawned, move |authority, token: &[u8]| {
        assert_eq!(token.len(), 43);
        *authority_capture.lock().unwrap() = Some(authority);
        Ok::<(), io::Error>(())
    })
    .unwrap();
    let authority = authority_cell.lock().unwrap().unwrap();
    (paired, pid, authority)
}

#[test]
#[cfg(not(target_os = "macos"))]
fn realistic_chrome_top_level_navigation_reaches_shell_over_raw_socket() {
    let _guard = SUBPROCESS_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, authority) =
        spawn_paired_dashboard_with_authority(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (_producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();

    let host = format!("{}:{}", authority.ip(), authority.port());
    let origin = format!("http://{host}");
    let navigation = format!(
        "GET / HTTP/1.1\r\n\
         Host: {host}\r\n\
         Upgrade-Insecure-Requests: 1\r\n\
         User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\r\n\
         Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
         Sec-Fetch-Site: none\r\n\
         Sec-Fetch-Mode: navigate\r\n\
         Sec-Fetch-User: ?1\r\n\
         Sec-Fetch-Dest: document\r\n\
         Accept-Encoding: gzip, deflate\r\n\
         Accept-Language: en-US,en;q=0.9\r\n\
         sec-ch-ua: \"Not/A)Brand\";v=\"8\", \"Chromium\";v=\"126\", \"Google Chrome\";v=\"126\"\r\n\
         sec-ch-ua-mobile: ?0\r\n\
         sec-ch-ua-platform: \"Linux\"\r\n\
         \r\n"
    );
    let mut stream = TcpStream::connect(std::net::SocketAddr::V4(authority)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    stream.write_all(navigation.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut response = Vec::with_capacity(4 * 1024);
    stream.read_to_end(&mut response).unwrap();
    let head = std::str::from_utf8(&response[..response.len().min(64)]).unwrap_or("");
    assert!(
        head.contains("HTTP/1.1 200 OK"),
        "realistic browser navigation must serve the shell (200), got head={head:?}"
    );
    assert!(
        std::str::from_utf8(&response)
            .unwrap_or("")
            .contains("<!doctype html"),
        "response body must be the dashboard HTML shell"
    );
    let _ = origin; // origin not needed for the GET shell route; silence unused warning.

    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    drop(launch);
    assert_process_reaped(pid);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn descriptor_equal_double_swap_fails_closed_disables_producers_and_reaps_children() {
    let _guard = SUBPROCESS_SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let temp_a = tempfile::tempdir().unwrap();
    let temp_b = tempfile::tempdir().unwrap();
    let (paired_a, pid_a, _token_a) = spawn_paired_dashboard(&temp_a.path().join("child.pid"));
    let (paired_b, pid_b, _token_b) = spawn_paired_dashboard(&temp_b.path().join("child.pid"));
    let (pending_a, consumer_a, descriptor_a) = paired_a.into_pending_activation().unwrap();
    let (pending_b, consumer_b, descriptor_b) = paired_b.into_pending_activation().unwrap();
    assert_eq!(descriptor_a, descriptor_b);
    let (producer_a, activated_a) =
        install_inspection_v1(PendingInspectionProducerV1::new(descriptor_a), consumer_a).unwrap();
    let (producer_b, activated_b) =
        install_inspection_v1(PendingInspectionProducerV1::new(descriptor_b), consumer_b).unwrap();

    let Err(error_a) = pending_a.commit(activated_b) else {
        panic!("descriptor-equal swapped activation A unexpectedly succeeded");
    };
    let Err(error_b) = pending_b.commit(activated_a) else {
        panic!("descriptor-equal swapped activation B unexpectedly succeeded");
    };
    assert_eq!(
        error_a.code(),
        gaze_proxy_dashboard::DashboardErrorCode::ActivationFailed
    );
    assert_eq!(
        error_b.code(),
        gaze_proxy_dashboard::DashboardErrorCode::ActivationFailed
    );
    assert!(matches!(
        producer_a.begin_logical(),
        Err(InspectionBeginLogicalErrorV1::Disabled)
    ));
    assert!(matches!(
        producer_b.begin_logical(),
        Err(InspectionBeginLogicalErrorV1::Disabled)
    ));
    assert_process_reaped(pid_a);
    assert_process_reaped(pid_b);
}

#[cfg(not(target_os = "macos"))]
fn http_response_body(full: &[u8]) -> Option<Vec<u8>> {
    let split = full.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_end = split + 4;
    let len = http_content_length(&full[..split])?;
    let body = full.get(header_end..header_end + len)?;
    Some(body.to_vec())
}

#[cfg(not(target_os = "macos"))]
fn http_content_length(headers: &[u8]) -> Option<usize> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let name = &line[..colon];
        if name.eq_ignore_ascii_case(b"content-length") {
            let value: Vec<u8> = line[colon + 1..]
                .iter()
                .filter(|b| !matches!(b, b' ' | b'\t'))
                .copied()
                .collect();
            return std::str::from_utf8(&value).ok()?.parse().ok();
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn http_round_trip(authority: SocketAddrV4, request: &[u8]) -> io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(authority)?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    stream.write_all(request)?;
    stream.flush()?;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if http_response_body(&response).is_some() {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&chunk[..n]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if http_response_body(&response).is_some() {
                    break;
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(response)
}

/// Performs the one-time launch-credential pair-session handshake and returns the
/// base64url-encoded page-session and CSRF tokens needed by authenticated routes.
#[cfg(not(target_os = "macos"))]
fn authenticated_session(authority: SocketAddrV4, token: &[u8]) -> (String, String) {
    let auth = std::str::from_utf8(token).expect("pairing token is base64url ASCII");
    let request = format!(
        "POST /api/v1/session/pair HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nAuthorization: GazeDashboardV1 {auth}\r\nContent-Type: application/gaze-dashboard-pair-v1\r\nContent-Length: 12\r\n\r\nGZDB-PAIR-V1"
    );
    let response =
        http_round_trip(authority, request.as_bytes()).expect("pair-session HTTP round trip");
    assert!(
        response.starts_with(b"HTTP/1.1 200 "),
        "pair-session rejected: {}",
        String::from_utf8_lossy(&response)
    );
    let body = http_response_body(&response).expect("pair-session body present");
    assert_eq!(body.len(), 70, "exact 70-byte bootstrap envelope");
    assert_eq!(&body[0..4], b"GZDB");
    assert_eq!(body[4], 1);
    assert_eq!(body[5], 2);
    let page_session = URL_SAFE_NO_PAD.encode(&body[6..38]);
    let csrf = URL_SAFE_NO_PAD.encode(&body[38..70]);
    (page_session, csrf)
}

/// Issues an authenticated in-browser `/purge` request (the unsolicited `0x20`
/// notification path). Returns true only when the child accepted it with a 202.
#[cfg(not(target_os = "macos"))]
fn try_browser_purge(authority: SocketAddrV4, page_b64: &str, csrf_b64: &str) -> bool {
    let mut request = format!(
        "POST /api/v1/purge HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nX-Gaze-Page-Session: {page_b64}\r\nX-Gaze-Csrf: {csrf_b64}\r\n\r\n"
    )
    .into_bytes();
    request.extend_from_slice(b"{}");
    match http_round_trip(authority, &request) {
        Ok(response) => response.starts_with(b"HTTP/1.1 202 "),
        Err(_) => false,
    }
}

#[cfg(not(target_os = "macos"))]
struct BrowserPurgeWorkers {
    stop: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<usize>>,
}

#[cfg(not(target_os = "macos"))]
impl BrowserPurgeWorkers {
    fn start(authority: SocketAddrV4, page: &str, csrf: &str, attempts: usize) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let mut handles = Vec::new();
        let mut starts = Vec::new();
        for _ in 0..3 {
            let stop = stop.clone();
            let (start_tx, start_rx) = mpsc::channel();
            starts.push(start_tx);
            let ready_tx = ready_tx.clone();
            let page = page.to_owned();
            let csrf = csrf.to_owned();
            handles.push(thread::spawn(move || {
                let mut accepted = usize::from(try_browser_purge(authority, &page, &csrf));
                ready_tx.send(()).unwrap();
                if start_rx.recv().is_err() {
                    return accepted;
                }
                for _ in 1..attempts {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    accepted += usize::from(try_browser_purge(authority, &page, &csrf));
                }
                accepted
            }));
        }
        for _ in 0..3 {
            ready_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("browser worker acknowledged its first request");
        }
        for start in starts {
            start.send(()).unwrap();
        }
        Self { stop, handles }
    }

    fn finish(&mut self) -> usize {
        self.stop.store(true, Ordering::Release);
        self.handles
            .drain(..)
            .map(|handle| handle.join().expect("browser worker must not panic"))
            .sum()
    }
}

#[cfg(not(target_os = "macos"))]
impl Drop for BrowserPurgeWorkers {
    fn drop(&mut self) {
        self.finish();
    }
}

/// A single browser-initiated `/purge` must arrive on the dedicated `0x20` channel,
/// drive exactly one serialized purge (advancing the epoch), and leave the control
/// protocol intact for a subsequent operator purge and shutdown.
#[test]
#[cfg(not(target_os = "macos"))]
fn browser_purge_request_advances_epoch_without_corrupting_control_protocol() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, token) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();
    let authority = launch.authority();
    let (page_b64, csrf_b64) = authenticated_session(authority, &token);

    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(0));
    assert!(
        try_browser_purge(authority, &page_b64, &csrf_b64),
        "browser purge request rejected"
    );

    let epoch = control.wait_for_epoch(1).unwrap_or_else(|error| {
        panic!(
            "browser purge acknowledgement failed: {error:?}; {}",
            runtime_diagnostic(&control, pid)
        )
    });
    assert_eq!(epoch, 1);

    control
        .purge()
        .expect("operator purge must succeed after a browser purge");
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(2));
    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    drop(launch);
    assert_process_reaped(pid);
    drop(producer);
}

/// Concurrent browser-initiated purges (unsolicited `0x20` notifications from
/// per-connection HTTP workers) must not interleave with the structured
/// command/ack protocol that operator-issued `control.purge()` exchanges. Before
/// the fix the notifications shared the control FD and corrupted the ack reads,
/// spuriously disabling the dashboard.
#[test]
#[cfg(not(target_os = "macos"))]
fn concurrent_browser_and_operator_purges_do_not_disable_dashboard() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, token) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();
    let authority = launch.authority();
    let (page_b64, csrf_b64) = authenticated_session(authority, &token);

    let mut browser = BrowserPurgeWorkers::start(authority, &page_b64, &csrf_b64, 25);

    for minimum in 1..=10 {
        control.purge().unwrap_or_else(|error| {
            panic!(
                "operator purge failed: {error:?}; {}",
                runtime_diagnostic(&control, pid)
            )
        });
        control.wait_for_epoch(minimum).unwrap_or_else(|error| {
            panic!(
                "concurrent purge acknowledgement failed: {error:?}; {}",
                runtime_diagnostic(&control, pid)
            )
        });
    }

    let total_browser_purges = browser.finish();
    assert!(
        total_browser_purges > 0,
        "concurrent test exercised no browser purges"
    );
    let minimum = total_browser_purges as u64 + 10;
    let acknowledged = control.wait_for_epoch(minimum).unwrap_or_else(|error| {
        panic!(
            "final purge acknowledgement failed: {error:?}; {}",
            runtime_diagnostic(&control, pid)
        )
    });
    assert_eq!(
        acknowledged, minimum,
        "all accepted purges advance exactly one epoch"
    );
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(minimum));

    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    drop(launch);
    assert_process_reaped(pid);
    drop(producer);
}

/// `rotate_pairing_secret` exchanges a 60-byte `PairingEnvelopeV2` (preceded by a
/// full purge) on the control socket while browser-initiated `0x20` notifications
/// are in flight. Before the fix the notifications shared the control FD and
/// shifted the envelope magic, producing a spurious `PairingFailed` disable.
#[test]
#[cfg(not(target_os = "macos"))]
fn concurrent_browser_purges_do_not_corrupt_rotate_pairing() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, token) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();
    let authority = launch.authority();
    let (page_b64, csrf_b64) = authenticated_session(authority, &token);

    let mut browser = BrowserPurgeWorkers::start(authority, &page_b64, &csrf_b64, 20);

    control
        .rotate_pairing_secret(Box::new(
            |_authority, _token: &[u8]| Ok::<(), io::Error>(()),
        ))
        .unwrap_or_else(|error| {
            panic!(
                "rotate pairing failed: {error:?}; {}",
                runtime_diagnostic(&control, pid)
            )
        });

    let total_browser_purges = browser.finish();
    assert!(
        total_browser_purges > 0,
        "concurrent test exercised no browser purges"
    );
    let minimum = total_browser_purges as u64 + 1;
    let acknowledged = control.wait_for_epoch(minimum).unwrap_or_else(|error| {
        panic!(
            "final purge acknowledgement failed: {error:?}; {}",
            runtime_diagnostic(&control, pid)
        )
    });
    assert_eq!(
        acknowledged, minimum,
        "all accepted purges advance exactly one epoch"
    );
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(minimum));

    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    drop(launch);
    assert_process_reaped(pid);
    drop(producer);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn rotate_immediately_followed_by_purge_and_shutdown_keeps_control_frames_intact() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, _) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let (producer, activated) =
        install_inspection_v1(PendingInspectionProducerV1::new(descriptor), consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();
    control
        .rotate_pairing_secret(Box::new(|_, _: &[u8]| Ok(())))
        .unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(1));
    control.purge().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(2));
    let logical = producer.begin_logical_blocking();
    assert!(
        logical.is_ok(),
        "begin_logical={:?}; {}",
        logical.as_ref().err(),
        runtime_diagnostic(&control, pid)
    );
    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    drop(launch);
    assert_process_reaped(pid);
}

#[test]
#[cfg(not(target_os = "macos"))]
fn failed_rotation_delivery_disables_registration_and_reaps_child() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid, _) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let (producer, activated) =
        install_inspection_v1(PendingInspectionProducerV1::new(descriptor), consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();
    let error = control
        .rotate_pairing_secret(Box::new(|_, _: &[u8]| {
            Err(io::Error::other("synthetic delivery failure"))
        }))
        .unwrap_err();
    assert_eq!(
        error.code(),
        gaze_proxy_dashboard::DashboardErrorCode::PairingFailed
    );
    // Drop joins the runtime's existing disable/reap path before observing it.
    drop(launch);
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    assert!(matches!(
        producer.begin_logical_blocking(),
        Err(InspectionBeginLogicalErrorV1::Disabled)
    ));
    assert_process_reaped(pid);
}
