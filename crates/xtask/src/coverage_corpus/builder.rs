use anyhow::{bail, Result};
use clap::Parser;

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(long)]
    regenerate: bool,
}

pub fn run(args: Args) -> Result<()> {
    if !args.regenerate {
        bail!("coverage-corpus requires --regenerate");
    }

    println!("Phase 0 stub: coverage corpus builder not implemented yet");
    Ok(())
}
