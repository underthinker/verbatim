//! `PermissionProbe` for Windows (ARCHITECTURE.md 4.6).
//!
//! Windows has no grant model for synthetic input or global hotkeys: no
//! Accessibility trust (macOS) and no portal consent (Linux). `SendInput` and
//! `RegisterHotKey` work for any interactive process; the one denial that
//! exists - UIPI blocking input into an elevated window - is per-target and
//! only observable at injection time, where the injector reports it honestly
//! (E4). Microphone consent is negotiated by the capture stack when the
//! device is opened, so nothing here can probe it without prompting.

use crate::traits::PermissionProbe;
use crate::types::{Capability, PermissionState};

/// Windows [`PermissionProbe`]: non-prompting by construction.
#[derive(Default)]
pub struct WinPermissionProbe;

impl WinPermissionProbe {
    pub fn new() -> Self {
        Self
    }
}

impl PermissionProbe for WinPermissionProbe {
    fn probe(&self, capability: Capability) -> PermissionState {
        match capability {
            // Owned by the capture stack at open time; unknowable up front
            // without prompting.
            Capability::Microphone => PermissionState::Undetermined,
            // No system grant exists (see module docs); UIPI denials surface
            // per injection attempt, not as a probeable capability.
            Capability::TextInjection | Capability::InputMonitoring => PermissionState::NotNeeded,
        }
    }
}
