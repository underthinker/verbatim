//! Menu-bar tray presence with a Quit item (feature `global-hotkey`, macOS).
//!
//! M1 scope (ROADMAP): a status-bar icon whose only menu item is Quit. It
//! shares the main-thread run loop the hotkey pumps - menu clicks arrive on
//! muda's process-global channel, which [`TrayBackend::quit_requested`] drains
//! each pump. State-reflecting icon variants and the richer menu (raw/polished,
//! device picker, recent dictations) are a Tauri-shell concern later
//! (UX.md 122); this slice only proves the presence and a clean quit.
//!
//! A plain CLI binary is not a GUI app, so before the status item is created we
//! promote the process to an *accessory* app: that lets the item appear in the
//! menu bar without the process also claiming a Dock tile or a main menu.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::errors::TrayError;

/// A menu-bar status item owning a single Quit menu entry. Not `Send`: like the
/// hotkey backend it must be built, polled, and dropped on the main thread that
/// owns the run loop (dropping it removes the item from the menu bar).
pub struct TrayBackend {
    // Held for its lifetime: dropping the icon tears the status item down.
    _tray: TrayIcon,
    quit_id: MenuId,
}

impl TrayBackend {
    /// Promote the process to an accessory app and install the status item.
    /// Must run on the main thread (the run-loop owner on macOS).
    pub fn new() -> Result<Self, TrayError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            TrayError::Backend("tray must be built on the main thread".to_owned())
        })?;

        // Accessory: menu-bar presence without a Dock tile. Idempotent, so it is
        // safe even though the eventual Tauri shell may set its own policy.
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        let quit = MenuItem::new("Quit Verbatim", true, None);
        let quit_id = quit.id().clone();
        let menu = Menu::new();
        menu.append(&quit)
            .map_err(|err| TrayError::Backend(err.to_string()))?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Verbatim")
            .with_icon(status_icon())
            // Let macOS tint the glyph for the light/dark menu bar (UX.md 122:
            // the icon reflects state; a template keeps it legible on both).
            .with_icon_as_template(true)
            .build()
            .map_err(|err| TrayError::Backend(err.to_string()))?;

        Ok(Self {
            _tray: tray,
            quit_id,
        })
    }

    /// Drain muda's global menu channel and report whether Quit was clicked.
    /// The run-loop pump delivers the click; this only reads what it queued, so
    /// it must be called from the same main-thread loop that pumps the hotkey.
    pub fn quit_requested(&self) -> bool {
        let receiver = MenuEvent::receiver();
        let mut quit = false;
        while let Ok(event) = receiver.try_recv() {
            if event.id == self.quit_id {
                quit = true;
            }
        }
        quit
    }
}

/// A small template glyph for the status item: a filled dot in the alpha
/// channel (RGB is ignored for a template image - macOS tints it). A
/// state-aware icon set replaces this when the overlay/tray design lands.
fn status_icon() -> Icon {
    const SIZE: u32 = 18;
    const R: f32 = 7.0;
    let center = (SIZE as f32 - 1.0) / 2.0;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy <= R * R {
                let px = ((y * SIZE + x) * 4) as usize;
                rgba[px + 3] = 255; // opaque inside the dot; RGB stays 0
            }
        }
    }
    // The dimensions match the buffer we just built, so this cannot fail; a
    // panic here would only fire on a programming error, not at runtime.
    #[allow(clippy::expect_used)]
    Icon::from_rgba(rgba, SIZE, SIZE).expect("status icon buffer is 18x18 RGBA")
}
