//! `/dev/uinput` virtual keyboard: the direct injection fallback when the
//! portal is unavailable or denied (spike 1). In-process via the evdev
//! crate; requires write access to `/dev/uinput` (input group or udev rule).

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, KeyCode, KeyEvent};

use crate::errors::InjectError;
use crate::types::InjectionBackend;

/// A newly created uinput device is invisible to the compositor for a short
/// window; give udev/libinput time to pick it up before the first event.
const DEVICE_SETTLE: Duration = Duration::from_millis(200);

/// Gap between key transitions, mirroring real typing pacing.
const KEY_GAP: Duration = Duration::from_millis(5);

fn backend_error(reason: impl ToString) -> InjectError {
    InjectError::Backend {
        backend: InjectionBackend::Uinput,
        reason: reason.to_string(),
    }
}

/// Lazily created, then resident, virtual keyboard.
#[derive(Default)]
pub struct UinputKeyboard {
    device: Mutex<Option<VirtualDevice>>,
}

impl UinputKeyboard {
    /// Whether `/dev/uinput` is present and writable for this process.
    pub fn available() -> bool {
        std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .is_ok()
    }

    fn create_device() -> Result<VirtualDevice, InjectError> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_LEFTCTRL);
        keys.insert(KeyCode::KEY_V);
        let device = VirtualDevice::builder()
            .map_err(backend_error)?
            .name("Verbatim virtual keyboard")
            .with_keys(&keys)
            .map_err(backend_error)?
            .build()
            .map_err(backend_error)?;
        thread::sleep(DEVICE_SETTLE);
        Ok(device)
    }

    /// Synthesize Ctrl-V from the virtual keyboard.
    pub fn send_paste_chord(&self) -> Result<(), InjectError> {
        let mut guard = self
            .device
            .lock()
            .map_err(|_| backend_error("uinput device lock poisoned"))?;
        if guard.is_none() {
            *guard = Some(Self::create_device()?);
        }
        let device = guard
            .as_mut()
            .ok_or_else(|| backend_error("uinput device missing after creation"))?;

        let chord: [(KeyCode, i32); 4] = [
            (KeyCode::KEY_LEFTCTRL, 1),
            (KeyCode::KEY_V, 1),
            (KeyCode::KEY_V, 0),
            (KeyCode::KEY_LEFTCTRL, 0),
        ];
        for (key, value) in chord {
            device
                .emit(&[*KeyEvent::new(key, value)])
                .map_err(backend_error)?;
            thread::sleep(KEY_GAP);
        }
        Ok(())
    }
}
