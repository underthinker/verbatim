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
use verbatim_core::runner::{RunnerHandle, SessionRunner};
use verbatim_platform::AccessibilityAnnouncer;
#[cfg(not(all(
    feature = "real-injection",
    any(target_os = "macos", target_os = "linux")
)))]
use verbatim_platform::fake::FakeAnnouncer;
#[cfg(not(feature = "real-injection"))]
use verbatim_platform::fake::{FakePermissionProbe, FakePermissionRequester};
#[cfg(feature = "real-injection")]
use verbatim_platform::{Capability, PermissionRequestError};
use verbatim_platform::{PermissionProbe, PermissionRequest};

use crate::bridge::{self, SessionStateDto, UiEvent};
use crate::config::OnboardingState;
use crate::history::{History, HistoryEntry};
use crate::models::{ManagedModel, ModelManager};
use crate::onboarding::{self, ModelInfo, Onboarding};
use crate::settings::Config;
use crate::{config, daemon, ipc, overlay, tray};

/// How many history rows the window lists (UX.md 7 reverse-chron pairs).
const HISTORY_LIMIT: u32 = 200;

/// The published end-user docs (`docs/site`, deployed by `.github/workflows/docs.yml`).
const DOCS_URL: &str = "https://underthinker.github.io/verbatim/";

/// User-initiated permission grants are completed in the OS settings UI. The
/// read-only platform probe supplies the live state while onboarding polls.
#[cfg(feature = "real-injection")]
struct SystemPermissionRequester;

#[cfg(feature = "real-injection")]
impl PermissionRequest for SystemPermissionRequester {
    fn request(&self, capability: Capability) -> Result<(), PermissionRequestError> {
        self.open_settings(capability)
    }

    fn open_settings(&self, capability: Capability) -> Result<(), PermissionRequestError> {
        let url = permission_settings_url(capability)
            .ok_or(PermissionRequestError::Unsupported(capability))?;
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|error| PermissionRequestError::Backend(error.to_string()))
    }
}

#[cfg(all(feature = "real-injection", target_os = "macos"))]
fn permission_settings_url(capability: Capability) -> Option<&'static str> {
    match capability {
        Capability::Microphone => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        Capability::TextInjection | Capability::InputMonitoring => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        }
    }
}

#[cfg(all(feature = "real-injection", target_os = "windows"))]
fn permission_settings_url(capability: Capability) -> Option<&'static str> {
    match capability {
        Capability::Microphone => Some("ms-settings:privacy-microphone"),
        Capability::TextInjection | Capability::InputMonitoring => None,
    }
}

#[cfg(all(feature = "real-injection", target_os = "linux"))]
fn permission_settings_url(capability: Capability) -> Option<&'static str> {
    match capability {
        Capability::Microphone | Capability::TextInjection => {
            Some("https://underthinker.github.io/verbatim/permissions/#linux")
        }
        Capability::InputMonitoring => None,
    }
}

#[cfg(all(feature = "real-injection", target_os = "macos"))]
fn build_permission_probe() -> Arc<dyn PermissionProbe> {
    Arc::new(verbatim_platform::macos::MacPermissionProbe::new())
}

#[cfg(all(feature = "real-injection", target_os = "windows"))]
fn build_permission_probe() -> Arc<dyn PermissionProbe> {
    Arc::new(verbatim_platform::windows::WinPermissionProbe::new())
}

#[cfg(all(feature = "real-injection", target_os = "linux"))]
fn build_permission_probe() -> Arc<dyn PermissionProbe> {
    Arc::new(verbatim_platform::linux::LinuxPermissionProbe::new())
}

struct Shell {
    handle: RunnerHandle,
}

/// Pick the screen-reader announcer for this build (UX.md 8).
///
/// The real backends only exist under `real-injection`, so headless CI and the
/// fake pipeline keep the no-screen-reader fake and touch no OS accessibility
/// stack. Each OS reaches its screen reader differently: macOS posts an
/// NSAccessibility announcement, Windows raises a UIA notification against the
/// overlay window's host provider, and Linux watches `org.a11y.Status` and
/// posts transient notifications Orca presents.
#[cfg(all(feature = "real-injection", target_os = "macos"))]
fn build_announcer(_overlay: &tauri::WebviewWindow) -> Arc<dyn AccessibilityAnnouncer> {
    Arc::new(verbatim_platform::macos::MacAnnouncer::new())
}

