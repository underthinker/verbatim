//! Real global-hotkey-backed [`HotkeyManager`] (feature `global-hotkey`).
//!
//! The `global-hotkey` crate (from the Tao/Tauri authors) registers a system
//! chord and publishes down/up edges on a process-global channel. On macOS it
//! installs a Carbon handler on the *application* event target and adds its
//! event source to the *main* `CFRunLoop`, so edges are delivered only while
//! the **main thread** pumps its run loop. The backend therefore does no
//! threading of its own: the caller registers on the main thread and drives
//! [`GlobalHotkeyBackend::pump`] from that same thread. Hold/toggle semantics
//! stay in core (`verbatim_core::hotkey`); this layer forwards raw edges only.
//!
//! Cross-platform note: only the macOS run-loop path is wired for this slice.
//! Windows and Linux keep the fake until their backends land later in M1.

use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::errors::HotkeyError;
use crate::traits::{HotkeyCallback, HotkeyManager};
use crate::types::{HotkeyBinding, HotkeyEvent};

/// A hotkey source that must be serviced from the thread owning the OS event
/// loop (the main thread on macOS). The daemon holds one and calls [`pump`]
/// (MainThreadHotkey::pump) in a loop; concrete backends decide what a pump
/// does (drain a channel, or just run the run loop so a tap fires).
pub trait MainThreadHotkey {
    /// Advance the OS event loop for at most `timeout`, delivering edges.
    fn pump(&self, timeout: Duration);
}

impl MainThreadHotkey for GlobalHotkeyBackend {
    fn pump(&self, timeout: Duration) {
        GlobalHotkeyBackend::pump(self, timeout);
    }
}

/// A [`HotkeyManager`] backed by `global-hotkey`. Not `Send`: the manager must
/// be created, pumped, and dropped on the one thread that owns the OS event
/// loop (the main thread on macOS).
#[derive(Default)]
pub struct GlobalHotkeyBackend {
    registered: Option<Registered>,
}

struct Registered {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    id: u32,
    callback: HotkeyCallback,
}

impl GlobalHotkeyBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pump the current thread's run loop for `timeout`, then forward any edges
    /// the pump surfaced for our chord. Must be called on the same thread that
    /// called [`register`](HotkeyManager::register) - the main thread on macOS.
    pub fn pump(&self, timeout: Duration) {
        pump_event_loop(timeout);

        let Some(reg) = &self.registered else {
            return;
        };
        let receiver = GlobalHotKeyEvent::receiver();
        while let Ok(event) = receiver.try_recv() {
            if event.id == reg.id {
                (reg.callback)(match event.state {
                    HotKeyState::Pressed => HotkeyEvent::Pressed,
                    HotKeyState::Released => HotkeyEvent::Released,
                });
            }
        }
    }
}

impl HotkeyManager for GlobalHotkeyBackend {
    fn register(
        &mut self,
        binding: &HotkeyBinding,
        on_event: HotkeyCallback,
    ) -> Result<(), HotkeyError> {
        if self.registered.is_some() {
            return Err(HotkeyError::AlreadyRegistered);
        }

        let hotkey: HotKey = binding
            .chord
            .parse()
            .map_err(|_| HotkeyError::ChordUnavailable(binding.chord.clone()))?;
        let id = hotkey.id();

        let manager =
            GlobalHotKeyManager::new().map_err(|err| HotkeyError::Backend(err.to_string()))?;
        manager
            .register(hotkey)
            .map_err(|err| classify(err.to_string(), &binding.chord))?;

        self.registered = Some(Registered {
            manager,
            hotkey,
            id,
            callback: on_event,
        });
        Ok(())
    }

    fn unregister(&mut self) {
        if let Some(reg) = self.registered.take() {
            let _ = reg.manager.unregister(reg.hotkey);
        }
    }
}

impl Drop for GlobalHotkeyBackend {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Map the crate's error string onto the typed error. A "taken" chord is
/// distinct from a generic backend failure so callers can guide the user.
fn classify(message: String, chord: &str) -> HotkeyError {
    let lower = message.to_lowercase();
    if lower.contains("already") || lower.contains("registered") || lower.contains("in use") {
        HotkeyError::ChordUnavailable(chord.to_owned())
    } else {
        HotkeyError::Backend(message)
    }
}

/// Run the current thread's run loop for `timeout` so queued hotkey edges get
/// delivered. macOS needs an explicit `CFRunLoop`; other platforms drive their
/// delivery elsewhere, so we just sleep the interval.
#[cfg(target_os = "macos")]
pub(crate) fn pump_event_loop(timeout: Duration) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEventMask};
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode};

    // AppKit events (status-item clicks, menu tracking) sit in NSApplication's
    // event queue and are only delivered through nextEvent/sendEvent - running
    // the bare CFRunLoop leaves them queued forever, which made the tray icon
    // ignore clicks. nextEventMatchingMask also runs the CFRunLoop in the given
    // mode while it waits, so hotkey/tap sources still fire during the pump.
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        let deadline = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
        while let Some(event) = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&deadline),
                NSDefaultRunLoopMode,
                true,
            )
        } {
            app.sendEvent(&event);
        }
        return;
    }

    use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};

    // Not the main thread (tests): return early once a source has been serviced.
    CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, timeout, true);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pump_event_loop(timeout: Duration) {
    std::thread::sleep(timeout);
}
