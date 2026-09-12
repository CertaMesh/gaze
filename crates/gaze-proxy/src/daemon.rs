use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::ProxyError;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DaemonPaths {
    pub pidfile: PathBuf,
    pub config: PathBuf,
    pub log_file: PathBuf,
    pub stderr_file: PathBuf,
}

impl DaemonPaths {
    pub fn new(pidfile: PathBuf, config: PathBuf, log_file: PathBuf, stderr_file: PathBuf) -> Self {
        Self {
            pidfile,
            config,
            log_file,
            stderr_file,
        }
    }

    pub fn resolve() -> Result<Self, ProxyError> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| ProxyError::DaemonConfig {
                detail: "could not resolve local data directory".to_string(),
            })?
            .join("gaze");
        let config_dir = dirs::config_dir()
            .ok_or_else(|| ProxyError::DaemonConfig {
                detail: "could not resolve config directory".to_string(),
            })?
            .join("gaze");
        let log_dir = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .ok_or_else(|| ProxyError::DaemonConfig {
                    detail: "could not resolve home directory".to_string(),
                })?
                .join("Library/Logs/gaze")
        } else {
            data_dir.join("Logs")
        };
        Ok(Self {
            pidfile: data_dir.join("proxy.pid"),
            config: config_dir.join("proxy.toml"),
            log_file: log_dir.join("proxy.log"),
            stderr_file: log_dir.join("proxy-stderr.log"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DaemonConfig {
    pub bind: SocketAddr,
    pub session_ttl: String,
    pub policy: Option<PathBuf>,
    pub rulepack: Option<String>,
    pub adapters: AdapterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AdapterConfig {
    pub openai: ProviderConfig,
    pub anthropic: ProviderConfig,
    pub gemini: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ProviderConfig {
    pub upstream: Url,
}

impl ProviderConfig {
    pub fn new(upstream: Url) -> Self {
        Self { upstream }
    }
}

impl AdapterConfig {
    pub fn new(openai: Url, anthropic: Url, gemini: Url) -> Self {
        Self {
            openai: ProviderConfig::new(openai),
            anthropic: ProviderConfig::new(anthropic),
            gemini: ProviderConfig::new(gemini),
        }
    }
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            openai: ProviderConfig {
                upstream: Url::parse("https://api.openai.com").expect("static url"),
            },
            anthropic: ProviderConfig {
                upstream: Url::parse("https://api.anthropic.com").expect("static url"),
            },
            gemini: ProviderConfig {
                upstream: Url::parse("https://generativelanguage.googleapis.com")
                    .expect("static url"),
            },
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".parse().expect("static bind addr"),
            session_ttl: "30m".to_string(),
            policy: None,
            rulepack: Some("core".to_string()),
            adapters: AdapterConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StartOptions {
    pub paths: DaemonPaths,
    pub config: DaemonConfig,
    pub extra_args: Vec<String>,
}

impl StartOptions {
    pub fn new(paths: DaemonPaths, config: DaemonConfig) -> Self {
        Self {
            paths,
            config,
            extra_args: Vec::new(),
        }
    }

    pub fn with_extra_args(mut self, extra_args: Vec<String>) -> Self {
        self.extra_args = extra_args;
        self
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StopOptions {
    pub paths: DaemonPaths,
    pub timeout: Duration,
    pub force: bool,
}

impl StopOptions {
    pub fn new(paths: DaemonPaths, timeout: Duration, force: bool) -> Self {
        Self {
            paths,
            timeout,
            force,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DaemonStatus {
    pub pid: u32,
    pub bind: Option<String>,
    pub started_at: Option<String>,
    pub running: bool,
}

pub fn read_or_default_config(paths: &DaemonPaths) -> Result<DaemonConfig, ProxyError> {
    if !paths.config.exists() {
        return Ok(DaemonConfig::default());
    }
    let text = fs::read_to_string(&paths.config).map_err(|source| ProxyError::DaemonIo {
        path: paths.config.clone(),
        source,
    })?;
    toml::from_str(&text).map_err(|err| ProxyError::DaemonConfig {
        detail: err.to_string(),
    })
}

pub fn write_config(paths: &DaemonPaths, config: &DaemonConfig) -> Result<(), ProxyError> {
    if let Some(parent) = paths.config.parent() {
        fs::create_dir_all(parent).map_err(|source| ProxyError::DaemonIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let text = toml::to_string_pretty(config).map_err(|err| ProxyError::DaemonConfig {
        detail: err.to_string(),
    })?;
    fs::write(&paths.config, text).map_err(|source| ProxyError::DaemonIo {
        path: paths.config.clone(),
        source,
    })
}

/// Rebuilds the daemon's configured surface as `gaze proxy serve` flags.
///
/// The persisted [`DaemonConfig`] is the single source of truth for a detached
/// proxy: `start` and `restart` write it, and the child is handed nothing but
/// what this derives from it, so `serve` keeps exactly one policy input whether
/// it runs in the foreground or as the daemon child. The exhaustive
/// destructuring is load-bearing — a new `DaemonConfig` field stops compiling
/// here instead of silently never reaching the daemon, which is how `--policy`,
/// `--rulepack`, and the adapter upstreams went missing (solo todo #2965).
fn serve_args(config: &DaemonConfig) -> Vec<OsString> {
    let DaemonConfig {
        bind,
        session_ttl,
        policy,
        rulepack,
        adapters,
    } = config;
    let AdapterConfig {
        openai,
        anthropic,
        gemini,
    } = adapters;

    let mut args: Vec<OsString> = vec![
        "--bind".into(),
        bind.to_string().into(),
        "--session-ttl".into(),
        session_ttl.into(),
    ];
    // The policy path travels, not its resolved contents: the child loads it
    // through the same sequence as `gaze clean`, and a path that has moved since
    // `start` fails the child closed instead of quietly demoting the chokepoint
    // to the bundled floor. Serializing resolved inputs into the config would
    // make that file a second, staleable copy of the policy.
    if let Some(policy) = policy {
        args.push("--policy".into());
        args.push(policy.into());
    }
    if let Some(rulepack) = rulepack {
        args.push("--rulepack".into());
        args.push(rulepack.into());
    }
    for (flag, ProviderConfig { upstream }) in [
        ("--upstream-openai", openai),
        ("--upstream-anthropic", anthropic),
        ("--upstream-gemini", gemini),
    ] {
        args.push(flag.into());
        args.push(upstream.to_string().into());
    }
    args
}

pub fn start(options: StartOptions) -> Result<u32, ProxyError> {
    cleanup_stale(&options.paths)?;
    if let Some(status) = status(&options.paths)? {
        if status.running {
            return Err(ProxyError::DaemonAlreadyRunning {
                pid: status.pid,
                pidfile: options.paths.pidfile,
            });
        }
    }
    write_config(&options.paths, &options.config)?;
    create_parent(&options.paths.pidfile)?;
    create_parent(&options.paths.log_file)?;
    let lock = lock_pidfile(&options.paths)?;

    let mut child = {
        let spawn_result = (|| {
            let stdout = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&options.paths.log_file)
                .map_err(|source| ProxyError::DaemonIo {
                    path: options.paths.log_file.clone(),
                    source,
                })?;
            let stderr = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&options.paths.stderr_file)
                .map_err(|source| ProxyError::DaemonIo {
                    path: options.paths.stderr_file.clone(),
                    source,
                })?;
            let mut command =
                Command::new(
                    std::env::current_exe().map_err(|source| ProxyError::DaemonIo {
                        path: PathBuf::from("current_exe"),
                        source,
                    })?,
                );
            command
                .args(["proxy", "serve", "--_foreground-daemon"])
                .args(serve_args(&options.config))
                .args(&options.extra_args)
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            drop(lock);
            command.spawn().map_err(|source| ProxyError::DaemonIo {
                path: PathBuf::from("gaze proxy serve"),
                source,
            })
        })();
        match spawn_result {
            Ok(child) => child,
            Err(e) => {
                // The lock was already released before the spawn attempt so
                // that the child could acquire it.  Remove the empty pidfile
                // only when we can re-acquire the exclusive lock and confirm
                // no PID has been written; if another startup has taken over
                // the file in the interim, leave it alone.
                if let Ok(mut f) = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&options.paths.pidfile)
                {
                    if f.try_lock_exclusive().is_ok() {
                        let mut contents = String::new();
                        // Only remove if we can read the file and it is still
                        // empty; a read error means we cannot confirm the
                        // contents, so leave the file alone.
                        if f.read_to_string(&mut contents).is_ok()
                            && contents.trim().is_empty()
                        {
                            let _ = fs::remove_file(&options.paths.pidfile);
                        }
                    }
                }
                return Err(e);
            }
        }
    };
    confirm_started(&mut child, &options.paths)
}

/// Confirms the spawned child is still alive after a short probe window and
/// returns its pid.
///
/// The child resolves the configured policy, so a fail-closed load error
/// surfaces as an immediate exit. `kill(pid, 0)` cannot see that: an unreaped
/// child is a zombie and still answers as alive, which reported a dead daemon as
/// started (solo todo #2965). `try_wait` reaps and reports the real status
/// within the same 250ms budget, returning early on failure.
///
/// A failure slower than the probe window still reports success; that daemon is
/// dead rather than serving unprotected, and `gaze proxy status` shows it.
fn confirm_started(child: &mut Child, paths: &DaemonPaths) -> Result<u32, ProxyError> {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // The parent created this pidfile before spawning and the child
                // never filled it in; leaving the empty file behind would fail
                // every later start as stale.
                let _ = fs::remove_file(&paths.pidfile);
                return Err(ProxyError::DaemonExitedEarly {
                    code: status.code(),
                    stderr_file: paths.stderr_file.clone(),
                });
            }
            Ok(None) if Instant::now() >= deadline => return Ok(child.id()),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(source) => {
                return Err(ProxyError::DaemonIo {
                    path: PathBuf::from("gaze proxy serve"),
                    source,
                })
            }
        }
    }
}

pub fn init_foreground_daemon(paths: &DaemonPaths, bind: SocketAddr) -> Result<(), ProxyError> {
    create_parent(&paths.pidfile)?;
    create_parent(&paths.log_file)?;
    let _lock = lock_pidfile(paths)?;
    write_pidfile(&paths.pidfile, std::process::id(), bind, Utc::now())
}

pub fn stop(options: StopOptions) -> Result<(), ProxyError> {
    cleanup_stale(&options.paths)?;
    let Some(status) = status(&options.paths)? else {
        return Err(ProxyError::DaemonNotRunning);
    };
    terminate(status.pid, false)?;
    let started = Instant::now();
    while started.elapsed() < options.timeout {
        if !process_exists(status.pid) {
            let _ = fs::remove_file(&options.paths.pidfile);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if options.force {
        terminate(status.pid, true)?;
        let _ = fs::remove_file(&options.paths.pidfile);
        Ok(())
    } else {
        Err(ProxyError::DaemonAlreadyRunning {
            pid: status.pid,
            pidfile: options.paths.pidfile,
        })
    }
}

pub fn restart(options: StartOptions, timeout: Duration, force: bool) -> Result<u32, ProxyError> {
    let stop_result = stop(StopOptions {
        paths: options.paths.clone(),
        timeout,
        force,
    });
    if !matches!(stop_result, Ok(()) | Err(ProxyError::DaemonNotRunning)) {
        cleanup_stale(&options.paths)?;
    }
    start(options)
}

pub fn status(paths: &DaemonPaths) -> Result<Option<DaemonStatus>, ProxyError> {
    if !paths.pidfile.exists() {
        return Ok(None);
    }
    let record = read_pidfile(&paths.pidfile)?;
    Ok(Some(DaemonStatus {
        running: process_exists(record.pid),
        pid: record.pid,
        bind: record.bind,
        started_at: record.started_at,
    }))
}

pub fn cleanup_stale(paths: &DaemonPaths) -> Result<(), ProxyError> {
    // Open the pidfile and acquire an exclusive lock before inspecting or
    // removing it.  Classifying the file via status() and then removing by
    // pathname (without holding a lock) creates a window: a concurrent startup
    // can publish a live PID between the read and the remove, and we would
    // delete the running daemon's pidfile.  Acquiring the lock first ties
    // cleanup to the inode we are about to remove and prevents that window.
    let mut f = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.pidfile)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ProxyError::DaemonIo {
                path: paths.pidfile.clone(),
                source,
            })
        }
    };

    // If we cannot acquire the exclusive lock, an active startup or the daemon
    // itself holds it; leave the file intact.
    if f.try_lock_exclusive().is_err() {
        return Ok(());
    }

    // Re-read the contents under the lock so we see the final state.
    let mut contents = String::new();
    f.read_to_string(&mut contents).map_err(|source| ProxyError::DaemonIo {
        path: paths.pidfile.clone(),
        source,
    })?;

    // Parse the PID from the locked file.  Empty/unparseable means the
    // startup that created this file never finished; removable.
    let is_stale = if contents.trim().is_empty() {
        true
    } else {
        match contents.lines().next().and_then(|l| l.parse::<u32>().ok()) {
            Some(pid) => !process_exists(pid),
            None => true, // unparseable — treat as stale
        }
    };

    if !is_stale {
        return Ok(());
    }

    // The file is stale and we hold the lock.  Remove by inode (the lock fd
    // still refers to this inode even after the unlink, so no other path
    // can slip a replacement in under the same lock).
    match fs::remove_file(&paths.pidfile) {
        Ok(()) => Ok(()),
        // Benign: another cleanup already removed it between our open and now.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ProxyError::DaemonIo {
            path: paths.pidfile.clone(),
            source,
        }),
    }
}

pub fn logs(paths: &DaemonPaths, follow: bool) -> Result<(), ProxyError> {
    if follow {
        Command::new("tail")
            .arg("-F")
            .arg(&paths.log_file)
            .status()
            .map_err(|source| ProxyError::DaemonIo {
                path: paths.log_file.clone(),
                source,
            })?;
    } else if paths.log_file.exists() {
        let text = fs::read_to_string(&paths.log_file).map_err(|source| ProxyError::DaemonIo {
            path: paths.log_file.clone(),
            source,
        })?;
        print!("{text}");
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<(), ProxyError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ProxyError::DaemonIo {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn lock_pidfile(paths: &DaemonPaths) -> Result<File, ProxyError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&paths.pidfile)
        .map_err(|source| ProxyError::DaemonIo {
            path: paths.pidfile.clone(),
            source,
        })?;
    file.try_lock_exclusive()
        .map_err(|source| ProxyError::DaemonIo {
            path: paths.pidfile.clone(),
            source,
        })?;
    Ok(file)
}

fn write_pidfile(
    path: &Path,
    pid: u32,
    bind: SocketAddr,
    started_at: DateTime<Utc>,
) -> Result<(), ProxyError> {
    let contents = format!(
        "{pid}\nbind={bind}\nstarted_at={}\n",
        started_at.to_rfc3339()
    );
    if contents.len() > 200 {
        return Err(ProxyError::DaemonConfig {
            detail: "pidfile content exceeds 200 bytes".to_string(),
        });
    }
    fs::write(path, contents).map_err(|source| ProxyError::DaemonIo {
        path: path.to_path_buf(),
        source,
    })
}

struct PidRecord {
    pid: u32,
    bind: Option<String>,
    started_at: Option<String>,
}

fn read_pidfile(path: &Path) -> Result<PidRecord, ProxyError> {
    let mut text = String::new();
    File::open(path)
        .and_then(|mut file| file.read_to_string(&mut text))
        .map_err(|source| ProxyError::DaemonIo {
            path: path.to_path_buf(),
            source,
        })?;
    let mut lines = text.lines();
    let pid = lines
        .next()
        .ok_or_else(|| ProxyError::DaemonPidfileStale {
            pidfile: path.to_path_buf(),
        })?
        .parse::<u32>()
        .map_err(|_| ProxyError::DaemonPidfileStale {
            pidfile: path.to_path_buf(),
        })?;
    let mut bind = None;
    let mut started_at = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("bind=") {
            bind = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("started_at=") {
            started_at = Some(rest.to_string());
        }
    }
    Ok(PidRecord {
        pid,
        bind,
        started_at,
    })
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_exists(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate(pid: u32, force: bool) -> Result<(), ProxyError> {
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    let rc = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if rc == 0 {
        Ok(())
    } else {
        Err(ProxyError::DaemonNotRunning)
    }
}

#[cfg(not(unix))]
fn terminate(_pid: u32, _force: bool) -> Result<(), ProxyError> {
    Err(ProxyError::DaemonConfig {
        detail: "daemon stop is not implemented on this platform".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(dir: &tempfile::TempDir) -> DaemonPaths {
        DaemonPaths {
            pidfile: dir.path().join("proxy.pid"),
            config: dir.path().join("proxy.toml"),
            log_file: dir.path().join("proxy.log"),
            stderr_file: dir.path().join("proxy-stderr.log"),
        }
    }

    #[test]
    fn pidfile_round_trip_includes_bind_and_started_at() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        write_pidfile(
            &paths.pidfile,
            123,
            "127.0.0.1:8787".parse().unwrap(),
            DateTime::parse_from_rfc3339("2026-05-14T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        let status = status(&paths).unwrap().unwrap();
        assert_eq!(status.pid, 123);
        assert_eq!(status.bind.as_deref(), Some("127.0.0.1:8787"));
        assert!(!status.running);
    }

    #[test]
    fn cleanup_stale_unlinks_dead_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        write_pidfile(
            &paths.pidfile,
            999_999,
            "127.0.0.1:8787".parse().unwrap(),
            Utc::now(),
        )
        .unwrap();
        cleanup_stale(&paths).unwrap();
        assert!(!paths.pidfile.exists());
    }

    #[test]
    fn cleanup_stale_unlinks_empty_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        std::fs::write(&paths.pidfile, "").unwrap();

        cleanup_stale(&paths).unwrap();
        assert!(!paths.pidfile.exists());
        assert!(status(&paths).unwrap().is_none());
    }

    #[test]
    fn cleanup_stale_leaves_running_pidfile_alone() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        init_foreground_daemon(&paths, "127.0.0.1:8787".parse().unwrap()).unwrap();

        cleanup_stale(&paths).unwrap();
        assert!(
            paths.pidfile.exists(),
            "cleanup_stale must not unlink a live daemon pidfile"
        );
    }

    #[test]
    fn cleanup_stale_propagates_non_stale_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        std::fs::create_dir(&paths.pidfile).unwrap();

        let err = cleanup_stale(&paths).unwrap_err();
        assert!(matches!(err, ProxyError::DaemonIo { .. }));
        assert!(
            paths.pidfile.exists(),
            "cleanup_stale must not unlink for non-stale errors"
        );
    }

    // solo todo #2965: every field the adopter can configure has to reach the
    // detached child, because the child's argv is all it ever sees.
    #[test]
    fn serve_args_forward_every_configured_field_to_the_daemon_child() {
        let config = DaemonConfig {
            policy: Some(PathBuf::from("/etc/gaze/prod.toml")),
            rulepack: Some("core-extended".to_string()),
            adapters: AdapterConfig::new(
                Url::parse("http://127.0.0.1:4001").unwrap(),
                Url::parse("http://127.0.0.1:4002").unwrap(),
                Url::parse("http://127.0.0.1:4003").unwrap(),
            ),
            ..Default::default()
        };

        assert_eq!(
            serve_args(&config),
            [
                "--bind",
                "127.0.0.1:8787",
                "--session-ttl",
                "30m",
                "--policy",
                "/etc/gaze/prod.toml",
                "--rulepack",
                "core-extended",
                "--upstream-openai",
                "http://127.0.0.1:4001/",
                "--upstream-anthropic",
                "http://127.0.0.1:4002/",
                "--upstream-gemini",
                "http://127.0.0.1:4003/",
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn serve_args_omit_policy_when_the_daemon_has_none() {
        let args = serve_args(&DaemonConfig::default());

        assert!(!args.contains(&OsString::from("--policy")));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--rulepack", "core"].map(OsString::from)));
    }

    #[test]
    fn config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let config = DaemonConfig::default();
        write_config(&paths, &config).unwrap();
        let loaded = read_or_default_config(&paths).unwrap();
        assert_eq!(loaded.bind, config.bind);
        assert_eq!(
            loaded.adapters.openai.upstream,
            config.adapters.openai.upstream
        );
    }

    /// A locked empty pidfile is a startup in progress.  `cleanup_stale` must
    /// not unlink it: doing so lets a concurrent caller create a fresh inode at
    /// the same path, acquire its own lock, and proceed as a second daemon.
    #[test]
    fn cleanup_stale_preserves_locked_startup_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);

        // Simulate a startup that has created and locked an empty pidfile but
        // has not yet published a PID.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&paths.pidfile)
            .unwrap();
        lock.try_lock_exclusive()
            .expect("should be able to lock a freshly created file");

        // `cleanup_stale` sees an unparseable (empty) file, but must recognise
        // the exclusive lock and leave the inode intact.
        cleanup_stale(&paths).unwrap();

        assert!(
            paths.pidfile.exists(),
            "cleanup_stale must not unlink an actively locked startup pidfile"
        );

        // A second concurrent start attempt must not be able to re-lock the
        // file, proving the original startup still owns it.
        let second = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&paths.pidfile)
            .unwrap();
        assert!(
            second.try_lock_exclusive().is_err(),
            "a second startup must not acquire the lock while the first holds it"
        );

        // Releasing the lock explicitly (mimics startup completing or aborting).
        drop(lock);
    }

    /// `cleanup_stale` must propagate real unlink failures rather than
    /// swallowing them and reporting success.  This regression covers the
    /// deterministic path described in the r2 review: make the parent directory
    /// read/execute-only so that `unlink` (which requires write permission on
    /// the parent) fails with EACCES, then verify that the file is still present
    /// and that `cleanup_stale` returns an I/O error.
    ///
    /// Skipped when running as root because root bypasses DAC permission checks.
    #[test]
    #[cfg(unix)]
    fn cleanup_stale_reports_unlink_failure() {
        use std::os::unix::fs::PermissionsExt;

        if unsafe { libc::getuid() } == 0 {
            // root ignores DAC permissions; the test would be a false pass.
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);

        // Create an empty (stale) pidfile.
        std::fs::write(&paths.pidfile, "").unwrap();

        // Revoke write permission on the parent directory so unlink fails.
        let parent = paths.pidfile.parent().unwrap();
        let original = std::fs::metadata(parent).unwrap().permissions();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = cleanup_stale(&paths);

        // Restore permissions before asserting so the TempDir drop can clean up.
        std::fs::set_permissions(parent, original).unwrap();

        assert!(
            result.is_err(),
            "cleanup_stale must propagate the unlink failure, not return Ok"
        );
        assert!(
            paths.pidfile.exists(),
            "the stale pidfile must remain when unlink failed"
        );
    }
}
