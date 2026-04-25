use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    SymmetricPotemkin,
    ClassMapOverrideSafety,
    RecognizerCompositionValidator,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::SymmetricPotemkin => println!("symmetric_potemkin_gate: scaffolded"),
        Command::ClassMapOverrideSafety => println!("class_map_override_safety: scaffolded"),
        Command::RecognizerCompositionValidator => {
            println!("recognizer_composition_validator: scaffolded")
        }
    }
    Ok(())
}