/// The Windows announcer needs a window to hang the UIA host provider on; the
/// overlay is the one that represents the announced state. A handle we cannot
/// read leaves the app silent to Narrator rather than failing the launch.
#[cfg(all(feature = "real-injection", target_os = "windows"))]
fn build_announcer(overlay: &tauri::WebviewWindow) -> Arc<dyn AccessibilityAnnouncer> {
    match overlay.hwnd() {
        Ok(hwnd) => Arc::new(verbatim_platform::windows::WinAnnouncer::new(
            hwnd.0 as isize,
        )),
        Err(err) => {
            tracing::warn!(?err, "overlay HWND unavailable; announcements disabled");
            Arc::new(FakeAnnouncer::default())
        }
    }
}

#[cfg(all(feature = "real-injection", target_os = "linux"))]
fn build_announcer(_overlay: &tauri::WebviewWindow) -> Arc<dyn AccessibilityAnnouncer> {
    Arc::new(verbatim_platform::linux::LinuxAnnouncer::new())
}

#[cfg(not(all(
    feature = "real-injection",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
)))]
fn build_announcer(_overlay: &tauri::WebviewWindow) -> Arc<dyn AccessibilityAnnouncer> {
    Arc::new(FakeAnnouncer::default())
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

/// Validate + persist a full config from the Settings webview (UX.md 7), then
/// re-apply it to the running runner live (Phase D). The hotkey is
/// conflict-checked before anything is written; an invalid chord is rejected
/// whole so the file never holds a chord the runner cannot bind.
///
/// The polish toggle, personal dictionary, and per-app profiles take effect on
/// the next dictation without a restart. Hotkey mode / model changes still apply
/// on next launch: those are owned by the OS hotkey registration and the engine
/// loaders, not the runner's pipeline knobs.
#[tauri::command]
async fn settings_set(state: tauri::State<'_, Shell>, config: Config) -> Result<(), String> {
    Config::validate_hotkey(&config.hotkey).map_err(|err| err.to_string())?;
    config.save().map_err(|err| err.to_string())?;
    state
        .handle
        .reconfigure(config.to_runner_config())
        .await
        .map_err(|_| "runner stopped".to_owned())
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

/// Open the end-user docs in the OS default browser (About tab).
///
/// The URL is a Rust constant, not a webview argument: the command surface
/// stays closed, so a compromised webview cannot turn this into an
/// open-arbitrary-URL primitive (THREAT_MODEL.md, IPC verb-closure posture).
#[tauri::command]
fn open_docs() -> Result<(), String> {
    tauri_plugin_opener::open_url(DOCS_URL, None::<&str>).map_err(|err| err.to_string())
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

    // Dogfood counters (M4 Phase E): reconcile any crash from a prior run, then
    // count each verified delivery. Local only; surfaced via `verbatim stats`.
    let stats_dir = crate::config::data_dir();
    crate::stats::begin_run(&stats_dir);
    tauri::async_runtime::spawn(crate::stats::run_recorder(
        events.subscribe(),
        stats_dir.clone(),
    ));

    let (runner, handle) = SessionRunner::new(
        daemon::build_deps(),
        Config::load().to_runner_config(),
        Arc::clone(&events),
    );
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

    // Onboarding and the model manager share the production downloader so a
    // partial transfer can resume from either surface.
    let downloader =
        match verbatim_model_downloader::HttpModelDownloader::new(crate::models::models_dir()) {
            Ok(downloader) => {
                Arc::new(downloader) as Arc<dyn verbatim_engines::model::ModelDownloader>
            }
            Err(err) => {
                eprintln!("verbatim: model downloader initialization failed: {err}");
                return ExitCode::FAILURE;
            }
        };
    #[cfg(feature = "real-injection")]
    let (permission_probe, permission_requester): (
        Arc<dyn PermissionProbe>,
        Arc<dyn PermissionRequest>,
    ) = (
        build_permission_probe(),
        Arc::new(SystemPermissionRequester),
    );
    #[cfg(not(feature = "real-injection"))]
    let (permission_probe, permission_requester): (
        Arc<dyn PermissionProbe>,
        Arc<dyn PermissionRequest>,
    ) = {
        let probe = Arc::new(FakePermissionProbe::default());
        (
            Arc::clone(&probe) as Arc<dyn PermissionProbe>,
            Arc::new(FakePermissionRequester::new(probe)),
        )
    };
    let onboarding = Onboarding::new(
        permission_probe,
        permission_requester,
        Arc::clone(&downloader),
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

    // Global hotkey: the shell binds the same chord the headless daemon does,
    // so the installed app dictates without a separate daemon process. On
    // macOS registration must happen on this (main) thread before the webview
    // loop starts; the backend's run-loop source is then serviced by the
    // webview event loop itself, and the chord path's channel is drained from
    // the run callback below.
    #[cfg(all(feature = "global-hotkey", target_os = "macos"))]
    let hotkey = {
        let (chord, mode) = daemon::hotkey_selection();
        let (edge_tx, semantics) = daemon::hotkey_semantics_channel(handle.clone(), mode);
        tauri::async_runtime::spawn(semantics);
        daemon::register_macos_hotkey(&chord, mode, edge_tx)
    };
    // The Linux/Windows backends own their delivery threads, so they only have
    // to outlive the shell: hold them until the webview loop returns.
    #[cfg(all(feature = "real-injection", target_os = "linux"))]
    let _hotkey = {
        let (chord, mode) = daemon::hotkey_selection();
        let (edge_tx, semantics) = daemon::hotkey_semantics_channel(handle.clone(), mode);
        tauri::async_runtime::spawn(semantics);
        daemon::register_linux_hotkey(&chord, mode, edge_tx)
    };
    #[cfg(all(feature = "real-injection", target_os = "windows"))]
    let _hotkey = {
        let (chord, mode) = daemon::hotkey_selection();
        let (edge_tx, semantics) = daemon::hotkey_semantics_channel(handle.clone(), mode);
        tauri::async_runtime::spawn(semantics);
        daemon::register_windows_hotkey(&chord, mode, edge_tx)
    };

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Shell { handle })
        .manage(OnboardingShell {
            service: onboarding,
        })
        .manage(ModelManager::new(downloader, Arc::clone(&events)))
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
            open_docs,
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
            let overlay_window = overlay::create_window(app.handle())?;
            overlay::spawn_driver(
                app.handle().clone(),
                &events,
                build_announcer(&overlay_window),
            );
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
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(err) => {
            eprintln!("verbatim: shell failed: {err}");
            return ExitCode::FAILURE;
        }
    };

    app.run(move |_app, _event| {
        // The chord backend's edges land on a process-global channel that must
        // be drained on the main thread each time the event loop turns (the
        // modifier-tap backend delivers straight from its run-loop source and
        // drains as a no-op).
        #[cfg(all(feature = "global-hotkey", target_os = "macos"))]
        if matches!(_event, tauri::RunEvent::MainEventsCleared) {
            hotkey.drain();
        }
    });

    // The webview loop returned, so this is an orderly shutdown: clear the
    // marker so the next launch is not counted as a crash.
    crate::stats::end_run_clean(&stats_dir);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::DOCS_URL;

    /// The About tab's one-click open and the published site must not drift
    /// apart: `open_docs` sends the user wherever `docs.yml` deploys Starlight.
    #[test]
    fn docs_url_matches_the_starlight_site_config() {
        let config = include_str!("../../../docs/site/astro.config.mjs");
        let site = DOCS_URL.trim_end_matches('/');
        assert!(
            config.contains(&format!("site: \"{site}\"")),
            "DOCS_URL {DOCS_URL} is not the site astro.config.mjs publishes"
        );
    }
}
