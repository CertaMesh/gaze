use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek};
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
    let owned_file = lock
        .file
        .try_clone()
        .map_err(|source| daemon_io(&options.paths.pidfile, source))?;

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
                #[cfg(test)]
                tests::run_start_before_cleanup_hook();
                cleanup_owned(&options.paths.pidfile, owned_file, CleanupRule::Empty)?;
                return Err(e);
            }
        }
    };
    confirm_started(&mut child, &options.paths, owned_file)
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
fn confirm_started(
    child: &mut Child,
    paths: &DaemonPaths,
    owned_file: File,
) -> Result<u32, ProxyError> {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                cleanup_owned(&paths.pidfile, owned_file, CleanupRule::Stale)?;
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
                });
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
    let owned_file = open_pidfile(&options.paths.pidfile)?;
    terminate(status.pid, false)?;
    let started = Instant::now();
    while started.elapsed() < options.timeout {
        if !process_exists(status.pid) {
            if let Some(file) = owned_file {
                cleanup_owned(&options.paths.pidfile, file, CleanupRule::Pid(status.pid))?;
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if options.force {
        terminate(status.pid, true)?;
        if let Some(file) = owned_file {
            cleanup_owned(&options.paths.pidfile, file, CleanupRule::Pid(status.pid))?;
        }
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
    let Some(file) = open_pidfile(&paths.pidfile)? else {
        return Ok(());
    };
    #[cfg(test)]
    tests::run_cleanup_after_open_hook();
    cleanup_owned(&paths.pidfile, file, CleanupRule::Stale)?;
    Ok(())
}

fn daemon_io(path: &Path, source: std::io::Error) -> ProxyError {
    ProxyError::DaemonIo {
        path: path.to_path_buf(),
        source,
    }
}

fn open_pidfile(path: &Path) -> Result<Option<File>, ProxyError> {
    match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => Ok(Some(file)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(daemon_io(path, source)),
    }
}

fn lock_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}

fn try_pidfile_lock(file: &File) -> std::io::Result<()> {
    #[cfg(test)]
    if let Some(error) = tests::take_lock_error() {
        return Err(error);
    }
    file.try_lock_exclusive()
}

#[derive(Debug, PartialEq, Eq)]
enum CleanupOutcome {
    Removed,
    Missing,
    Replaced,
    Busy,
    Kept,
}

#[derive(Clone, Copy)]
enum CleanupRule {
    Empty,
    Stale,
    Pid(u32),
}

impl CleanupRule {
    fn permits(self, contents: &str) -> bool {
        let pid = contents
            .lines()
            .next()
            .and_then(|line| line.parse::<u32>().ok());
        match self {
            Self::Empty => contents.trim().is_empty(),
            Self::Stale => pid.is_none_or(|pid| !process_exists(pid)),
            Self::Pid(expected) => pid == Some(expected),
        }
    }
}

/// Owns both the descriptor lock and the persistent `<pidfile>.lock` lock.
///
/// Identity uses `same_file::Handle`, comparing `(dev, ino)` on Unix and the
/// corresponding file identity on Windows.
///
/// The sidecar is never unlinked: startup takes it before creating the pidfile,
/// foreground publication holds it through the write, and every removal (stale,
/// failed spawn, early exit, stop) goes through this guard. Thus no competing
/// lifecycle path can replace the pathname between identity check and unlink.
/// Lock order is always sidecar then pidfile; contention never blocks a handoff.
/// External programs must not remove the sidecar or mutate the pidfile while
/// Gaze is using it. The inode lock alone cannot protect a pathname after unlink.
struct PidfileGuard {
    file: File,
    _namespace: File,
}

impl Drop for PidfileGuard {
    fn drop(&mut self) {
        // A failed startup retains a cloned descriptor as its identity witness.
        // Explicit unlock releases the shared lock before the child takes over.
        let _ = FileExt::unlock(&self.file);
    }
}

impl PidfileGuard {
    fn namespace(path: &Path) -> std::io::Result<File> {
        let mut name = path.as_os_str().to_os_string();
        name.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(name))?;
        file.try_lock_exclusive()?;
        Ok(file)
    }

    fn remove(mut self, path: &Path, rule: CleanupRule) -> Result<CleanupOutcome, ProxyError> {
        let identity = same_file::Handle::from_file(
            self.file
                .try_clone()
                .map_err(|source| daemon_io(path, source))?,
        )
        .map_err(|source| daemon_io(path, source))?;
        let current = match same_file::Handle::from_path(path) {
            Ok(current) => current,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CleanupOutcome::Missing)
            }
            Err(source) => return Err(daemon_io(path, source)),
        };
        if identity != current {
            tracing::debug!("pidfile cleanup skipped: owned inode was replaced");
            return Ok(CleanupOutcome::Replaced);
        }
        self.file
            .rewind()
            .map_err(|source| daemon_io(path, source))?;
        let mut contents = String::new();
        self.file
            .read_to_string(&mut contents)
            .map_err(|source| daemon_io(path, source))?;
        if !rule.permits(&contents) {
            return Ok(CleanupOutcome::Kept);
        }
        #[cfg(test)]
        tests::run_cleanup_before_unlink_hook();
        match fs::remove_file(path) {
            Ok(()) => Ok(CleanupOutcome::Removed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CleanupOutcome::Missing),
            Err(source) => Err(daemon_io(path, source)),
        }
    }
}

