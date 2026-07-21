#![cfg(unix)]
#![cfg_attr(target_os = "macos", allow(dead_code, unused_imports))]

use std::io;
use std::process::Command;
use std::time::Duration;

use gaze_inspection::{install_inspection_v1, PendingInspectionProducerV1};
use gaze_proxy_dashboard::{
    ChildConfig, ChildInheritedHandles, ClientLimits, DashboardChildEntrypoint,
    DashboardPayloadAcceptance, DashboardStartupConfig, DashboardSupervisor, IpcLimits,
    LoopbackBind, RetentionLimits, SpawnedDashboardChild,
};

#[test]
#[ignore = "subprocess helper only"]
fn dashboard_child_helper() {
    let Some(_) = std::env::var_os("GAZE_DASHBOARD_CONTROL_SOCKET_V1") else {
        return;
    };
    let handles = ChildInheritedHandles::connect_from_environment().unwrap();
    let config = ChildConfig::new(
        LoopbackBind::configured("127.0.0.1:0".parse().unwrap()).unwrap(),
        RetentionLimits::new(4, 64 * 1024, Duration::from_secs(30)).unwrap(),
        ClientLimits::conservative(),
        IpcLimits::new(4, 64 * 1024).unwrap(),
    );
    DashboardChildEntrypoint::run(handles, config).unwrap();
}

#[test]
#[cfg(not(target_os = "macos"))]
fn activation_fails_closed_without_registration_identity_authority() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "dashboard_child_helper",
        "--nocapture",
    ]);
    let spawned = SpawnedDashboardChild::spawn(command).unwrap();

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
    let Err(error) = pending.commit(activated) else {
        panic!("activation unexpectedly succeeded without an identity receipt");
    };
    assert_eq!(
        error.code(),
        gaze_proxy_dashboard::DashboardErrorCode::ActivationFailed
    );
}
