//! The `verbatim` binary: CLI entry with headless parity from M1
//! (ARCHITECTURE.md 6). The Tauri shell joins during M1 wire-up.
//!
//! Security (ENGINEERING.md 8): the trigger IPC accepts trigger verbs only,
//! never text payloads; other processes must never be able to inject text
//! through us.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "verbatim",
    version,
    about = "Local-first dictation with on-device polish"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Control a running Verbatim instance (how native shortcut bindings
    /// drive dictation on GNOME, and how scripts integrate).
    Trigger {
        #[arg(value_enum)]
        verb: TriggerVerb,
    },
    /// Show the state of the running Verbatim instance.
    Status,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum TriggerVerb {
    Start,
    Stop,
    Toggle,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Trigger { verb } => {
            eprintln!(
                "verbatim: cannot deliver `trigger {verb:?}`: no running instance (IPC lands later in M1)"
            );
            ExitCode::FAILURE
        }
        Command::Status => {
            eprintln!("verbatim: no running instance (IPC lands later in M1)");
            ExitCode::FAILURE
        }
    }
}
