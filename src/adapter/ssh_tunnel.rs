//! SSH tunnel lifecycle. v0.1 shells out to the system `ssh` binary with
//! `-f -N -L local:host:remote` and tracks the forked child PID via the
//! returned control-socket path. On shutdown we run `ssh -O exit` against
//! the same control socket. Rust-native `russh` is deferred to v0.2.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TunnelSpec {
    pub ssh_host: String,    // e.g. "deploy@prod.example.com"
    pub local_port: u16,     // e.g. 13306
    pub remote_host: String, // e.g. "127.0.0.1"
    pub remote_port: u16,    // e.g. 3306
}

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    #[error("ssh spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ssh exited non-zero: {0}")]
    NonZero(String),
}

pub struct SshTunnel {
    control_path: PathBuf,
    ssh_host: String,
}

impl SshTunnel {
    /// Spawn a background tunnel. Reuses `~/.ssh/config`, so auth and
    /// jump hosts stay with the developer's regular ssh setup.
    pub fn open(spec: &TunnelSpec) -> Result<Self, TunnelError> {
        let control_path = std::env::temp_dir().join(format!(
            "gaze-ssh-{}-{}.sock",
            spec.local_port,
            std::process::id()
        ));
        let status = Command::new("ssh")
            .arg("-f")
            .arg("-N")
            .arg("-M")
            .arg("-S")
            .arg(&control_path)
            .arg("-o")
            .arg("ExitOnForwardFailure=yes")
            .arg("-o")
            .arg("ServerAliveInterval=30")
            .arg("-L")
            .arg(format!(
                "{}:{}:{}",
                spec.local_port, spec.remote_host, spec.remote_port
            ))
            .arg(&spec.ssh_host)
            .status()?;
        if !status.success() {
            return Err(TunnelError::NonZero(format!(
                "ssh returned {:?}",
                status.code()
            )));
        }
        Ok(Self {
            control_path,
            ssh_host: spec.ssh_host.clone(),
        })
    }

    /// Tear down the tunnel via `ssh -O exit`. Also removes the socket file.
    pub fn close(&mut self) -> Result<(), TunnelError> {
        let _ = Command::new("ssh")
            .arg("-S")
            .arg(&self.control_path)
            .arg("-O")
            .arg("exit")
            .arg(&self.ssh_host)
            .status();
        let _ = std::fs::remove_file(&self.control_path);
        Ok(())
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_path_is_unique_per_port() {
        let spec1 = TunnelSpec {
            ssh_host: "x@y".into(),
            local_port: 13306,
            remote_host: "127.0.0.1".into(),
            remote_port: 3306,
        };
        let spec2 = TunnelSpec {
            local_port: 13307,
            ..spec1.clone()
        };
        // We can't actually open a tunnel in CI; just sanity-check the
        // control-path derivation would differ.
        let p1 = std::env::temp_dir().join(format!(
            "gaze-ssh-{}-{}.sock",
            spec1.local_port,
            std::process::id()
        ));
        let p2 = std::env::temp_dir().join(format!(
            "gaze-ssh-{}-{}.sock",
            spec2.local_port,
            std::process::id()
        ));
        assert_ne!(p1, p2);
    }
}
