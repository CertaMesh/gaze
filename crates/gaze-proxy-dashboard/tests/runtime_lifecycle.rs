#![cfg(unix)]
#![cfg_attr(target_os = "macos", allow(dead_code, unused_imports))]

use std::io;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gaze_inspection::{
    install_inspection_v1, InspectionBeginLogicalErrorV1, PendingInspectionProducerV1,
};
use gaze_proxy_dashboard::{
    ChildConfig, ChildInheritedHandles, ClientLimits, DashboardChildEntrypoint, DashboardLifecycle,
    DashboardPayloadAcceptance, DashboardStartupConfig, DashboardSupervisor, IpcLimits,
    LoopbackBind, PairedDashboard, RetentionLimits, SpawnedDashboardChild,
};

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
fn spawn_paired_dashboard(pid_file: &Path) -> (PairedDashboard, u32) {
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
    let paired = DashboardSupervisor::prepare(config, spawned, |_authority, token: &[u8]| {
        assert_eq!(token.len(), 43);
        Ok::<(), io::Error>(())
    })
    .unwrap();
    (paired, pid)
}

#[cfg(not(target_os = "macos"))]
fn assert_process_reaped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let alive = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            return;
        }
        assert!(Instant::now() < deadline, "dashboard child was not reaped");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[cfg(not(target_os = "macos"))]
fn matched_activation_owns_serialized_purge_shutdown_and_child_reap() {
    let temp = tempfile::tempdir().unwrap();
    let (paired, pid) = spawn_paired_dashboard(&temp.path().join("child.pid"));
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();

    control.purge().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(1));
    assert!(producer.begin_logical().is_ok());
    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
    assert!(matches!(
        producer.begin_logical(),
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
        .set_read_timeout(Some(Duration::from_secs(5)))
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
    let temp_a = tempfile::tempdir().unwrap();
    let temp_b = tempfile::tempdir().unwrap();
    let (paired_a, pid_a) = spawn_paired_dashboard(&temp_a.path().join("child.pid"));
    let (paired_b, pid_b) = spawn_paired_dashboard(&temp_b.path().join("child.pid"));
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
