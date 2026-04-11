use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gaze", version, about = "GDPR-compliant debugging proxy")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Use the global audit log at ~/.gaze/audit.db instead of ./.gaze/audit.db
    #[arg(long, global = true)]
    global: bool,

    /// Allow running without mlock (containers, restricted envs).
    #[arg(long, global = true)]
    allow_unlocked_key: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new Gaze project in the current directory.
    Init,
    /// Parse and validate policy.toml.
    Check {
        #[arg(default_value = "policy.toml")]
        policy: PathBuf,
    },
    /// Start the MCP stdio server.
    Serve {
        #[arg(default_value = "policy.toml")]
        policy: PathBuf,
    },
    /// Print audit log entries.
    Audit,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Init => match gaze::cli::init::run(&std::env::current_dir().unwrap()) {
            Ok(()) => {
                println!("gaze: initialized");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("gaze init: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Check { policy } => match gaze::cli::check::run(&policy) {
            Ok(out) => {
                println!("{out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("gaze check: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Serve { policy: _ } => match gaze::cli::serve::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("gaze serve: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Audit => match gaze::cli::audit::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("gaze audit: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
