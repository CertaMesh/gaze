#![cfg_attr(docsrs, feature(doc_cfg))]

//! Gaze CLI — pipe-mode `clean` / `restore` for LLM-facing integrations.
//!
//! See the changelog for shipped integration history.

use std::process::ExitCode;

mod clean_overrides;
mod commands;
mod error;
mod io;
mod logger;
mod pipeline;
mod restore;
mod style;

use clap::Parser;

use crate::commands::Cli;
use crate::error::CliError;

fn main() -> ExitCode {
    logger::install_panic_hook();

    #[cfg(feature = "setup")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("setup")) {
        let args = std::env::args_os().collect::<Vec<_>>();
        if args
            .windows(2)
            .any(|pair| pair[0] == "--safety-net" && pair[1] == "ner")
            || args.iter().any(|arg| arg == "--safety-net=ner")
        {
            let err = CliError::SetupDetail(
                "`--safety-net ner` was removed; use `--safety-net none` for the former NER-only policy".to_string(),
            );
            err.emit_stderr();
            return ExitCode::from(err.exit_code());
        }
    }

    // Test-only panic trigger. Lets the integration suite prove the panic
    // hook sanitizes stderr under `RUST_BACKTRACE=1`. Gated by an env var
    // so no production invocation can stumble into it.
    if std::env::var_os("GAZE_TEST_PANIC").is_some() {
        panic!("gaze test-only panic trigger");
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            // --help and --version are surfaced by clap as Err variants whose
            // intent is "print info to stdout and exit 0"; they are not argv
            // failures and must bypass the sanitizer so `gaze --version`
            // prints the crate version cleanly (required by the homebrew test
            // block).
            use clap::error::ErrorKind;
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = err.print();
                return ExitCode::SUCCESS;
            }
            // Real argv errors: clap's default handler would dump usage text
            // to stderr before our sanitizer runs. Route them through the
            // standard stderr line so the host wrapper can parse a variant
            // even on malformed argv.
            CliError::PolicyConfig.emit_stderr();
            return ExitCode::from(CliError::PolicyConfig.exit_code());
        }
    };

    match commands::dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            err.emit_stderr();
            ExitCode::from(err.exit_code())
        }
    }
}
