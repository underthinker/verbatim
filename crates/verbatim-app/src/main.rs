//! The `verbatim` binary: CLI entry with headless parity from M1
//! (ARCHITECTURE.md 6); all components live in the `verbatim_app` library.
//!
//! The transport is a Unix domain socket on Unix and a named pipe on Windows
//! (`transport`); the wire protocol is byte-identical on both.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use verbatim_app::{client, daemon, gui, ipc, selftest};

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
    /// socket. This is the default when no subcommand is given, except a macOS
    /// app-bundle launch (Finder/Dock pass no args), which runs the GUI.
    Daemon,
    /// Run the Tauri shell: the daemon plus the desktop window (M2). CLI
    /// triggers keep working against it over the same socket.
    Gui,
    /// Control a running Verbatim instance (how native shortcut bindings
    /// drive dictation on GNOME, and how scripts integrate).
    Trigger {
        #[arg(value_enum)]
        verb: TriggerVerb,
    },
    /// Show the state of the running Verbatim instance.
    Status,
    /// Print the local dogfood counters (completed dictations, crash-free
    /// rate) for a tester report. Read from disk; no daemon needed. Never sent
    /// anywhere.
    Stats,
    /// Inject a sentinel string into the focused foreign app to verify the
    /// platform backend chain end-to-end (M1 acceptance, issue #18). Needs a
    /// `real-injection` build; confirm the sentinel lands to tick the box.
    InjectSelftest {
        /// Text to inject instead of the built-in sentinel.
        text: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum TriggerVerb {
    Start,
    Stop,
    Toggle,
}

/// What a bare `verbatim` runs: the headless daemon from the CLI (M1 parity),
/// but the desktop shell when launched from a macOS app bundle - Finder and
/// the Dock pass no arguments, and a double-click must open the product, not
/// park a headless process.
fn default_command() -> Command {
    #[cfg(target_os = "macos")]
    {
        let from_bundle = std::env::current_exe().is_ok_and(|exe| {
            exe.components()
                .any(|part| part.as_os_str().to_string_lossy().ends_with(".app"))
        });
        if from_bundle {
            return Command::Gui;
        }
    }
    Command::Daemon
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command.unwrap_or_else(default_command) {
        // The daemon may need to own the main thread's run loop (macOS global
        // hotkey), so it manages its own runtime rather than running under one.
        Command::Daemon => run_daemon(),
        // The webview event loop must own the process main thread.
        Command::Gui => {
            init_tracing();
            gui::run()
        }
        Command::Trigger { verb } => block_on(run_trigger(verb)),
        Command::Status => block_on(run_status()),
        Command::Stats => {
            print!(
                "{}",
                verbatim_app::stats::report(&verbatim_app::config::data_dir())
            );
            ExitCode::SUCCESS
        }
        Command::InjectSelftest { text } => {
            init_tracing();
            selftest::run(text)
        }
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
