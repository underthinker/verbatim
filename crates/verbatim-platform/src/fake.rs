//! Fake platform implementations: the deterministic test seam for core and
//! app tests (ENGINEERING.md section 4, E2E smoke).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use verbatim_engines::AudioBuffer;

use crate::errors::{
    AutostartError, CaptureError, ClipboardError, FocusError, HotkeyError, InjectError,
};
use crate::traits::{
    AudioCapture, Autostart, ClipboardGuard, FocusTracker, HotkeyCallback, HotkeyManager,
    PermissionProbe, TextInjector,
};
use crate::types::{
    Capability, ClipboardSnapshot, FocusedApp, HotkeyBinding, HotkeyEvent, InjectionBackend,
    InjectionReceipt, InjectionStrategy, PermissionState, RestoreOutcome,
};

/// A `HotkeyManager` whose events are fired manually from tests.
#[derive(Default)]
pub struct FakeHotkeyManager {
    callback: Mutex<Option<HotkeyCallback>>,
}

impl FakeHotkeyManager {
    /// Simulate the user pressing the registered chord.
    pub fn press(&self) {
        self.fire(HotkeyEvent::Pressed);
    }

    /// Simulate the user releasing the registered chord.
    pub fn release(&self) {
        self.fire(HotkeyEvent::Released);
    }

    fn fire(&self, event: HotkeyEvent) {
        if let Ok(guard) = self.callback.lock()
            && let Some(callback) = guard.as_ref()
        {
            callback(event);
        }
    }
}

impl HotkeyManager for FakeHotkeyManager {
    fn register(
        &mut self,
        _binding: &HotkeyBinding,
        on_event: HotkeyCallback,
    ) -> Result<(), HotkeyError> {
        let mut guard = self
            .callback
            .lock()
            .map_err(|_| HotkeyError::Backend("fake hotkey manager mutex poisoned".to_owned()))?;
        if guard.is_some() {
            return Err(HotkeyError::AlreadyRegistered);
        }
        *guard = Some(on_event);
        Ok(())
    }

    fn unregister(&mut self) {
        if let Ok(mut guard) = self.callback.lock() {
            *guard = None;
        }
    }
}

/// An `AudioCapture` that returns a fixed fixture buffer on `stop`.
pub struct FakeAudioCapture {
    fixture: AudioBuffer,
    capturing: AtomicBool,
}

impl FakeAudioCapture {
    pub fn new(fixture: AudioBuffer) -> Self {
        Self {
            fixture,
            capturing: AtomicBool::new(false),
        }
    }
}

impl AudioCapture for FakeAudioCapture {
    fn start(&mut self) -> Result<(), CaptureError> {
        self.capturing.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) -> Result<AudioBuffer, CaptureError> {
        if !self.capturing.swap(false, Ordering::SeqCst) {
            return Err(CaptureError::NotCapturing);
        }
        Ok(self.fixture.clone())
    }

    fn abort(&mut self) {
        self.capturing.store(false, Ordering::SeqCst);
    }

    fn is_capturing(&self) -> bool {
        self.capturing.load(Ordering::SeqCst)
    }
}

/// A `TextInjector` that records what it injected; can be told to fail.
#[derive(Default)]
pub struct FakeTextInjector {
    injected: Mutex<Vec<(String, FocusedApp)>>,
    fail: AtomicBool,
}

impl FakeTextInjector {
    /// Make every subsequent `inject` fail (E4 path).
    pub fn set_failing(&self, failing: bool) {
        self.fail.store(failing, Ordering::SeqCst);
    }

    /// Texts injected so far, in order.
    pub fn injected_texts(&self) -> Vec<String> {
        self.injected
            .lock()
            .map(|entries| entries.iter().map(|(text, _)| text.clone()).collect())
            .unwrap_or_default()
    }
}

impl TextInjector for FakeTextInjector {
    fn probe(&self) -> Vec<InjectionBackend> {
        vec![
            InjectionBackend::ClipboardAssistedPaste,
            InjectionBackend::ClipboardOnly,
        ]
    }

    fn inject(
        &self,
        text: &str,
        target: &FocusedApp,
        _strategy: InjectionStrategy,
    ) -> Result<InjectionReceipt, InjectError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(InjectError::AllBackendsFailed);
        }
        self.injected
            .lock()
            .map_err(|_| InjectError::Backend {
                backend: InjectionBackend::ClipboardAssistedPaste,
                reason: "fake injector mutex poisoned".to_owned(),
            })?
            .push((text.to_owned(), target.clone()));
        Ok(InjectionReceipt {
            backend: InjectionBackend::ClipboardAssistedPaste,
            verified: true,
        })
    }
}

