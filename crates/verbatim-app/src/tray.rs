//! The cross-platform system tray (M2 Phase E-4).
//!
//! A state-reflecting menu-bar / system-tray icon plus the UX.md 7 menu. Unlike
//! the M1 `verbatim-platform::tray` (a macOS-only Quit stopgap for the headless
//! daemon), this is owned by the Tauri shell and uses Tauri's native tray, so
//! macOS / Windows / Linux share one implementation - Tauri owns the run loop
//! and menu-event routing.
//!
//! Like the overlay (`overlay.rs`), the icon is driven straight from the core
//! `EventBus` on the Rust side, never through the webview command path
//! (ARCHITECTURE.md 4.9: the tray is a direct bus consumer). Menu actions drive
//! the same `RunnerHandle` / config / history the webviews use.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use verbatim_core::event::{Event, EventBus};
use verbatim_core::runner::RunnerHandle;
use verbatim_core::session::SessionState;

use crate::history::History;
use crate::settings::Config;

/// Stable id so the driver can fetch the icon back via `app.tray_by_id`.
pub const TRAY_ID: &str = "verbatim";

const ID_PAUSE: &str = "pause";
const ID_POLISH: &str = "polish";
const ID_DEVICE: &str = "device";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";
/// Recent-dictation menu ids are `recent:<history-row-id>`; the click handler
/// looks the raw text up in `TrayState::recents`.
const RECENT_PREFIX: &str = "recent:";
/// How many recent dictations the menu lists (UX.md 7: last 5).
const RECENT_LIMIT: u32 = 5;
/// Menu labels are truncated so a long dictation cannot stretch the tray menu.
const LABEL_CHARS: usize = 40;

/// Shell state the menu handlers reach through `app.state`.
struct TrayState {
    /// Drives live pause/resume (the one menu action the runner applies live).
    handle: RunnerHandle,
    history: std::sync::Arc<History>,
    /// Mirrors the runner's paused flag so the checkbox can toggle without an
    /// async round-trip; the runner is the source of truth on next `status`.
    paused: AtomicBool,
    /// Menu id -> raw transcript for the recent-dictation copy action.
    recents: Mutex<HashMap<String, String>>,
}

/// Install the tray. Managed state, menu, and icon are all created here; the
/// driver ([`spawn_driver`]) keeps the icon in sync with the session afterwards.
pub fn create(
    app: &AppHandle,
    handle: RunnerHandle,
    history: std::sync::Arc<History>,
) -> tauri::Result<()> {
    app.manage(TrayState {
        handle,
        history,
        paused: AtomicBool::new(false),
        recents: Mutex::new(HashMap::new()),
    });

    let menu = build_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(TrayVisual::Idle))
        .tooltip(tooltip(TrayVisual::Idle))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Subscribe the tray to the core bus for the app's lifetime: session
/// transitions repaint the icon, a recorded dictation rebuilds the recent menu.
/// Lagged receivers skip to the live edge, same policy as the overlay driver.
pub fn spawn_driver(app: AppHandle, events: &EventBus) {
    let mut receiver = events.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(Event::SessionTransition { to, .. }) => update_icon(&app, to),
                Ok(Event::DictationRecorded { .. }) => refresh_menu(&app),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "tray driver lagged; skipping to live edge");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        ID_PAUSE => toggle_pause(app),
        ID_POLISH => toggle_polish(app),
        ID_SETTINGS => show_main(app),
        ID_QUIT => app.exit(0),
        id if id.starts_with(RECENT_PREFIX) => copy_recent(app, id),
        _ => {}
    }
}

fn toggle_pause(app: &AppHandle) {
    let state = app.state::<TrayState>();
    let paused = !state.paused.load(Ordering::SeqCst);
    state.paused.store(paused, Ordering::SeqCst);
    let handle = state.handle.clone();
    tauri::async_runtime::spawn(async move {
        if handle.set_paused(paused).await.is_err() {
            tracing::warn!("tray pause toggle failed: runner stopped");
        }
    });
    refresh_menu(app);
}

/// Flip the raw/polished output setting. ponytail: persists only - the runner
/// reads polish at construction (like `settings_set`), so it takes effect on
/// next launch. Wire a runner reconfigure command when live rebinding lands.
fn toggle_polish(app: &AppHandle) {
    let mut config = Config::load();
    config.polish = !config.polish;
    if let Err(err) = config.save() {
        tracing::warn!(?err, "tray polish toggle save failed");
    }
    refresh_menu(app);
}

