//! `FocusTracker` for Wayland.
//!
//! Wayland deliberately has no cross-client "which app is focused" API, and
//! the injection backends here (portal keyboard, uinput) target whatever the
//! compositor has focused, not an app we pick. A best-effort placeholder
//! identity keeps the pipeline honest without faking knowledge we cannot
//! have; per-app profiles need compositor-specific extensions and land after
//! M1 (E7 focus semantics then tighten with them).

use crate::errors::FocusError;
use crate::traits::FocusTracker;
use crate::types::FocusedApp;

#[derive(Default)]
pub struct LinuxFocusTracker;

impl LinuxFocusTracker {
    pub fn new() -> Self {
        Self
    }
}

impl FocusTracker for LinuxFocusTracker {
    fn focused_app(&self) -> Result<FocusedApp, FocusError> {
        Ok(FocusedApp {
            app_id: "wayland:compositor-focused".to_owned(),
            window_title: None,
        })
    }
}
