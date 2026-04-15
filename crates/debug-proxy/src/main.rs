use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "debug-proxy")]
#[command(about = "MCP debug proxy backed by the Gaze v0.2 core")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    Check {
        #[arg(default_value = "policy.toml")]
        policy: PathBuf,
    },
    Serve {
        #[arg(default_value = "policy.toml")]
        policy: PathBuf,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Init { dir } => debug_proxy::cli::init::run(&dir).map_err(|err| err.to_string()),
        Command::Check { policy } => {
            let summary = debug_proxy::cli::check::run(&policy).map_err(|err| err.to_string())?;
            println!("{summary}");
            Ok(())
        }
        Command::Serve { policy } => debug_proxy::cli::serve::run_cmd(&policy)
            .await
            .map_err(|err| err.to_string()),
    }
}
