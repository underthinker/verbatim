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

use tauri::{Emitter, Manager};

use verbatim_core::event::{Event, EventBus};
use verbatim_core::runner::{RunnerConfig, RunnerHandle, SessionRunner};
use verbatim_engines::fake::FakeModelDownloader;
use verbatim_platform::fake::{FakePermissionProbe, FakePermissionRequester};

use crate::bridge::{self, SessionStateDto, UiEvent};
use crate::config::OnboardingState;
use crate::history::{History, HistoryEntry};
use crate::models::{ManagedModel, ModelManager};
use crate::onboarding::{self, ModelInfo, Onboarding};
use crate::settings::Config;
use crate::{config, daemon, ipc, overlay, tray};

/// How many history rows the window lists (UX.md 7 reverse-chron pairs).
const HISTORY_LIMIT: u32 = 200;

struct Shell {
    handle: RunnerHandle,
}

/// The onboarding service, managed for the onboarding webview's commands.
struct OnboardingShell {
    service: Onboarding,
}

/// Read a capability's current permission state without prompting.
#[tauri::command]
fn onboarding_permission(
    state: tauri::State<'_, OnboardingShell>,
    capability: String,
) -> Result<String, String> {
    let cap = onboarding::parse_capability(&capability)
        .ok_or_else(|| format!("unknown capability: {capability}"))?;
    Ok(format!("{:?}", state.service.permission(cap)))
}

/// Trigger the OS permission prompt on user click, then return the re-checked
/// state (UX.md 6 steps 2-3).
#[tauri::command]
fn onboarding_request_permission(
    state: tauri::State<'_, OnboardingShell>,
    capability: String,
) -> Result<String, String> {
    let cap = onboarding::parse_capability(&capability)
        .ok_or_else(|| format!("unknown capability: {capability}"))?;
    Ok(format!("{:?}", state.service.request_permission(cap)))
}

/// Open the OS settings pane for a capability (deep link / re-check loop).
#[tauri::command]
fn onboarding_open_settings(
    state: tauri::State<'_, OnboardingShell>,
    capability: String,
) -> Result<(), String> {
    let cap = onboarding::parse_capability(&capability)
        .ok_or_else(|| format!("unknown capability: {capability}"))?;
    state.service.open_settings(cap);
    Ok(())
}

/// The recommended transcription model for the detected hardware.
#[tauri::command]
fn onboarding_recommended_model(state: tauri::State<'_, OnboardingShell>) -> ModelInfo {
    state.service.recommended_model()
}

/// The full model catalog (choose a different model).
#[tauri::command]
fn onboarding_catalog(state: tauri::State<'_, OnboardingShell>) -> Vec<ModelInfo> {
    state.service.catalog()
}

/// Download a model, streaming `DownloadProgress` on the event bridge; returns
/// the resolved on-disk path. An interrupted download surfaces as an error for
/// the webview to offer a resumable retry (E8).
#[tauri::command]
fn onboarding_download_model(
    state: tauri::State<'_, OnboardingShell>,
    model_id: String,
) -> Result<String, String> {
    state
        .service
        .download_model(&model_id)
        .map_err(|err| err.to_string())
}