fn copy_recent(app: &AppHandle, id: &str) {
    let text = app
        .state::<TrayState>()
        .recents
        .lock()
        .ok()
        .and_then(|map| {
            // Poisoned lock is unreachable (no panic holds it); degrade to no copy.
            map.get(id).cloned()
        });
    if let Some(text) = text
        && let Err(err) = app.clipboard().write_text(text)
    {
        tracing::warn!(?err, "tray copy-recent to clipboard failed");
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Rebuild the whole menu and swap it in. Called after a toggle (checkbox
/// changed) or a new dictation (recent list changed) - the menu is small enough
/// that a full rebuild is simpler and cheaper than mutating it in place.
fn refresh_menu(app: &AppHandle) {
    match build_menu(app) {
        Ok(menu) => {
            if let Some(tray) = app.tray_by_id(TRAY_ID)
                && let Err(err) = tray.set_menu(Some(menu))
            {
                tracing::warn!(?err, "tray set_menu failed");
            }
        }
        Err(err) => tracing::warn!(?err, "tray menu rebuild failed"),
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<TrayState>();
    let paused = state.paused.load(Ordering::SeqCst);
    let polished = Config::load().polish;

    let pause =
        CheckMenuItem::with_id(app, ID_PAUSE, "Pause Verbatim", true, paused, None::<&str>)?;
    let polish = CheckMenuItem::with_id(
        app,
        ID_POLISH,
        "Polished output",
        true,
        polished,
        None::<&str>,
    )?;
    // ponytail: input-device picker deferred (E-4b). Needs
    // AudioCapture::input_devices() + a cpal enumeration backend; disabled stub
    // until the real audio seam lands, per plans/m2-ux-shell.md scope call.
    let device = MenuItem::with_id(app, ID_DEVICE, "Input device: Default", false, None::<&str>)?;
    let recent = build_recent(app, &state)?;
    let settings = MenuItem::with_id(app, ID_SETTINGS, "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit Verbatim", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &pause,
            &PredefinedMenuItem::separator(app)?,
            &polish,
            &device,
            &PredefinedMenuItem::separator(app)?,
            &recent,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &quit,
        ],
    )
}

/// The "recent dictations" submenu; each item copies its raw transcript on
/// click. Rebuilds the id->text map so clicks resolve against the live list.
fn build_recent(app: &AppHandle, state: &TrayState) -> tauri::Result<Submenu<tauri::Wry>> {
    let submenu = Submenu::with_id(app, "recent", "Recent dictations", true)?;
    let entries = state.history.list(RECENT_LIMIT).unwrap_or_else(|err| {
        tracing::warn!(?err, "tray recent-dictations query failed");
        Vec::new()
    });

    if let Ok(mut map) = state.recents.lock() {
        map.clear();
        if entries.is_empty() {
            let empty = MenuItem::with_id(
                app,
                "recent:none",
                "No recent dictations",
                false,
                None::<&str>,
            )?;
            submenu.append(&empty)?;
        } else {
            for entry in entries {
                let id = format!("{RECENT_PREFIX}{}", entry.id);
                let item = MenuItem::with_id(app, &id, truncate(&entry.raw), true, None::<&str>)?;
                submenu.append(&item)?;
                map.insert(id, entry.raw);
            }
        }
    }
    Ok(submenu)
}

/// One-line menu label: collapse newlines and clip to `LABEL_CHARS`.
fn truncate(text: &str) -> String {
    let flat = text.replace(['\n', '\r'], " ");
    let flat = flat.trim();
    if flat.chars().count() <= LABEL_CHARS {
        return flat.to_owned();
    }
    let clipped: String = flat.chars().take(LABEL_CHARS - 1).collect();
    format!("{clipped}…")
}

fn update_icon(app: &AppHandle, to: SessionState) {
    let visual = TrayVisual::from(to);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(err) = tray.set_icon(Some(icon(visual))) {
            tracing::warn!(?err, "tray set_icon failed");
        }
        let _ = tray.set_tooltip(Some(tooltip(visual)));
    }
}

/// The four states the tray icon reflects (UX.md 7). The processing phases
/// (transcribe/polish/inject) collapse into one, matching the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayVisual {
    Idle,
    Recording,
    Processing,
    Error,
}

impl From<SessionState> for TrayVisual {
    fn from(state: SessionState) -> Self {
        use SessionState as S;
        match state {
            S::Idle => Self::Idle,
            S::Arming | S::Recording | S::Finalizing => Self::Recording,
            S::Transcribing | S::Polishing | S::Injecting => Self::Processing,
            S::Failed(_) => Self::Error,
        }
    }
}

fn tooltip(visual: TrayVisual) -> String {
    let label = match visual {
        TrayVisual::Idle => "idle",
        TrayVisual::Recording => "recording",
        TrayVisual::Processing => "processing",
        TrayVisual::Error => "error",
    };
    format!("Verbatim - {label}")
}

/// Procedurally drawn 32x32 RGBA glyphs, distinct in *shape* as well as colour
/// so state is not signalled by colour alone: idle = thin ring, recording =
/// filled disc, processing = thick ring, error = disc with an exclamation cut
/// out. ponytail: Phase F (a11y) refines these against the high-contrast audit.
fn icon(visual: TrayVisual) -> Image<'static> {
    const SIZE: u32 = 32;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let (r, g, b) = match visual {
        TrayVisual::Idle => (0x9a, 0x9a, 0x9a),
        TrayVisual::Recording => (0xe0, 0x3b, 0x3b),
        TrayVisual::Processing => (0xe0, 0x9b, 0x2b),
        TrayVisual::Error => (0xd0, 0x33, 0x2b),
    };

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let mut alpha = match visual {
                TrayVisual::Idle => ring(dist, 11.0, 13.0),
                TrayVisual::Recording | TrayVisual::Error => disc(dist, 12.0),
                TrayVisual::Processing => ring(dist, 7.0, 13.0),
            };
            // Punch an exclamation mark out of the error disc.
            if visual == TrayVisual::Error && exclamation(dx, dy) {
                alpha = 0;
            }
            if alpha > 0 {
                let i = ((y * SIZE + x) * 4) as usize;
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = alpha;
            }
        }
    }
    Image::new_owned(rgba, SIZE, SIZE)
}

