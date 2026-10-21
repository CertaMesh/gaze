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

    /// The readiness marker next to the pidfile (`proxy.ready` for `proxy.pid`).
    ///
    /// The daemon child publishes it only after it has bound its port, and
    /// `start` reports success only once it names the child it spawned.
    pub fn ready_marker(&self) -> PathBuf {
        self.pidfile.with_extension("ready")
    }
}

/// How long `start` waits for the daemon child to bind before giving up.
///
/// Generous on purpose: loading a large NER or safety-net model can take a
/// while on a loaded host, and the cap exists only so `start` never hangs.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(120);

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
    /// Cap on waiting for the readiness marker; see [`DEFAULT_READY_TIMEOUT`].
    pub ready_timeout: Duration,
}

impl StartOptions {
    pub fn new(paths: DaemonPaths, config: DaemonConfig) -> Self {
        Self {
            paths,
            config,
            extra_args: Vec::new(),
            ready_timeout: DEFAULT_READY_TIMEOUT,
        }
    }

    pub fn with_ready_timeout(mut self, ready_timeout: Duration) -> Self {
        self.ready_timeout = ready_timeout;
        self
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
/// `--rulepack`, and the adapter upstreams went missing.
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
            // No daemon is running and this start holds the lifecycle lock, so
            // any marker left behind is from an earlier run and never trusted.
            remove_ready_marker(&options.paths, |_| true)?;
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
    confirm_started(
        &mut child,
        &options.paths,
        owned_file,
        options.ready_timeout,
    )
}

/// Waits until the spawned child has bound its port, then returns its pid.
///
/// Readiness is the marker the child publishes after its bind
/// ([`mark_ready`]), and only a marker naming this child counts: `start`
/// removes any earlier marker before spawning, and a marker for another pid is
/// ignored. A liveness window alone reported success before the port was bound
/// and missed a daemon that died later in startup, such as while loading a model.
///
/// `try_wait` reaps the child, so an exit at any point before readiness is
/// reported as [`ProxyError::DaemonExitedEarly`]; `kill(pid, 0)` would count an
/// unreaped zombie as alive. A child that neither binds nor exits within
/// `timeout` is killed and reported as [`ProxyError::DaemonStartTimeout`].
/// Both failures remove the child's pidfile and marker.
fn confirm_started(
    child: &mut Child,
    paths: &DaemonPaths,
    owned_file: File,
    timeout: Duration,
) -> Result<u32, ProxyError> {
    let pid = child.id();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                discard_failed_start(paths, pid, owned_file)?;
                return Err(ProxyError::DaemonExitedEarly {
                    code: status.code(),
                    stderr_file: paths.stderr_file.clone(),
                });
            }
            Ok(None) => {}
            Err(source) => {
                return Err(ProxyError::DaemonIo {
                    path: PathBuf::from("gaze proxy serve"),
                    source,
                });
            }
        }
        if read_ready_marker(paths)?.is_some_and(|marker| marker.pid == pid) {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            // Kill and reap so the pidfile's pid is provably dead before cleanup.
            let _ = child.kill();
            let _ = child.wait();
            discard_failed_start(paths, pid, owned_file)?;
            return Err(ProxyError::DaemonStartTimeout {
                timeout,
                stderr_file: paths.stderr_file.clone(),
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn discard_failed_start(paths: &DaemonPaths, pid: u32, owned_file: File) -> Result<(), ProxyError> {
    remove_owned_ready_marker(paths, pid)?;
    cleanup_owned(&paths.pidfile, owned_file, CleanupRule::Stale)?;
    Ok(())
}

/// Publishes the readiness marker for this process once it has bound `bind`.
///
/// The marker is written to a private temporary file and renamed into place,
/// so a reader sees either no marker or a complete one. On Unix it is created
/// owner-only (`0600`).
pub fn mark_ready(paths: &DaemonPaths, bind: SocketAddr) -> Result<(), ProxyError> {
    let marker = paths.ready_marker();
    create_parent(&marker)?;
    let pid = std::process::id();
    let mut name = marker.as_os_str().to_os_string();
    name.push(format!(".{pid}.tmp"));
    let tmp = PathBuf::from(name);
    let _ = fs::remove_file(&tmp);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut file| {
        use std::io::Write;
        file.write_all(format!("{pid}\nbind={bind}\n").as_bytes())?;
        file.sync_all()
    });
    if let Err(source) = written.and_then(|()| fs::rename(&tmp, &marker)) {
        let _ = fs::remove_file(&tmp);
        return Err(daemon_io(&marker, source));
    }
    Ok(())
}

struct ReadyMarker {
    pid: u32,
}

/// Reads the readiness marker. A missing marker is `None`; an unparseable one
/// is treated as missing because only a complete marker naming a pid counts.
fn read_ready_marker(paths: &DaemonPaths) -> Result<Option<ReadyMarker>, ProxyError> {
    let marker = paths.ready_marker();
    let text = match fs::read_to_string(&marker) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(daemon_io(&marker, source)),
    };
    Ok(text
        .lines()
        .next()
        .and_then(|line| line.parse::<u32>().ok())
        .map(|pid| ReadyMarker { pid }))
}

/// Removes the readiness marker when `permits` accepts what it records.
fn remove_ready_marker(
    paths: &DaemonPaths,
    permits: impl FnOnce(Option<&ReadyMarker>) -> bool,
) -> Result<(), ProxyError> {
    let marker = paths.ready_marker();
    if !marker.exists() || !permits(read_ready_marker(paths)?.as_ref()) {
        return Ok(());
    }
    match fs::remove_file(&marker) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(daemon_io(&marker, source)),
    }
}

