use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TunnelSpec {
    pub ssh_host: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
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
    pub fn open(spec: &TunnelSpec) -> Result<Self, TunnelError> {
        let control_path = Self::control_path(spec.local_port);
        let status = Command::new("ssh")
            .arg("-f")
            .arg("-N")
            .arg("-M")
            .arg("-S")
            .arg(&control_path)
            .arg("-o")
            .arg("ExitOnForwardFailure=yes")
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

    pub fn control_path(local_port: u16) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gaze-ssh-{}-{}.sock",
            local_port,
            std::process::id()
        ))
    }

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
    fn control_path_varies_by_port() {
        let left = SshTunnel::control_path(13306);
        let right = SshTunnel::control_path(13307);
        assert_ne!(left, right);
    }
}