/// Anti-aliased filled disc: opaque within `radius`, one-pixel soft edge.
fn disc(dist: f32, radius: f32) -> u8 {
    coverage(radius - dist)
}

/// Anti-aliased ring between `inner` and `outer`, one-pixel soft edges.
fn ring(dist: f32, inner: f32, outer: f32) -> u8 {
    coverage((outer - dist).min(dist - inner))
}

/// Map a signed distance (pixels inside the shape) to an alpha with a 1px edge.
fn coverage(inside: f32) -> u8 {
    (inside.clamp(0.0, 1.0) * 255.0) as u8
}

/// True inside an exclamation-mark glyph centred on the disc (vertical bar plus
/// a dot below), used to knock a hole in the error icon.
fn exclamation(dx: f32, dy: f32) -> bool {
    let bar = dx.abs() <= 1.5 && (-8.0..=2.0).contains(&dy);
    let dot = dx.abs() <= 1.5 && (5.0..=8.0).contains(&dy);
    bar || dot
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbatim_core::error::ErrorId;

    #[test]
    fn session_states_map_to_the_four_tray_visuals() {
        use SessionState as S;
        use TrayVisual as V;
        assert_eq!(V::from(S::Idle), V::Idle);
        assert_eq!(V::from(S::Arming), V::Recording);
        assert_eq!(V::from(S::Recording), V::Recording);
        assert_eq!(V::from(S::Finalizing), V::Recording);
        assert_eq!(V::from(S::Transcribing), V::Processing);
        assert_eq!(V::from(S::Polishing), V::Processing);
        assert_eq!(V::from(S::Injecting), V::Processing);
        assert_eq!(V::from(S::Failed(ErrorId::E1)), V::Error);
    }

    #[test]
    fn labels_flatten_newlines_and_clip_length() {
        assert_eq!(truncate("  hello\nthere  "), "hello there");
        let long = "x".repeat(80);
        let label = truncate(&long);
        assert_eq!(label.chars().count(), LABEL_CHARS);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn every_visual_draws_some_opaque_pixels() {
        for visual in [
            TrayVisual::Idle,
            TrayVisual::Recording,
            TrayVisual::Processing,
            TrayVisual::Error,
        ] {
            let opaque = icon(visual)
                .rgba()
                .iter()
                .skip(3)
                .step_by(4)
                .any(|&a| a > 0);
            assert!(opaque, "{visual:?} icon must draw something");
        }
    }
}
