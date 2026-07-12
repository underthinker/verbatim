//! The dictation overlay window (M2 Phase B).
//!
//! A small always-on-top pill (UX.md 7) that is click-through and never takes
//! keyboard focus (UX.md 2 global rules; spike 1: focus-steal broke Handy on
//! KDE). The window is created hidden at startup so showing it on ARMING is a
//! cheap native call, keeping the < 50 ms visible budget (plans/m2-ux-shell.md
//! Phase B).
//!
//! Updates are driven directly from the core `EventBus` on the Rust side -
//! never through the webview command path. The driver translates
//! `SessionTransition`/`InputLevel` into a tiny overlay-only event vocabulary
//! (`OverlayEvent`) emitted straight to this window, and owns show/hide
//! orchestration including the post-success linger and the "didn't catch
//! anything" flash (UX.md 2).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use verbatim_core::event::{Event, EventBus};
use verbatim_core::session::SessionState;
use verbatim_platform::AccessibilityAnnouncer;

use crate::error_catalog::{self, ErrorPresentation};

/// Tauri window label for the overlay.
pub const WINDOW_LABEL: &str = "overlay";

/// The overlay-only event channel; targeted at the overlay window, distinct
/// from the 1:1 bus mirror the main webview subscribes to (`bridge`).
pub const EVENT_CHANNEL: &str = "verbatim://overlay";

/// Logical pill size (UX.md 7: small pill).
// The transparent host must fit the largest rendered state, not only the
// compact listening pill. Error copy can wrap beside an action affordance;
// keeping the native host smaller than the CSS pill clipped exactly the useful
// part of recovery messages on Retina displays.
const WIDTH: f64 = 480.0;
const HEIGHT: f64 = 104.0;
/// Logical gap between the pill and the bottom edge of the display.
const BOTTOM_MARGIN: f64 = 48.0;

/// Success tick + 200 ms fade before the window hides (UX.md 2 INJECTING).
const SUCCESS_LINGER: Duration = Duration::from_millis(450);
/// "Didn't catch anything" soft flash duration (UX.md 2 global rules).
const NOTHING_HEARD_LINGER: Duration = Duration::from_millis(1400);
/// Auto-dismiss timeouts are stretched while assistive tech is active so a
/// screen reader has time to finish speaking the state (UX.md 8).
const ASSISTIVE_LINGER_FACTOR: u32 = 3;

/// What the overlay is currently presenting. The webview owns the visuals
/// (shimmer, waveform, sweep, progress, tick) and reduced-motion handling;
/// this enum is the complete state vocabulary it renders from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayPhase {
    /// Mic icon + "starting" shimmer.
    Arming,
    /// Live waveform driven by `Level` events.
    Recording,
    /// Waveform freezes, short sweep animation.
    Finalizing,
    /// Indeterminate progress (TRANSCRIBING / POLISHING / INJECTING share one
    /// visual phase per UX.md 2).
    Processing,
    /// Brief success tick, then the fade; the driver hides the window after
    /// `SUCCESS_LINGER`.
    Success,
    /// Soft "didn't catch anything" flash, no dialog.
    NothingHeard,
    /// Session failed; carries the UX catalog ID. Full error surfaces are
    /// Phase C - the overlay shows a minimal named-error pill until then.
    Error,
}

/// Wire shape emitted to the overlay webview.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OverlayEvent {
    Phase {
        phase: OverlayPhase,
        /// The full error catalog response (copy + primary action); set only
        /// when `phase` is `Error` (UX.md 4). The webview renders copy and the
        /// action label from this - it never maps error IDs itself.
        error: Option<ErrorPresentation>,
    },
    Level {
        rms: f32,
    },
}

/// Window-level action a session transition demands.
#[derive(Debug, Clone, PartialEq)]
pub enum Directive {
    /// Present `phase` and make sure the window is visible.
    Show {
        phase: OverlayPhase,
        error: Option<ErrorPresentation>,
    },
    /// Present `phase`, then hide after `linger` unless superseded.
    Flash {
        phase: OverlayPhase,
        linger: Duration,
    },
    /// Hide immediately (ESC cancel discards without ceremony).
    Hide,
}

