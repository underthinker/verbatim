//! `PermissionProbe` for Linux: probed without prompting (ARCHITECTURE.md
//! 4.6). Microphone access has no gatekeeper outside sandboxes; text
//! injection is "granted" when either the portal is plausible (consent is
//! asked lazily on first use) or `/dev/uinput` is writable.

use crate::linux::portal;
use crate::linux::uinput::UinputKeyboard;
use crate::traits::PermissionProbe;
use crate::types::{Capability, PermissionState};

#[derive(Default)]
pub struct LinuxPermissionProbe;

impl LinuxPermissionProbe {
    pub fn new() -> Self {
        Self
    }
}

impl PermissionProbe for LinuxPermissionProbe {
    fn probe(&self, capability: Capability) -> PermissionState {
        match capability {
            Capability::Microphone => PermissionState::NotNeeded,
            Capability::TextInjection => {
                if UinputKeyboard::available() {
                    PermissionState::Granted
                } else if portal::portal_plausible() {
                    // Consent is a lazy portal dialog; we cannot know without
                    // prompting, which this probe must never do.
                    PermissionState::Undetermined
                } else {
                    PermissionState::Denied
                }
            }
            Capability::InputMonitoring => PermissionState::NotNeeded,
        }
    }
}
