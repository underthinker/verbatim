//! The Tauri 2 shell (`verbatim gui`, M2 Phase A).
//!
//! Layering (ARCHITECTURE.md 1): the webview talks only to thin Tauri commands
//! that drive the same `RunnerHandle` the CLI daemon uses - every command a
//! headless client could also invoke over the trigger socket. No engine or
//! platform calls exist on this surface.
//!
//! The shell *is* the daemon: it boots the runner and serves the trigger
//! socket, so `verbatim trigger`/`status` and native shortcut bindings keep
//! working unchanged while the window is open. The core `EventBus` is
//! forwarded to the webview event system 1:1 through `bridge::UiEvent`; the
//! overlay and tray (Phase B/E) will subscribe to the Rust bus directly
//! instead - no webview round-trip on the hot path.

use std::process::ExitCode;
use std::sync::Arc;

use tauri::Emitter;

use verbatim_core::event::EventBus;
use verbatim_core::runner::{RunnerConfig, RunnerHandle, SessionRunner};

use crate::bridge::{self, SessionStateDto, UiEvent};
use crate::{daemon, ipc};

struct Shell {
    handle: RunnerHandle,
}

/// Trigger dictation from the webview. The verb set is the same closed set
/// the IPC socket accepts (`ipc::Request::parse`); anything else is rejected
/// before interpretation, mirroring the wire-protocol posture.
#[tauri::command]
async fn trigger(state: tauri::State<'_, Shell>, verb: String) -> Result<SessionStateDto, String> {
    let verb = match ipc::Request::parse(&format!("{verb}\n")) {
        Ok(ipc::Request::Trigger(verb)) => verb,
        Ok(ipc::Request::Status) | Err(_) => return Err(format!("unrecognized verb: {verb}")),
    };
    state
        .handle
        .trigger(verb.to_trigger())
        .await
        .map_err(|_| "runner stopped".to_owned())?;
    session_state(state).await
}

/// Current session state, for initial render before any event arrives.
#[tauri::command]
async fn session_state(state: tauri::State<'_, Shell>) -> Result<SessionStateDto, String> {
    state
        .handle
        .status()
        .await
        .map(|status| status.state.into())
        .map_err(|_| "runner stopped".to_owned())
}

/// Forward every core bus event to the webview, 1:1 (ARCHITECTURE.md 4.9).
/// A lagged receiver skips to the live edge: surfaces replay events and any
/// missed transition is superseded by the next one.
fn spawn_event_bridge(app: tauri::AppHandle, events: &EventBus) {
    let mut receiver = events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Err(err) = app.emit(bridge::EVENT_CHANNEL, UiEvent::from(event)) {
                        tracing::warn!(?err, "event bridge emit failed");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "event bridge lagged; skipping to live edge");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Run the shell on the calling thread, which must be the process main thread
/// (the webview event loop owns it, like the macOS hotkey run loop does for
/// the headless daemon).
pub fn run() -> ExitCode {
    let events = Arc::new(EventBus::default());
    let (runner, handle) = SessionRunner::new(daemon::build_deps(), RunnerConfig::default(), {
        Arc::clone(&events)
    });
    tauri::async_runtime::spawn(runner.run());

    // Keep the trigger socket alive so CLI triggers and native shortcut
    // bindings drive the same runner the window shows.
    {
        let handle = handle.clone();
        let path = ipc::socket_path();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = daemon::serve_with_handle(&path, handle).await {
                tracing::error!(?err, "trigger socket failed; webview commands still work");
            }
        });
    }

    let result = tauri::Builder::default()
        .manage(Shell { handle })
        .invoke_handler(tauri::generate_handler![trigger, session_state])
        .setup(move |app| {
            spawn_event_bridge(app.handle().clone(), &events);
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("verbatim: shell failed: {err}");
            ExitCode::FAILURE
        }
    }
}