/// Map a session transition to an overlay directive - the UX.md 2 overlay
/// column as a pure function, unit-tested without a display server.
pub fn directive(from: SessionState, to: SessionState) -> Option<Directive> {
    use SessionState as S;

    match (from, to) {
        (_, S::Arming) => Some(Directive::Show {
            phase: OverlayPhase::Arming,
            error: None,
        }),
        (_, S::Recording) => Some(Directive::Show {
            phase: OverlayPhase::Recording,
            error: None,
        }),
        (_, S::Finalizing) => Some(Directive::Show {
            phase: OverlayPhase::Finalizing,
            error: None,
        }),
        (_, S::Transcribing | S::Polishing | S::Injecting) => Some(Directive::Show {
            phase: OverlayPhase::Processing,
            error: None,
        }),
        // Injection delivered: success tick + fade.
        (S::Injecting, S::Idle) => Some(Directive::Flash {
            phase: OverlayPhase::Success,
            linger: SUCCESS_LINGER,
        }),
        // VAD saw no speech: soft flash, no dialog (UX.md 2 global rules).
        (S::Finalizing, S::Idle) => Some(Directive::Flash {
            phase: OverlayPhase::NothingHeard,
            linger: NOTHING_HEARD_LINGER,
        }),
        // ESC cancel from ARMING/RECORDING: discard, vanish.
        (S::Arming | S::Recording, S::Idle) => Some(Directive::Hide),
        (_, S::Failed(id)) => Some(Directive::Show {
            phase: OverlayPhase::Error,
            error: Some(error_catalog::present(id)),
        }),
        _ => None,
    }
}

/// The spoken form of an overlay phase for the OS accessibility announcement
/// (UX.md 8). Mirrors the webview's visible `PHASE_LABEL`; errors speak their
/// plain catalog copy. `None` for states with nothing worth announcing.
fn announcement(phase: OverlayPhase, error: Option<&ErrorPresentation>) -> Option<String> {
    let text = match phase {
        OverlayPhase::Arming => "Starting",
        OverlayPhase::Recording => "Listening",
        OverlayPhase::Finalizing => "Finishing",
        OverlayPhase::Processing => "Transcribing",
        OverlayPhase::Success => "Done",
        OverlayPhase::NothingHeard => "Didn't catch anything",
        OverlayPhase::Error => return error.map(|e| e.copy.to_owned()),
    };
    Some(text.to_owned())
}

/// Stretch an auto-dismiss linger when a screen reader is active so it has time
/// to speak the state before the pill vanishes (UX.md 8).
fn linger_for(base: Duration, screen_reader: bool) -> Duration {
    if screen_reader {
        base * ASSISTIVE_LINGER_FACTOR
    } else {
        base
    }
}

