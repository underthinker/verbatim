//! The `verbatim` binary: CLI entry with headless parity from M1
//! (ARCHITECTURE.md 6). The Tauri shell joins in a later M1 phase.
//!
//! Security (ENGINEERING.md 8): the trigger IPC accepts trigger verbs only,
//! never text payloads; other processes must never be able to inject text
//! through us. The wire protocol enforces this - see `ipc.rs`.
//!
//! The transport is a Unix domain socket on Unix and a named pipe on Windows
//! (`transport.rs`); the wire protocol is byte-identical on both.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod client;
mod daemon;
mod ipc;
mod transport;

#[derive(Parser)]
#[command(
    name = "verbatim",
    version,
    about = "Local-first dictation with on-device polish"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the background instance: owns the session runner and the trigger
    /// socket. This is the default when no subcommand is given.
    Daemon,
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
    match cli.command.unwrap_or(Command::Daemon) {
        // The daemon may need to own the main thread's run loop (macOS global
        // hotkey), so it manages its own runtime rather than running under one.
        Command::Daemon => run_daemon(),
        Command::Trigger { verb } => block_on(run_trigger(verb)),
        Command::Status => block_on(run_status()),
    }
}

/// Run a short-lived async client to completion on a fresh runtime.
fn block_on<F: std::future::Future<Output = ExitCode>>(fut: F) -> ExitCode {
    match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(fut),
        Err(err) => {
            eprintln!("verbatim: runtime init failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_daemon() -> ExitCode {
    init_tracing();
    let events = std::sync::Arc::new(verbatim_core::event::EventBus::default());
    let path = ipc::socket_path();

    // On macOS the global hotkey is delivered only on the main thread's run
    // loop, so that path owns this thread and runs tokio on background workers.
    #[cfg(all(feature = "global-hotkey", target_os = "macos"))]
    {
        match daemon::serve_with_hotkey(&path, events) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("verbatim: daemon failed: {err}");
                ExitCode::FAILURE
            }
        }
    }

    #[cfg(not(all(feature = "global-hotkey", target_os = "macos")))]
    block_on(async move {
        match daemon::serve(&path, events).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("verbatim: daemon failed: {err}");
                ExitCode::FAILURE
            }
        }
    })
}

async fn run_trigger(verb: TriggerVerb) -> ExitCode {
    let verb = match verb {
        TriggerVerb::Start => ipc::Verb::Start,
        TriggerVerb::Stop => ipc::Verb::Stop,
        TriggerVerb::Toggle => ipc::Verb::Toggle,
    };
    client::run(&ipc::socket_path(), ipc::Request::Trigger(verb)).await
}

async fn run_status() -> ExitCode {
    client::run(&ipc::socket_path(), ipc::Request::Status).await
}

fn init_tracing() {
    // Best-effort: a daemon with no log sink is still functional.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .try_init();
}
