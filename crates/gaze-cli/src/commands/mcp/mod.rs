mod doctor;
mod install;
mod serve;

use std::path::PathBuf;

use clap::ValueEnum;

use crate::error::CliError;

#[derive(ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    All,
}

#[derive(Debug)]
pub(crate) struct InstallArgs {
    pub client: Client,
    pub agents_md: Option<PathBuf>,
    pub dry_run: bool,
    pub skip_agents_md: bool,
}

#[derive(Debug)]
pub(crate) struct DoctorArgs {
    pub agents_md: Option<PathBuf>,
    pub strict: bool,
    pub json: bool,
}

#[derive(Debug)]
pub(crate) struct ServeArgs {
    pub manifest_dir: Option<PathBuf>,
    pub max_file_size: Option<u64>,
}

pub(crate) fn install(args: InstallArgs) -> Result<(), CliError> {
    install::run(args)
}

pub(crate) fn doctor(args: DoctorArgs) -> Result<(), CliError> {
    doctor::run(args)
}

pub(crate) fn serve(args: ServeArgs) -> Result<(), CliError> {
    serve::run(args)
}

#[allow(dead_code)]
fn default_agents_md() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("AGENTS.md")
}
