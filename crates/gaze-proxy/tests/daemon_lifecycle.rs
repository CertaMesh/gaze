use std::time::Duration;

use gaze_proxy::daemon::{self, DaemonConfig, DaemonPaths, StartOptions, StopOptions};

fn temp_paths(dir: &tempfile::TempDir) -> DaemonPaths {
    DaemonPaths::new(
        dir.path().join("proxy.pid"),
        dir.path().join("proxy.toml"),
        dir.path().join("proxy.log"),
        dir.path().join("proxy-stderr.log"),
    )
}

#[test]
fn daemon_config_and_pidfile_lifecycle_use_state_paths() {
    let dir = tempfile::tempdir().unwrap();
    let paths = temp_paths(&dir);
    let config = DaemonConfig::default();

    daemon::write_config(&paths, &config).unwrap();
    let loaded = daemon::read_or_default_config(&paths).unwrap();
    assert_eq!(loaded.bind, config.bind);

    daemon::init_foreground_daemon(&paths, config.bind).unwrap();
    let status = daemon::status(&paths).unwrap().unwrap();
    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.bind.as_deref(), Some("127.0.0.1:8787"));
    assert!(status.running);
}

#[test]
fn stale_pidfile_cleanup_unlinks_dead_process_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = temp_paths(&dir);
    std::fs::write(
        &paths.pidfile,
        "999999\nbind=127.0.0.1:8787\nstarted_at=2026-05-14T00:00:00Z\n",
    )
    .unwrap();

    daemon::cleanup_stale(&paths).unwrap();
    assert!(!paths.pidfile.exists());
}

#[test]
fn stop_reports_not_running_when_pidfile_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = temp_paths(&dir);
    let err = daemon::stop(StopOptions::new(paths, Duration::from_millis(10), false)).unwrap_err();
    assert!(matches!(err, gaze_proxy::ProxyError::DaemonNotRunning));
}

#[test]
fn status_reports_stale_for_empty_pidfile_without_recovering() {
    let dir = tempfile::tempdir().unwrap();
    let paths = temp_paths(&dir);
    std::fs::write(&paths.pidfile, "").unwrap();

    let err = daemon::status(&paths).unwrap_err();
    assert!(matches!(
        err,
        gaze_proxy::ProxyError::DaemonPidfileStale { .. }
    ));
    assert!(
        paths.pidfile.exists(),
        "status is a read and must not mutate the pidfile"
    );
}

#[test]
fn start_failure_leaves_no_pidfile_and_retry_does_not_brick() {
    let dir = tempfile::tempdir().unwrap();
    let paths = temp_paths(&dir);
    std::fs::create_dir(&paths.stderr_file).unwrap();

    let err = daemon::start(StartOptions::new(paths.clone(), DaemonConfig::default())).unwrap_err();
    assert!(matches!(err, gaze_proxy::ProxyError::DaemonIo { .. }));
    assert!(
        !paths.pidfile.exists(),
        "a failed start must not leave the pidfile that bricks the next one"
    );

    daemon::cleanup_stale(&paths).unwrap();
    assert!(daemon::status(&paths).unwrap().is_none());

    let err2 =
        daemon::start(StartOptions::new(paths.clone(), DaemonConfig::default())).unwrap_err();
    assert!(
        matches!(err2, gaze_proxy::ProxyError::DaemonIo { .. }),
        "retry must fail with the original error, not a stale-pidfile brick"
    );
    assert!(!paths.pidfile.exists());
}
