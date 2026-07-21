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
fn binding_precedes_socket_writer_runtime_and_admission_activation_in_commit() {
    let source = include_str!("../src/supervisor.rs");
    let commit = source
        .find("impl PendingDashboardActivation")
        .expect("pending activation implementation");
    let body = &source[commit..];
    let binding = body.find(".bind(activated)").expect("binding step");
    for (name, needle) in [
        ("socket take", "child.take_inspection()"),
        ("writer spawn", "WriterHandle::spawn"),
        ("runtime start", "DashboardLaunch::start"),
        ("admission activation", "admission.activate()"),
    ] {
        let side_effect = body.find(needle).unwrap_or_else(|| panic!("missing {name}"));
        assert!(binding < side_effect, "binding must precede {name}");
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
