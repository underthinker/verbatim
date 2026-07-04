//! `PermissionProbe` over the macOS TCC/AX preflight APIs: probed without
//! prompting (ARCHITECTURE.md 4.6, spike 2).

use crate::macos::ffi::{ax_trusted, input_monitoring_access, microphone_authorization};
use crate::traits::PermissionProbe;
use crate::types::{Capability, PermissionState};

/// `AVAuthorizationStatus` values.
const AV_NOT_DETERMINED: isize = 0;
const AV_AUTHORIZED: isize = 3;

/// `kIOHIDAccessType` values.
const IOHID_GRANTED: u32 = 0;
const IOHID_DENIED: u32 = 1;

/// Zero-state [`PermissionProbe`]; every query hits the live system API.
#[derive(Default)]
pub struct MacPermissionProbe;

impl MacPermissionProbe {
    pub fn new() -> Self {
        Self
    }
}

impl PermissionProbe for MacPermissionProbe {
    fn probe(&self, capability: Capability) -> PermissionState {
        match capability {
            Capability::Microphone => match microphone_authorization() {
                AV_AUTHORIZED => PermissionState::Granted,
                AV_NOT_DETERMINED => PermissionState::Undetermined,
                // Restricted (1) and denied (2) are both effectively denied;
                // -1 (class unavailable) is treated the same, conservatively.
                _ => PermissionState::Denied,
            },
            // AX trust is binary and never prompts here; not-yet-granted reads
            // as denied rather than undetermined.
            Capability::TextInjection => {
                if ax_trusted() {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                }
            }
            Capability::InputMonitoring => match input_monitoring_access() {
                IOHID_GRANTED => PermissionState::Granted,
                IOHID_DENIED => PermissionState::Denied,
                _ => PermissionState::Undetermined,
            },
        }
    }
}