/// An in-memory `ClipboardGuard` with a real change counter, mirroring
/// NSPasteboard `changeCount` semantics: every write (including restore)
/// bumps the counter.
#[derive(Default)]
pub struct FakeClipboardGuard {
    text: Mutex<Option<String>>,
    change_count: AtomicU64,
    /// `change_count` produced by our own most recent transient write, so
    /// restore can detect any intervening write regardless of how many
    /// transient writes the driver made.
    transient_change_count: AtomicU64,
}

impl FakeClipboardGuard {
    /// Simulate the user (or another app) writing the clipboard.
    pub fn user_write(&self, text: &str) -> Result<(), ClipboardError> {
        self.write(text).map(|_| ())
    }

    fn write(&self, text: &str) -> Result<u64, ClipboardError> {
        let mut guard = self.text.lock().map_err(|_| ClipboardError::Unavailable)?;
        *guard = Some(text.to_owned());
        Ok(self.change_count.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

impl ClipboardGuard for FakeClipboardGuard {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
        let guard = self.text.lock().map_err(|_| ClipboardError::Unavailable)?;
        Ok(ClipboardSnapshot {
            change_count: self.change_count.load(Ordering::SeqCst),
            text: guard.clone(),
        })
    }

    fn set_transient_text(&self, text: &str) -> Result<(), ClipboardError> {
        let change_count = self.write(text)?;
        self.transient_change_count
            .store(change_count, Ordering::SeqCst);
        Ok(())
    }

    fn restore_if_unchanged(
        &self,
        snapshot: ClipboardSnapshot,
    ) -> Result<RestoreOutcome, ClipboardError> {
        // Anything written after our own last transient write means the user
        // or another app wrote in between, and their content wins.
        let mut guard = self.text.lock().map_err(|_| ClipboardError::Unavailable)?;
        if self.change_count.load(Ordering::SeqCst)
            > self.transient_change_count.load(Ordering::SeqCst)
        {
            return Ok(RestoreOutcome::UserModified);
        }
        *guard = snapshot.text;
        self.change_count.fetch_add(1, Ordering::SeqCst);
        Ok(RestoreOutcome::Restored)
    }
}

/// A `PermissionProbe` with per-capability scripted states (default Granted).
#[derive(Default)]
pub struct FakePermissionProbe {
    overrides: Mutex<Vec<(Capability, PermissionState)>>,
}

impl FakePermissionProbe {
    pub fn set(&self, capability: Capability, state: PermissionState) {
        if let Ok(mut overrides) = self.overrides.lock() {
            overrides.retain(|(existing, _)| *existing != capability);
            overrides.push((capability, state));
        }
    }
}

impl PermissionProbe for FakePermissionProbe {
    fn probe(&self, capability: Capability) -> PermissionState {
        self.overrides
            .lock()
            .ok()
            .and_then(|overrides| {
                overrides
                    .iter()
                    .find(|(existing, _)| *existing == capability)
                    .map(|(_, state)| *state)
            })
            .unwrap_or(PermissionState::Granted)
    }
}

/// A `FocusTracker` whose frontmost app is set by the test.
pub struct FakeFocusTracker {
    focused: Mutex<FocusedApp>,
}

impl Default for FakeFocusTracker {
    fn default() -> Self {
        Self {
            focused: Mutex::new(FocusedApp {
                app_id: "com.example.editor".to_owned(),
                window_title: Some("Untitled".to_owned()),
            }),
        }
    }
}

impl FakeFocusTracker {
    pub fn set_focused(&self, app: FocusedApp) {
        if let Ok(mut focused) = self.focused.lock() {
            *focused = app;
        }
    }
}

impl FocusTracker for FakeFocusTracker {
    fn focused_app(&self) -> Result<FocusedApp, FocusError> {
        self.focused
            .lock()
            .map(|focused| focused.clone())
            .map_err(|_| FocusError::Unknown)
    }
}

/// An in-memory `Autostart`.
#[derive(Default)]
pub struct FakeAutostart {
    enabled: AtomicBool,
}

impl Autostart for FakeAutostart {
    fn is_enabled(&self) -> Result<bool, AutostartError> {
        Ok(self.enabled.load(Ordering::SeqCst))
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        self.enabled.store(enabled, Ordering::SeqCst);
        Ok(())
    }
}
