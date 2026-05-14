use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        .arg("--bind")
        .arg(options.config.bind.to_string())
        .arg("--session-ttl")
        .arg(&options.config.session_ttl)
        .args(options.extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    drop(lock);
    let child = command.spawn().map_err(|source| ProxyError::DaemonIo {
        path: PathBuf::from("gaze proxy serve"),
        source,
    })?;
    std::thread::sleep(Duration::from_millis(250));
    if process_exists(child.id()) {
        Ok(child.id())
    } else {
        Err(ProxyError::DaemonNotRunning)
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
    if let Some(status) = status(paths)? {
        if !status.running {
            fs::remove_file(&paths.pidfile).map_err(|source| ProxyError::DaemonIo {
                path: paths.pidfile.clone(),
                source,
            })?;
        }
    }
    Ok(())
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
}