/// Build the overlay window, hidden. Every flag here is a UX.md 2/7
/// requirement: click-through, never focused, above all windows, on every
/// workspace, no chrome.
pub fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window =
        WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Verbatim Overlay")
            .inner_size(WIDTH, HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .decorations(false)
            .shadow(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            // Non-activating: WS_EX_NOACTIVATE on Windows, non-key on macOS,
            // gtk_window_set_accept_focus(false) on Linux. The KDE Plasma 6
            // focus-steal check (spike-1 regression) is the manual sign-off.
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()?;
    // Click-through is set on first show, not here: on GTK/wlroots the window's
    // GDK surface is not realized while hidden, and tao's `CursorIgnoreEvents`
    // handler unwraps it, aborting the process at startup. See `mark_click_through`.
    #[cfg(target_os = "linux")]
    promote_to_layer_surface(&window);
    Ok(window)
}

/// Promote the overlay to a Wayland layer-shell surface with no keyboard
/// interactivity. On Wayland the compositor - not the client - decides
/// focus-on-map for xdg-toplevels, so `focusable(false)` (an X11 accept-focus
/// hint) does not stop wlroots/GNOME giving the pill keyboard focus when it
/// maps. A focused overlay is not cosmetic: the injected Ctrl-V would land in
/// the pill instead of the target editor. A layer surface with
/// `KeyboardMode::None` is the only protocol-level "never focus me" on Wayland.
///
/// Must run before the window is realized; `create_window` builds it hidden, so
/// this is the last step before the first `show()`. Under X11/XWayland the
/// window is not a Wayland surface and this is a no-op (logged), leaving the
/// working `focusable(false)` path.
#[cfg(target_os = "linux")]
fn promote_to_layer_surface(window: &WebviewWindow) {
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    let gtk_window = match window.gtk_window() {
        Ok(w) => w,
        Err(err) => {
            tracing::warn!(?err, "overlay layer-shell setup skipped: no gtk window");
            return;
        }
    };
    gtk_window.init_layer_shell();
    gtk_window.set_layer(Layer::Overlay);
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_namespace("verbatim-overlay");
    // Bottom-center: anchor to the bottom edge (leaving left/right unanchored
    // centers horizontally), same margin as `position_bottom_center`.
    gtk_window.set_anchor(Edge::Bottom, true);
    gtk_window.set_layer_shell_margin(Edge::Bottom, BOTTOM_MARGIN as i32);
    if !gtk_window.is_layer_window() {
        tracing::warn!(
            "overlay is not a layer surface (compositor without layer-shell?); it may steal focus"
        );
    }
}

/// Make the overlay click-through, once, after it is first shown. Deferred from
/// `create_window` because the underlying window must be realized: macOS and
/// Windows tolerate the call on a hidden window, GTK/wlroots does not.
fn mark_click_through(window: &WebviewWindow, done: &Arc<AtomicBool>) {
    if done.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(err) = window.set_ignore_cursor_events(true) {
        tracing::warn!(?err, "overlay click-through setup failed");
        done.store(false, Ordering::SeqCst);
    }
}

/// On Linux the overlay is a layer-shell surface (see `promote_to_layer_surface`),
/// anchored bottom-center by the compositor, so `set_position` does not apply.
#[cfg(target_os = "linux")]
fn position_bottom_center(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// Bottom-center of the display the window is on (UX.md 7 default placement;
/// configurable placement is later scope).
#[cfg(not(target_os = "linux"))]
fn position_bottom_center(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let origin = monitor.position();
    let width = (WIDTH * scale) as i32;
    let height = ((HEIGHT + BOTTOM_MARGIN) * scale) as i32;
    let x = origin.x + (size.width as i32 - width) / 2;
    let y = origin.y + size.height as i32 - height;
    window.set_position(tauri::PhysicalPosition { x, y })
}

/// Subscribe the overlay to the core bus and drive it for the lifetime of the
/// app. Lagged receivers skip to the live edge, same policy as the webview
/// bridge: transitions replay and the next one supersedes anything missed.
pub fn spawn_driver(app: AppHandle, events: &EventBus, announcer: Arc<dyn AccessibilityAnnouncer>) {
    let mut receiver = events.subscribe();
    // Bumped on every directive; a pending flash-hide only fires if it is
    // still the latest presentation (a new session cancels the hide).
    let generation = Arc::new(AtomicU64::new(0));
    // Set true once the overlay has been shown and made click-through.
    let click_through = Arc::new(AtomicBool::new(false));

    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(Event::SessionTransition { from, to, .. }) => {
                    if let Some(directive) = directive(from, to) {
                        apply(&app, &generation, &click_through, &announcer, directive);
                    }
                }
                Ok(Event::InputLevel { rms }) => {
                    if let Err(err) =
                        app.emit_to(WINDOW_LABEL, EVENT_CHANNEL, OverlayEvent::Level { rms })
                    {
                        tracing::warn!(?err, "overlay level emit failed");
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "overlay driver lagged; skipping to live edge");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn apply(
    app: &AppHandle,
    generation: &Arc<AtomicU64>,
    click_through: &Arc<AtomicBool>,
    announcer: &Arc<dyn AccessibilityAnnouncer>,
    directive: Directive,
) {
    let stamp = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        tracing::warn!("overlay window missing; dropping directive {directive:?}");
        return;
    };

    // Announce the state to a screen reader (default on when one is running);
    // the click-through overlay window is never focused, so this OS-level
    // announcement is the only path assistive tech hears (UX.md 8). The post
    // must run on the main thread (NSAccessibility contract), so it hops there
    // via Tauri while the driver keeps consuming the bus off-thread.
    let announce = |phase: OverlayPhase, error: Option<&ErrorPresentation>| {
        if announcer.screen_reader_active()
            && let Some(message) = announcement(phase, error)
        {
            let announcer = Arc::clone(announcer);
            if let Err(err) = app.run_on_main_thread(move || announcer.announce(&message)) {
                tracing::warn!(?err, "overlay a11y announce dispatch failed");
            }
        }
    };

    let present = |phase: OverlayPhase, error: Option<ErrorPresentation>| {
        announce(phase, error.as_ref());
        if let Err(err) = app.emit_to(
            WINDOW_LABEL,
            EVENT_CHANNEL,
            OverlayEvent::Phase { phase, error },
        ) {
            tracing::warn!(?err, "overlay phase emit failed");
        }
        if let Err(err) = position_bottom_center(&window).and_then(|()| window.show()) {
            tracing::warn!(?err, "overlay show failed");
        } else {
            mark_click_through(&window, click_through);
        }
    };

    match directive {
        Directive::Show { phase, error } => present(phase, error),
        Directive::Flash { phase, linger } => {
            present(phase, None);
            let linger = linger_for(linger, announcer.screen_reader_active());
            let window = window.clone();
            let generation = Arc::clone(generation);
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(linger).await;
                if generation.load(Ordering::SeqCst) == stamp
                    && let Err(err) = window.hide()
                {
                    tracing::warn!(?err, "overlay hide after flash failed");
                }
            });
        }
        Directive::Hide => {
            if let Err(err) = window.hide() {
                tracing::warn!(?err, "overlay hide failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verbatim_core::error::ErrorId;

    #[test]
    fn transitions_map_to_the_ux_overlay_column() {
        use OverlayPhase as P;
        use SessionState as S;

        let show = |phase| Some(Directive::Show { phase, error: None });

        assert_eq!(directive(S::Idle, S::Arming), show(P::Arming));
        assert_eq!(directive(S::Arming, S::Recording), show(P::Recording));
        assert_eq!(directive(S::Recording, S::Finalizing), show(P::Finalizing));
        assert_eq!(
            directive(S::Finalizing, S::Transcribing),
            show(P::Processing)
        );
        assert_eq!(
            directive(S::Transcribing, S::Polishing),
            show(P::Processing)
        );
        assert_eq!(directive(S::Polishing, S::Injecting), show(P::Processing));
        assert_eq!(
            directive(S::Transcribing, S::Injecting),
            show(P::Processing)
        );
    }

    #[test]
    fn success_lingers_with_the_fade_budget() {
        match directive(SessionState::Injecting, SessionState::Idle) {
            Some(Directive::Flash { phase, linger }) => {
                assert_eq!(phase, OverlayPhase::Success);
                // Must cover the 200 ms fade (UX.md 2 INJECTING row).
                assert!(linger >= Duration::from_millis(200));
            }
            other => panic!("expected success flash, got {other:?}"),
        }
    }

    #[test]
    fn silence_flashes_nothing_heard_without_a_dialog() {
        match directive(SessionState::Finalizing, SessionState::Idle) {
            Some(Directive::Flash { phase, .. }) => {
                assert_eq!(phase, OverlayPhase::NothingHeard);
            }
            other => panic!("expected nothing-heard flash, got {other:?}"),
        }
    }

    #[test]
    fn cancel_hides_immediately() {
        assert_eq!(
            directive(SessionState::Recording, SessionState::Idle),
            Some(Directive::Hide)
        );
        assert_eq!(
            directive(SessionState::Arming, SessionState::Idle),
            Some(Directive::Hide)
        );
    }

    #[test]
    fn failures_carry_the_catalog_presentation() {
        match directive(SessionState::Recording, SessionState::Failed(ErrorId::E6)) {
            Some(Directive::Show {
                phase: OverlayPhase::Error,
                error: Some(presentation),
            }) => {
                assert_eq!(presentation, error_catalog::present(ErrorId::E6));
                assert_eq!(presentation.id, "E6");
                assert!(presentation.action.is_some(), "E6 needs a primary action");
            }
            other => panic!("expected an error presentation, got {other:?}"),
        }
    }

    #[test]
    fn phase_event_matches_the_ts_contract() {
        // Test-only: DTOs are plain data, serialization cannot fail.
        #[allow(clippy::unwrap_used)]
        let value = serde_json::to_value(OverlayEvent::Phase {
            phase: OverlayPhase::NothingHeard,
            error: None,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "phase",
                "phase": "nothingHeard",
                "error": null,
            })
        );
    }

    #[test]
    fn every_visible_phase_has_a_spoken_form() {
        use OverlayPhase as P;
        for phase in [
            P::Arming,
            P::Recording,
            P::Finalizing,
            P::Processing,
            P::Success,
            P::NothingHeard,
        ] {
            assert!(
                announcement(phase, None).is_some(),
                "{phase:?} needs a screen-reader announcement"
            );
        }
    }

    #[test]
    fn error_announcement_speaks_the_catalog_copy() {
        let present = error_catalog::present(ErrorId::E6);
        assert_eq!(
            announcement(OverlayPhase::Error, Some(&present)),
            Some(present.copy.to_owned())
        );
        // No presentation, nothing to say rather than a misleading generic.
        assert_eq!(announcement(OverlayPhase::Error, None), None);
    }

    #[test]
    fn assistive_tech_stretches_auto_dismiss_timeouts() {
        // Without a screen reader the linger is untouched; with one it grows so
        // the state can be spoken before the pill vanishes (UX.md 8).
        assert_eq!(linger_for(SUCCESS_LINGER, false), SUCCESS_LINGER);
        assert_eq!(
            linger_for(SUCCESS_LINGER, true),
            SUCCESS_LINGER * ASSISTIVE_LINGER_FACTOR
        );
        assert!(linger_for(NOTHING_HEARD_LINGER, true) > NOTHING_HEARD_LINGER);
    }

    #[test]
    fn level_event_matches_the_ts_contract() {
        #[allow(clippy::unwrap_used)]
        let value = serde_json::to_value(OverlayEvent::Level { rms: 0.25 }).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "level", "rms": 0.25 }));
    }
}
