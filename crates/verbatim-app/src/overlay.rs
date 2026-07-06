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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use verbatim_core::event::{Event, EventBus};
use verbatim_core::session::SessionState;

/// Tauri window label for the overlay.
pub const WINDOW_LABEL: &str = "overlay";

/// The overlay-only event channel; targeted at the overlay window, distinct
/// from the 1:1 bus mirror the main webview subscribes to (`bridge`).
pub const EVENT_CHANNEL: &str = "verbatim://overlay";

/// Logical pill size (UX.md 7: small pill).
const WIDTH: f64 = 320.0;
const HEIGHT: f64 = 72.0;
/// Logical gap between the pill and the bottom edge of the display.
const BOTTOM_MARGIN: f64 = 48.0;

/// Success tick + 200 ms fade before the window hides (UX.md 2 INJECTING).
const SUCCESS_LINGER: Duration = Duration::from_millis(450);
/// "Didn't catch anything" soft flash duration (UX.md 2 global rules).
const NOTHING_HEARD_LINGER: Duration = Duration::from_millis(1400);

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
        /// UX error catalog ID (E1-E10); set only when `phase` is `Error`.
        error: Option<String>,
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
        error: Option<String>,
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
            error: Some(format!("{id:?}")),
        }),
        _ => None,
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
    window.set_ignore_cursor_events(true)?;
    Ok(window)
}

/// Bottom-center of the display the window is on (UX.md 7 default placement;
/// configurable placement is later scope).
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
pub fn spawn_driver(app: AppHandle, events: &EventBus) {
    let mut receiver = events.subscribe();
    // Bumped on every directive; a pending flash-hide only fires if it is
    // still the latest presentation (a new session cancels the hide).
    let generation = Arc::new(AtomicU64::new(0));

    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(Event::SessionTransition { from, to, .. }) => {
                    if let Some(directive) = directive(from, to) {
                        apply(&app, &generation, directive);
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

fn apply(app: &AppHandle, generation: &Arc<AtomicU64>, directive: Directive) {
    let stamp = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        tracing::warn!("overlay window missing; dropping directive {directive:?}");
        return;
    };

    let present = |phase: OverlayPhase, error: Option<String>| {
        if let Err(err) = app.emit_to(
            WINDOW_LABEL,
            EVENT_CHANNEL,
            OverlayEvent::Phase { phase, error },
        ) {
            tracing::warn!(?err, "overlay phase emit failed");
        }
        if let Err(err) = position_bottom_center(&window).and_then(|()| window.show()) {
            tracing::warn!(?err, "overlay show failed");
        }
    };

    match directive {
        Directive::Show { phase, error } => present(phase, error),
        Directive::Flash { phase, linger } => {
            present(phase, None);
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
    fn failures_carry_the_catalog_id() {
        assert_eq!(
            directive(SessionState::Recording, SessionState::Failed(ErrorId::E6)),
            Some(Directive::Show {
                phase: OverlayPhase::Error,
                error: Some("E6".to_owned()),
            })
        );
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
    fn level_event_matches_the_ts_contract() {
        #[allow(clippy::unwrap_used)]
        let value = serde_json::to_value(OverlayEvent::Level { rms: 0.25 }).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "level", "rms": 0.25 }));
    }
}
