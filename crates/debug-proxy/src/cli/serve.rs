use std::path::Path;

pub use crate::mcp::server::ServerError;
use crate::policy::{build_pipeline, ConnectionConfig, PolicyError, PolicyFile};

pub struct PreparedServe {
    pub policy: PolicyFile,
    pub connection: ConnectionConfig,
    pub password: String,
    pub pipeline: gaze::Pipeline,
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error("missing env var `{0}` for connection password")]
    MissingPasswordEnv(String),
}

pub fn prepare(policy_path: &Path) -> Result<PreparedServe, ServeError> {
    let text = std::fs::read_to_string(policy_path).map_err(|source| ServeError::Io {
        path: policy_path.display().to_string(),
        source,
    })?;
    let policy = PolicyFile::from_toml(&text)?;
    let connection = policy
        .connection
        .get("production")
        .expect("validated production connection")
        .clone();
    let password = std::env::var(&connection.password_env)
        .map_err(|_| ServeError::MissingPasswordEnv(connection.password_env.clone()))?;
    let pipeline = build_pipeline(&policy)?;

    Ok(PreparedServe {
        policy,
        connection,
        password,
        pipeline,
    })
}

pub fn mysql_url(connection: &ConnectionConfig, password: &str) -> String {
    format!(
        "mysql://{}:{}@127.0.0.1:{}/{}",
        connection.user, password, connection.local_port, connection.database
    )
}

pub async fn run_cmd(policy_path: &Path) -> Result<(), ServerError> {
    crate::mcp::server::run(policy_path).await
}
