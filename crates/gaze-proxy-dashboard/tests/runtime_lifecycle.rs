#![cfg(unix)]

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::time::Duration;

use gaze_inspection::{install_inspection_v1, PendingInspectionProducerV1};
use gaze_proxy_dashboard::{
    ChildConfig, ChildInheritedHandles, ClientLimits, DashboardChildEntrypoint, DashboardLifecycle,
    DashboardPayloadAcceptance, DashboardStartupConfig, DashboardSupervisor, IpcLimits,
    LoopbackBind, RetentionLimits, SpawnedDashboardChild,
};

#[test]
#[ignore = "subprocess helper only"]
fn dashboard_child_helper() {
    let Ok(control_path) = std::env::var("GAZE_TEST_CONTROL_SOCKET") else {
        return;
    };
    let inspection_path = std::env::var("GAZE_TEST_INSPECTION_SOCKET").unwrap();
    let control = UnixStream::connect(control_path).unwrap();
    let inspection = UnixStream::connect(inspection_path).unwrap();
    let handles = ChildInheritedHandles::new(control, inspection).unwrap();
    let config = ChildConfig::new(
        LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
        RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap(),
        ClientLimits::conservative(),
        IpcLimits::new(4, 64 * 1024).unwrap(),
    );
    DashboardChildEntrypoint::run(handles, config).unwrap();
}

#[test]
fn activated_consumer_owns_serialized_purge_rotation_and_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let control_path = temp.path().join("control.sock");
    let inspection_path = temp.path().join("inspection.sock");
    let control_listener = UnixListener::bind(&control_path).unwrap();
    let inspection_listener = UnixListener::bind(&inspection_path).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "dashboard_child_helper",
            "--nocapture",
        ])
        .env("GAZE_TEST_CONTROL_SOCKET", &control_path)
        .env("GAZE_TEST_INSPECTION_SOCKET", &inspection_path)
        .spawn()
        .unwrap();
    let (control, _) = control_listener.accept().unwrap();
    let (inspection, _) = inspection_listener.accept().unwrap();
    let spawned = SpawnedDashboardChild::new(child, control, inspection).unwrap();

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
    let (pending, consumer, descriptor) = paired.into_pending_activation().unwrap();
    let producer = PendingInspectionProducerV1::new(descriptor);
    let (_producer, activated) = install_inspection_v1(producer, consumer).unwrap();
    let launch = pending.commit(activated).unwrap();
    let control = launch.control();

    control.purge().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(1));
    control
        .rotate_pairing_secret(Box::new(|_authority, token: &[u8]| {
            assert_eq!(token.len(), 43);
            Ok::<(), io::Error>(())
        }))
        .unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Running(2));
    control.shutdown().unwrap();
    assert_eq!(control.lifecycle(), DashboardLifecycle::Stopped);
}
