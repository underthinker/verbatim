//! `FocusTracker` via `NSWorkspace.frontmostApplication` for the focus rule
//! (E7) and per-app profiles (ARCHITECTURE.md 4.4).
//!
//! Window titles need per-app Accessibility queries and are deferred; `app_id`
//! (bundle identifier) is the stable key the profile system needs today.

use objc2_app_kit::NSWorkspace;

use crate::errors::FocusError;
use crate::traits::FocusTracker;
use crate::types::FocusedApp;

/// Zero-state [`FocusTracker`] reading the shared workspace on each query.
#[derive(Default)]
pub struct MacFocusTracker;

impl MacFocusTracker {
    pub fn new() -> Self {
        Self
    }
}

impl FocusTracker for MacFocusTracker {
    fn focused_app(&self) -> Result<FocusedApp, FocusError> {
        let app = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .ok_or(FocusError::Unknown)?;
        let app_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .or_else(|| app.localizedName().map(|s| s.to_string()))
            .ok_or(FocusError::Unknown)?;
        Ok(FocusedApp {
            app_id,
            window_title: None,
        })
    }
}
