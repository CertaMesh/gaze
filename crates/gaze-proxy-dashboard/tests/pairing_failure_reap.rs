#![cfg(target_os = "linux")]

use gaze_proxy_dashboard::{
    ClientLimits, DashboardErrorCode, DashboardPayloadAcceptance, DashboardStartupConfig,
    DashboardSupervisor, IpcLimits, LoopbackBind, NoDumpReadiness, RetentionLimits,
    SpawnedDashboardChild,
};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

#[test]
#[ignore = "subprocess protocol fault helper only"]
fn pairing_fault_child() {
    let Ok(mode) = std::env::var("GAZE_TEST_PAIRING_FAULT") else {
        return;
    };
    assert_eq!(
        NoDumpReadiness::install_and_verify(),
        NoDumpReadiness::Verified
    );
    std::fs::write(
        std::env::var_os("GAZE_TEST_PID_FILE").unwrap(),
        std::process::id().to_string(),
    )
    .unwrap();
    let connect = |name| UnixStream::connect(std::env::var_os(name).unwrap()).unwrap();
    let mut control = connect("GAZE_DASHBOARD_CONTROL_SOCKET_V1");
    let _inspection = connect("GAZE_DASHBOARD_INSPECTION_SOCKET_V1");
    let _purge = connect("GAZE_DASHBOARD_PURGE_REQUEST_SOCKET_V1");
    control
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // Independently specified runtime V2 wire; only synthetic secret bytes.
    let mut envelope = vec![0; 60];
    envelope[..4].copy_from_slice(b"GZDB");
    envelope[4] = 2;
    envelope[5] = 1;
    envelope[6..22].fill(3);
    envelope[22..26].copy_from_slice(&[127, 0, 0, 1]);
    envelope[26..28].copy_from_slice(&54321_u16.to_be_bytes());
    envelope[28..].fill(7);
    if mode == "v1" {
        envelope.remove(5);
        envelope[4] = 1;
    }
    control.write_all(&envelope).unwrap();
    if mode != "v1" {
        let mut ack = [0; 23];
        control.read_exact(&mut ack).unwrap();
        assert_eq!(&ack[..6], b"GZDB\x02\x02");
        assert_eq!(&ack[6..22], &[3; 16]);
        assert_eq!(ack[22], 1);
        let mut ready = ack.to_vec();
        ready[5] = 3;
        match mode.as_str() {
            "magic" => ready[0] = 0,
            "version" => ready[4] = 1,
            "reflected_ack" => ready[5] = 2,
            "stale_nonce" => ready[6..22].fill(2),
            "status" => ready[22] = 0,
            "truncated" => {
                ready.pop();
            }
            "duplicate" => ready.extend_from_within(..),
            "trailing" => ready.push(0x42),
            "eof" | "missing" => ready.clear(),
            _ => panic!("unknown synthetic mode"),
        }
        control.write_all(&ready).unwrap();
        if mode == "truncated" || mode == "eof" {
            control.shutdown(std::net::Shutdown::Write).unwrap();
        }
    }
    // Keep malformed peers alive until the actual supervisor rejects and reaps them.
    let _ = control.read(&mut [0]);
}

#[test]
fn every_ready_failure_and_v1_peer_is_rejected_before_pending_and_reaped() {
    for mode in [
        "magic",
        "version",
        "reflected_ack",
        "stale_nonce",
        "status",
        "truncated",
        "duplicate",
        "trailing",
        "eof",
        "missing",
        "v1",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("child.pid");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "pairing_fault_child", "--nocapture"])
            .env("GAZE_TEST_PAIRING_FAULT", mode)
            .env("GAZE_TEST_PID_FILE", &pid_file);
        let child = SpawnedDashboardChild::spawn(command).unwrap();
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let config = DashboardStartupConfig::Enabled {
            acceptance: DashboardPayloadAcceptance::provider_visible(),
            bind: LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
            retention: RetentionLimits::new(4, 4096, Duration::from_secs(30)).unwrap(),
            clients: ClientLimits::conservative(),
            ipc: IpcLimits::new(4, 4096).unwrap(),
        };
        let deliveries = Arc::new(AtomicUsize::new(0));
        let delivered = deliveries.clone();
        let result = DashboardSupervisor::prepare(config, child, move |_, _: &[u8]| {
            delivered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let Err(error) = result else {
            panic!("{mode} incorrectly produced a PairedDashboard")
        };
        assert_eq!(error.code(), DashboardErrorCode::PairingFailed, "{mode}");
        assert_eq!(deliveries.load(Ordering::SeqCst), usize::from(mode != "v1"));
        // waitpid(ECHILD) proves the actual Child owner has reaped, without PID reuse races.
        assert!(matches!(
            rustix::process::waitpid(
                rustix::process::Pid::from_raw(pid),
                rustix::process::WaitOptions::NOHANG
            ),
            Err(rustix::io::Errno::CHILD)
        ));
    }
}