/// Removes the marker of daemon `pid`, or one whose recorded pid is dead.
fn remove_owned_ready_marker(paths: &DaemonPaths, pid: u32) -> Result<(), ProxyError> {
    remove_ready_marker(paths, |marker| {
        marker.is_none_or(|marker| marker.pid == pid || !process_exists(marker.pid))
    })
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
            remove_owned_ready_marker(&options.paths, status.pid)?;
            if let Some(file) = owned_file {
                cleanup_owned(&options.paths.pidfile, file, CleanupRule::Pid(status.pid))?;
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if options.force {
        terminate(status.pid, true)?;
        remove_owned_ready_marker(&options.paths, status.pid)?;
        if let Some(file) = owned_file {
            cleanup_owned(&options.paths.pidfile, file, CleanupRule::Pid(status.pid))?;
        }
        Ok(())
    } else {
        Err(ProxyError::DaemonStopTimeout {
            pid: status.pid,
            timeout: options.timeout,
        })
    }
}

pub fn restart(options: StartOptions, timeout: Duration, force: bool) -> Result<u32, ProxyError> {
    let stop_result = stop(StopOptions {
        paths: options.paths.clone(),
        timeout,
        force,
    });
    match stop_result {
        Ok(()) | Err(ProxyError::DaemonNotRunning) => start(options),
        Err(error) => Err(error),
    }
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
    remove_ready_marker(paths, |marker| {
        marker.is_none_or(|marker| !process_exists(marker.pid))
    })?;
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
    _namespace: NamespaceLock,
}

struct NamespaceLock(File);

impl Drop for NamespaceLock {
    fn drop(&mut self) {
        // A concurrent process spawn can briefly inherit a duplicate descriptor.
        // Closing ours alone would leave its shared flock alive until exec.
        let _ = FileExt::unlock(&self.0);
    }
}

impl Drop for PidfileGuard {
    fn drop(&mut self) {
        // A failed startup retains a cloned descriptor as its identity witness.
        // Explicit unlock releases the shared lock before the child takes over.
        let _ = FileExt::unlock(&self.file);
    }
}

impl PidfileGuard {
    fn namespace(path: &Path) -> std::io::Result<NamespaceLock> {
        let mut name = path.as_os_str().to_os_string();
        name.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(name))?;
        file.try_lock_exclusive()?;
        Ok(NamespaceLock(file))
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
    fn cleanup_releases_namespace_with_inherited_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let lock = lock_pidfile(&paths).unwrap();
        // Deterministic equivalent of a descriptor inherited during fork/exec.
        let inherited = lock._namespace.0.try_clone().unwrap();
        drop(lock);
        let next = lock_pidfile(&paths).unwrap();
        drop(next);
        drop(inherited);
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
            confirm_started(&mut child, &paths, original, DEFAULT_READY_TIMEOUT),
            Err(ProxyError::DaemonExitedEarly { .. })
        ));
        assert!(paths.pidfile.exists());
    }

    const READY_FIXTURE_MODE: &str = "GAZE_PROXY_READY_FIXTURE_MODE";
    const READY_FIXTURE_DIR: &str = "GAZE_PROXY_READY_FIXTURE_DIR";

    /// Stand-in for the detached daemon child, re-executed from this test
    /// binary by [`spawn_ready_fixture`]. In an ordinary test run it does nothing.
    ///
    /// Like the real child it first publishes its pidfile, then spends longer
    /// than the old 250 ms liveness window "loading" before it either binds and
    /// publishes the readiness marker, dies, or hangs.
    #[test]
    fn ready_fixture_child() {
        let Ok(mode) = std::env::var(READY_FIXTURE_MODE) else {
            return;
        };
        let dir = PathBuf::from(std::env::var(READY_FIXTURE_DIR).unwrap());
        let paths = paths_in(&dir);
        init_foreground_daemon(&paths, "127.0.0.1:0".parse().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(750));
        match mode.as_str() {
            "bind-late" => {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                mark_ready(&paths, listener.local_addr().unwrap()).unwrap();
                for stream in listener.incoming() {
                    drop(stream);
                }
            }
            "die-late" => std::process::exit(3),
            "never-ready" => loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            other => panic!("unknown fixture mode {other}"),
        }
    }

    /// Kills the fixture however the test ends.
    struct FixtureChild(Child);

    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Mirrors `start`: lock the pidfile, keep a clone as the ownership
    /// witness, release the lock, then spawn the fixture daemon child.
    fn spawn_ready_fixture(dir: &tempfile::TempDir, mode: &str) -> (FixtureChild, File) {
        let paths = temp_paths(dir);
        let lock = lock_pidfile(&paths).unwrap();
        let owned = lock.file.try_clone().unwrap();
        drop(lock);
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::tests::ready_fixture_child",
                "--test-threads=1",
            ])
            .env(READY_FIXTURE_MODE, mode)
            .env(READY_FIXTURE_DIR, dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        (FixtureChild(child), owned)
    }

    fn marker_bind(paths: &DaemonPaths) -> SocketAddr {
        let text = fs::read_to_string(paths.ready_marker()).unwrap();
        text.lines()
            .find_map(|line| line.strip_prefix("bind="))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn start_waits_for_a_bind_slower_than_the_old_liveness_window() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let (mut child, owned) = spawn_ready_fixture(&dir, "bind-late");

        let pid = confirm_started(&mut child.0, &paths, owned, DEFAULT_READY_TIMEOUT).unwrap();

        assert_eq!(pid, child.0.id());
        assert!(
            paths.ready_marker().exists(),
            "start reported success before the daemon published readiness"
        );
        // The first request after a successful start must reach the daemon.
        std::net::TcpStream::connect(marker_bind(&paths)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn start_reports_a_daemon_that_dies_after_the_old_liveness_window() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let (mut child, owned) = spawn_ready_fixture(&dir, "die-late");

        let result = confirm_started(&mut child.0, &paths, owned, DEFAULT_READY_TIMEOUT);

        assert!(
            matches!(
                result,
                Err(ProxyError::DaemonExitedEarly { code: Some(3), .. })
            ),
            "{result:?}"
        );
        assert!(!paths.pidfile.exists(), "a failed start left its pidfile");
        assert!(!paths.ready_marker().exists());
    }

    #[cfg(unix)]
    #[test]
    fn start_never_trusts_a_marker_from_another_process() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        // A live but unrelated pid: this test process.
        mark_ready(&paths, "127.0.0.1:9".parse().unwrap()).unwrap();
        let (mut child, owned) = spawn_ready_fixture(&dir, "die-late");

        let result = confirm_started(&mut child.0, &paths, owned, DEFAULT_READY_TIMEOUT);

        assert!(
            matches!(result, Err(ProxyError::DaemonExitedEarly { .. })),
            "a marker naming another process was taken as readiness: {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn start_stops_a_daemon_that_never_becomes_ready() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let (mut child, owned) = spawn_ready_fixture(&dir, "never-ready");
        let timeout = Duration::from_secs(2);

        let result = confirm_started(&mut child.0, &paths, owned, timeout);

        assert!(
            matches!(result, Err(ProxyError::DaemonStartTimeout { timeout: found, .. }) if found == timeout),
            "{result:?}"
        );
        assert!(
            child.0.try_wait().unwrap().is_some(),
            "the unready daemon was left running"
        );
        assert!(!paths.pidfile.exists(), "a failed start left its pidfile");
    }

    #[test]
    fn start_removes_a_leftover_marker_before_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        mark_ready(&paths, "127.0.0.1:9".parse().unwrap()).unwrap();
        // Fail the spawn after the lock is taken, so no child can write one.
        fs::create_dir(&paths.stderr_file).unwrap();

        let result = start(StartOptions::new(paths.clone(), DaemonConfig::default()));

        assert!(matches!(result, Err(ProxyError::DaemonIo { .. })));
        assert!(!paths.ready_marker().exists());
    }

    #[test]
    fn cleanup_stale_removes_a_dead_daemons_marker_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        fs::write(paths.ready_marker(), "999999\nbind=127.0.0.1:9\n").unwrap();
        cleanup_stale(&paths).unwrap();
        assert!(!paths.ready_marker().exists());

        mark_ready(&paths, "127.0.0.1:9".parse().unwrap()).unwrap();
        cleanup_stale(&paths).unwrap();
        assert!(
            paths.ready_marker().exists(),
            "removed a live daemon's marker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ready_marker_is_owner_only_and_names_the_bound_address() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let bind: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        mark_ready(&paths, bind).unwrap();
        let mode = fs::metadata(paths.ready_marker())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            fs::read_to_string(paths.ready_marker()).unwrap(),
            format!("{}\nbind={bind}\n", std::process::id())
        );
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "temp file left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_removes_the_stopped_daemons_marker() {
        let dir = tempfile::tempdir().unwrap();
        let paths = temp_paths(&dir);
        let child = FixtureChild(
            Command::new("sleep")
                .arg("60")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = child.0.id();
        // Publish the pidfile and marker as a ready daemon would.
        write_pidfile(
            &paths.pidfile,
            pid,
            "127.0.0.1:9".parse().unwrap(),
            Utc::now(),
        )
        .unwrap();
        fs::write(paths.ready_marker(), format!("{pid}\nbind=127.0.0.1:9\n")).unwrap();

        stop(StopOptions::new(paths.clone(), Duration::ZERO, true)).unwrap();

        assert!(!paths.ready_marker().exists());
        drop(child);
    }

    fn temp_paths(dir: &tempfile::TempDir) -> DaemonPaths {
        paths_in(dir.path())
    }

    fn paths_in(dir: &Path) -> DaemonPaths {
        DaemonPaths {
            pidfile: dir.join("proxy.pid"),
            config: dir.join("proxy.toml"),
            log_file: dir.join("proxy.log"),
            stderr_file: dir.join("proxy-stderr.log"),
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

    // Every field the adopter can configure has to reach the
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