/// Persist onboarding completion + the chosen models, then hand off to the main
/// window (UX.md 6). The window swap is done server-side so the webview needs no
/// window-management ACL permissions.
#[tauri::command]
fn onboarding_complete(
    app: tauri::AppHandle,
    transcription_model: Option<String>,
    polish_model: Option<String>,
) -> Result<(), String> {
    OnboardingState {
        completed: true,
        transcription_model,
        polish_model,
    }
    .save()
    .map_err(|err| err.to_string())?;

    if let Some(main) = app.get_webview_window("main") {
        main.show().map_err(|err| err.to_string())?;
    }
    if let Some(window) = app.get_webview_window(onboarding::WINDOW_LABEL) {
        window.close().map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// The persisted user config, for the Settings webview's initial render.
#[tauri::command]
fn settings_get() -> Config {
    Config::load()
}

/// Validate + persist a full config from the Settings webview (UX.md 7). The
/// hotkey is conflict-checked before anything is written; an invalid chord is
/// rejected whole so the file never holds a chord the runner cannot bind.
///
/// ponytail: this persists only - live re-apply of hotkey mode / model to the
/// running runner takes effect on next launch. Wire a runner reconfigure
/// command when in-session rebinding is needed.
#[tauri::command]
fn settings_set(config: Config) -> Result<(), String> {
    Config::validate_hotkey(&config.hotkey).map_err(|err| err.to_string())?;
    config.save().map_err(|err| err.to_string())
}

/// Validate a proposed hotkey chord without persisting (live conflict check as
/// the user types a rebind, UX.md 3).
#[tauri::command]
fn settings_validate_hotkey(chord: String) -> Result<(), String> {
    Config::validate_hotkey(&chord).map_err(|err| err.to_string())
}

/// The catalog with each model's installed state, size, and default flag.
#[tauri::command]
fn models_list(state: tauri::State<'_, ModelManager>) -> Vec<ManagedModel> {
    state.list()
}

/// Total bytes used by installed model files (disk-usage readout, UX.md 7).
#[tauri::command]
fn models_disk_usage(state: tauri::State<'_, ModelManager>) -> u64 {
    state.disk_usage()
}

/// Download a model, streaming `DownloadProgress` on the event bridge; returns
/// the resolved on-disk path. An interruption surfaces as an error for the UI
/// to offer a resumable retry (E8).
#[tauri::command]
fn models_download(
    state: tauri::State<'_, ModelManager>,
    model_id: String,
) -> Result<String, String> {
    state.download(&model_id).map_err(|err| err.to_string())
}

/// Delete an installed model; clears the default for its kind if it was set.
#[tauri::command]
fn models_delete(state: tauri::State<'_, ModelManager>, model_id: String) -> Result<(), String> {
    state.delete(&model_id).map_err(|err| err.to_string())
}

/// Set the default model for its kind (must be installed), persisted to config.
#[tauri::command]
fn models_set_default(
    state: tauri::State<'_, ModelManager>,
    model_id: String,
) -> Result<(), String> {
    state.set_default(&model_id).map_err(|err| err.to_string())
}

/// Recent dictation history, newest first (History surface, UX.md 7).
#[tauri::command]
fn history_list(state: tauri::State<'_, Arc<History>>) -> Result<Vec<HistoryEntry>, String> {
    state.list(HISTORY_LIMIT).map_err(|err| err.to_string())
}

/// Clear all history (single delete + VACUUM).
#[tauri::command]
fn history_clear(state: tauri::State<'_, Arc<History>>) -> Result<(), String> {
    state.clear().map_err(|err| err.to_string())
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

/// Open the on-disk history DB, degrading to an in-memory store if the data dir
/// or DB file cannot be opened - history is never allowed to block startup.
fn open_history() -> History {
    let dir = config::data_dir();
    let opened = std::fs::create_dir_all(&dir)
        .map_err(|err| tracing::warn!(?err, "history data dir create failed"))
        .ok()
        .and_then(|()| {
            History::open(&dir.join("history.db"))
                .map_err(|err| tracing::error!(?err, "history db open failed"))
                .ok()
        });
    match opened {
        Some(history) => history,
        None => {
            tracing::warn!("history running in-memory this session");
            // Startup path: an in-memory SQLite open cannot realistically fail;
            // expect is permitted here per the coding standard (startup only).
            #[allow(clippy::expect_used)]
            History::open_in_memory().expect("in-memory history")
        }
    }
}

/// Subscribe to the bus and persist every `DictationRecorded` to history, using
/// the live retention setting (retention `0` = off, written as a no-op).
fn spawn_history_recorder(history: Arc<History>, events: &EventBus) {
    let mut receiver = events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(Event::DictationRecorded {
                    app_id,
                    raw,
                    polished,
                    ..
                }) => {
                    let retention = Config::load().history_retention_days;
                    if let Err(err) = history.record(&app_id, &raw, polished.as_deref(), retention)
                    {
                        tracing::warn!(?err, "history record failed");
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "history recorder lagged");
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

    // Onboarding service (Phase D). Permission and download backends are the
    // deterministic fakes for now; the real per-OS impls and the hash-verified
    // network downloader land later (behind feature flags / Phase E).
    let onboarding = Onboarding::new(
        Arc::new(FakePermissionProbe::default()),
        Arc::new(FakePermissionRequester::new(Arc::new(
            FakePermissionProbe::default(),
        ))),
        Arc::new(FakeModelDownloader::default()),
        onboarding::detect_hardware(),
        Arc::clone(&events),
    );
    let first_run = !OnboardingState::load().completed;

    // History store (Phase E-3). Best-effort open: a bad data dir degrades to an
    // in-memory store rather than blocking startup.
    let history = Arc::new(open_history());
    spawn_history_recorder(Arc::clone(&history), &events);

    // Handle + history clones the tray (Phase E-4) owns; it drives live
    // pause/resume and lists recent dictations from the same stores.
    let tray_handle = handle.clone();
    let tray_history = Arc::clone(&history);

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Shell { handle })
        .manage(OnboardingShell {
            service: onboarding,
        })
        .manage(ModelManager::new(
            Arc::new(FakeModelDownloader::default()),
            Arc::clone(&events),
        ))
        .manage(Arc::clone(&history))
        .invoke_handler(tauri::generate_handler![
            trigger,
            session_state,
            settings_get,
            settings_set,
            settings_validate_hotkey,
            models_list,
            models_disk_usage,
            models_download,
            models_delete,
            models_set_default,
            history_list,
            history_clear,
            onboarding_permission,
            onboarding_request_permission,
            onboarding_open_settings,
            onboarding_recommended_model,
            onboarding_catalog,
            onboarding_download_model,
            onboarding_complete,
        ])
        .setup(move |app| {
            spawn_event_bridge(app.handle().clone(), &events);
            // Overlay (Phase B): created hidden so ARMING can show it within
            // the < 50 ms budget; driven straight from the Rust bus.
            overlay::create_window(app.handle())?;
            overlay::spawn_driver(app.handle().clone(), &events);
            // Cross-platform tray (Phase E-4): a direct bus consumer like the
            // overlay; menu actions drive the same runner/config/history.
            tray::create(app.handle(), tray_handle, tray_history)?;
            tray::spawn_driver(app.handle().clone(), &events);
            // First run (Phase D): open onboarding instead of the main window;
            // the main window is declared hidden and shown once onboarding is
            // done (or immediately on a returning launch).
            if first_run {
                onboarding::create_window(app.handle())?;
            } else if let Some(main) = app.get_webview_window("main") {
                main.show()?;
            }
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