fn cleanup_owned(path: &Path, file: File, rule: CleanupRule) -> Result<CleanupOutcome, ProxyError> {
    let namespace = match PidfileGuard::namespace(path) {
        Ok(lock) => lock,
        Err(e) if lock_contended(&e) => return Ok(CleanupOutcome::Busy),
        Err(source) => return Err(daemon_io(path, source)),
    };
    match try_pidfile_lock(&file) {
        Ok(()) => PidfileGuard {
            file,
            _namespace: namespace,
        }
        .remove(path, rule),
        Err(e) if lock_contended(&e) => Ok(CleanupOutcome::Busy),
        Err(source) => Err(daemon_io(path, source)),
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

fn lock_pidfile(paths: &DaemonPaths) -> Result<PidfileGuard, ProxyError> {
    let namespace = PidfileGuard::namespace(&paths.pidfile)
        .map_err(|source| daemon_io(&paths.pidfile, source))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&paths.pidfile)
        .map_err(|source| daemon_io(&paths.pidfile, source))?;
    try_pidfile_lock(&file).map_err(|source| daemon_io(&paths.pidfile, source))?;
    Ok(PidfileGuard {
        file,
        _namespace: namespace,
    })
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

    thread_local! {
        static CLEANUP_AFTER_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
            std::cell::RefCell::new(None);
    }

    thread_local! {
        static START_BEFORE_CLEANUP: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
        static CLEANUP_BEFORE_UNLINK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
        static LOCK_ERROR: std::cell::RefCell<Option<std::io::Error>> = const { std::cell::RefCell::new(None) };
    }

    pub(super) fn take_lock_error() -> Option<std::io::Error> {
        LOCK_ERROR.with(|slot| slot.borrow_mut().take())
    }

    pub(super) fn run_start_before_cleanup_hook() {
        let hook = START_BEFORE_CLEANUP.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }

    pub(super) fn run_cleanup_before_unlink_hook() {
        let hook = CLEANUP_BEFORE_UNLINK.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }

    pub(super) fn run_cleanup_after_open_hook() {
        let hook = CLEANUP_AFTER_OPEN.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }

    #[test]
    #[cfg(unix)]
    fn r3_cleanup_preserves_replacement_inode_after_open() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(&paths.pidfile, "").unwrap();
        let old_inode = fs::metadata(&paths.pidfile).unwrap().ino();
        let replacement_lock = std::rc::Rc::new(std::cell::RefCell::new(None));
        let held = replacement_lock.clone();
        let concurrent_paths = paths.clone();
        CLEANUP_AFTER_OPEN.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                // A second cleaner finishes after this cleaner opened the old inode.
                cleanup_stale(&concurrent_paths).unwrap();
                let lock = lock_pidfile(&concurrent_paths).unwrap();
                write_pidfile(
                    &concurrent_paths.pidfile,
                    std::process::id(),
                    "127.0.0.1:8787".parse().unwrap(),
                    Utc::now(),
                )
                .unwrap();
                assert_ne!(
                    fs::metadata(&concurrent_paths.pidfile).unwrap().ino(),
                    old_inode
                );
                *held.borrow_mut() = Some(lock);
            }));
        });
        cleanup_stale(&paths).unwrap();
        assert!(
            paths.pidfile.exists(),
            "cleanup unlinked the replacement startup's locked inode"
        );
        assert!(status(&paths).unwrap().unwrap().running);
        drop(replacement_lock);
    }

    #[test]
    fn w1_cleanup_preserves_locked_startup_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let _lock = lock_pidfile(&paths).unwrap();
        let _ = cleanup_stale(&paths);
        assert!(
            paths.pidfile.exists(),
            "cleanup unlinked an actively locked startup pidfile"
        );
    }

    #[cfg(unix)]
    #[test]
    fn r2_cleanup_stale_reports_unlink_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        std::fs::write(&paths.pidfile, "").unwrap();
        // Reach unlink, rather than failing to create the sidecar directory entry.
        drop(PidfileGuard::namespace(&paths.pidfile).unwrap());
        let reached_unlink = std::rc::Rc::new(std::cell::Cell::new(false));
        let reached = reached_unlink.clone();
        CLEANUP_BEFORE_UNLINK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || reached.set(true)));
        });
        let original_permissions = std::fs::metadata(dir.path()).unwrap().permissions();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let cleanup_result = cleanup_stale(&paths);
        let remains = paths.pidfile.exists();
        // Restore permissions before asserting so the fixture always cleans up.
        std::fs::set_permissions(dir.path(), original_permissions).unwrap();
        assert!(
            reached_unlink.get(),
            "fixture must reach the unlink operation"
        );
        assert!(
            remains,
            "fixture must actually prevent unlink (run unprivileged)"
        );
        assert!(
            matches!(cleanup_result, Err(ProxyError::DaemonIo { .. })),
            "cleanup must report the failed unlink instead of success: {cleanup_result:?}"
        );
    }

    #[test]
    fn cleanup_reports_non_contention_lock_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(&paths.pidfile, "").unwrap();
        LOCK_ERROR
            .with(|slot| *slot.borrow_mut() = Some(std::io::Error::from_raw_os_error(libc::EIO)));
        let error = cleanup_stale(&paths).unwrap_err();
        assert!(
            matches!(error, ProxyError::DaemonIo { source, .. } if source.raw_os_error() == Some(libc::EIO))
        );
        assert!(paths.pidfile.exists());
    }

    #[test]
    fn cleanup_documented_lock_contention_is_benign() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(&paths.pidfile, "").unwrap();
        LOCK_ERROR.with(|slot| *slot.borrow_mut() = Some(fs2::lock_contended_error()));
        cleanup_stale(&paths).unwrap();
        assert!(paths.pidfile.exists());
        cleanup_stale(&paths).unwrap();
        assert!(!paths.pidfile.exists());
    }

    #[test]
    fn cleanup_holds_namespace_through_unlink() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(&paths.pidfile, "").unwrap();
        let concurrent_paths = paths.clone();
        CLEANUP_BEFORE_UNLINK.with(|slot| *slot.borrow_mut() = Some(Box::new(move || {
            // Exercise startup and another cleaner in the check-to-unlink window.
            assert!(matches!(PidfileGuard::namespace(&concurrent_paths.pidfile), Err(error) if lock_contended(&error)));
            assert!(matches!(lock_pidfile(&concurrent_paths), Err(ProxyError::DaemonIo { source, .. }) if lock_contended(&source)));
            let file = open_pidfile(&concurrent_paths.pidfile).unwrap().unwrap();
            assert_eq!(cleanup_owned(&concurrent_paths.pidfile, file, CleanupRule::Stale).unwrap(), CleanupOutcome::Busy);
            assert!(concurrent_paths.pidfile.exists());
        })));
        cleanup_stale(&paths).unwrap();
        assert!(!paths.pidfile.exists());
        let lock = lock_pidfile(&paths).unwrap();
        drop(lock);
        cleanup_stale(&paths).unwrap();
    }

    #[test]
    fn cleanup_failed_start_preserves_replacement_inode() {
        // Both a startup holding its lock and an unlocked, still-empty handoff
        // belong to the replacement. Contents alone cannot establish ownership.
        for hold_lock in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = temp_paths(&dir);
            fs::create_dir(&paths.stderr_file).unwrap();
            let replacement = std::rc::Rc::new(std::cell::RefCell::new(None));
            let held = replacement.clone();
            let concurrent_paths = paths.clone();
            START_BEFORE_CLEANUP.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    cleanup_stale(&concurrent_paths).unwrap();
                    let lock = lock_pidfile(&concurrent_paths).unwrap();
                    if hold_lock {
                        *held.borrow_mut() = Some(lock);
                    }
                }))
            });
            assert!(matches!(
                start(StartOptions::new(paths.clone(), DaemonConfig::default())),
                Err(ProxyError::DaemonIo { .. })
            ));
            assert!(
                paths.pidfile.exists(),
                "failed start removed replacement (locked={hold_lock})"
            );
            assert_eq!(fs::read_to_string(&paths.pidfile).unwrap(), "");
        }
    }

    #[test]
    fn cleanup_failed_start_reports_lock_error() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::create_dir(&paths.stderr_file).unwrap();
        START_BEFORE_CLEANUP.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(|| {
                LOCK_ERROR.with(|slot| {
                    *slot.borrow_mut() = Some(std::io::Error::from_raw_os_error(libc::EIO))
                });
            }))
        });
        let error = start(StartOptions::new(paths.clone(), DaemonConfig::default())).unwrap_err();
        assert!(
            matches!(error, ProxyError::DaemonIo { source, .. } if source.raw_os_error() == Some(libc::EIO))
        );
        assert!(paths.pidfile.exists());
    }

    #[test]
    fn cleanup_stop_preserves_changed_pid_on_owned_inode() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(&paths.pidfile, "999999").unwrap();
        let original = open_pidfile(&paths.pidfile).unwrap().unwrap();
        init_foreground_daemon(&paths, "127.0.0.1:8787".parse().unwrap()).unwrap();
        assert_eq!(
            cleanup_owned(&paths.pidfile, original, CleanupRule::Pid(999999)).unwrap(),
            CleanupOutcome::Kept
        );
        assert!(status(&paths).unwrap().unwrap().running);
    }

    #[test]
    fn cleanup_early_exit_preserves_replacement_inode() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let lock = lock_pidfile(&paths).unwrap();
        let original = lock.file.try_clone().unwrap();
        drop(lock);
        cleanup_stale(&paths).unwrap();
        let replacement = lock_pidfile(&paths).unwrap();
        drop(replacement);
        // Reaping is deterministic: the probe observes an already-exited child.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        child.wait().unwrap();
        assert!(matches!(
            confirm_started(&mut child, &paths, original),
            Err(ProxyError::DaemonExitedEarly { .. })
        ));
        assert!(paths.pidfile.exists());
    }

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
        // Reach unlink, rather than failing to create the sidecar directory entry.
        drop(PidfileGuard::namespace(&paths.pidfile).unwrap());
        let reached_unlink = std::rc::Rc::new(std::cell::Cell::new(false));
        let reached = reached_unlink.clone();
        CLEANUP_BEFORE_UNLINK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || reached.set(true)));
        });

        // Revoke write permission on the parent directory so unlink fails.
        let parent = paths.pidfile.parent().unwrap();
        let original = std::fs::metadata(parent).unwrap().permissions();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = cleanup_stale(&paths);

        // Restore permissions before asserting so the TempDir drop can clean up.
        std::fs::set_permissions(parent, original).unwrap();
        assert!(
            reached_unlink.get(),
            "fixture must reach the unlink operation"
        );

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
