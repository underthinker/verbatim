//! Fake platform implementations: the deterministic test seam for core and
//! app tests (ENGINEERING.md section 4, E2E smoke).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use verbatim_engines::AudioBuffer;

use crate::errors::{
    AutostartError, CaptureError, ClipboardError, FocusError, HotkeyError, InjectError,
    PermissionRequestError,
};
use crate::traits::{
    AudioCapture, Autostart, ClipboardGuard, FocusTracker, HotkeyCallback, HotkeyManager,
    PermissionProbe, PermissionRequest, TextInjector,
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

/// What a `FakeTextInjector` does with every `inject`, mirroring the honest
/// receipt contract every real platform injector obeys (ARCHITECTURE.md 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FakeInjectionOutcome {
    /// Delivered and verified into the target app (typed or pasted).
    #[default]
    Verified,
    /// The active backends could not deliver (e.g. Windows UIPI block against
    /// an elevated window, or a Wayland portal denial), so the text was staged
    /// on the clipboard for the user to paste: `verified = false`, the E4 path.
    ClipboardFallback,
    /// A secure input field is focused; injection is refused and nothing is
    /// staged anywhere (E5).
    SecureField,
    /// Total failure: not even the clipboard fallback could stage the text.
    /// The extreme last case (clipboard write itself failed).
    HardFailure,
}

/// A `TextInjector` that records what it delivered; its outcome is scriptable so
/// tests can drive every honest-failure branch.
#[derive(Default)]
pub struct FakeTextInjector {
    injected: Mutex<Vec<(String, FocusedApp)>>,
    clipboard_staged: Mutex<Vec<String>>,
    outcome: Mutex<FakeInjectionOutcome>,
}

impl FakeTextInjector {
    /// Force a specific injection outcome for every subsequent `inject`.
    pub fn set_outcome(&self, outcome: FakeInjectionOutcome) {
        if let Ok(mut current) = self.outcome.lock() {
            *current = outcome;
        }
    }

    /// Back-compat shim: `true` is a total failure, `false` restores verified
    /// delivery. Prefer [`set_outcome`](Self::set_outcome) for the clipboard
    /// fallback and secure-field paths.
    pub fn set_failing(&self, failing: bool) {
        self.set_outcome(if failing {
            FakeInjectionOutcome::HardFailure
        } else {
            FakeInjectionOutcome::Verified
        });
    }

    /// Texts verifiably injected so far, in order.
    pub fn injected_texts(&self) -> Vec<String> {
        self.injected
            .lock()
            .map(|entries| entries.iter().map(|(text, _)| text.clone()).collect())
            .unwrap_or_default()
    }

    /// Texts staged on the clipboard by the fallback path, in order. Proves the
    /// text survived a failed primary injection (was not silently lost).
    pub fn clipboard_texts(&self) -> Vec<String> {
        self.clipboard_staged
            .lock()
            .map(|v| v.clone())
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
        let outcome = self
            .outcome
            .lock()
            .map(|o| *o)
            .unwrap_or(FakeInjectionOutcome::Verified);
        match outcome {
            FakeInjectionOutcome::Verified => {
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
            FakeInjectionOutcome::ClipboardFallback => {
                self.clipboard_staged
                    .lock()
                    .map_err(|_| InjectError::Backend {
                        backend: InjectionBackend::ClipboardOnly,
                        reason: "fake injector mutex poisoned".to_owned(),
                    })?
                    .push(text.to_owned());
                Ok(InjectionReceipt {
                    backend: InjectionBackend::ClipboardOnly,
                    verified: false,
                })
            }
            FakeInjectionOutcome::SecureField => Err(InjectError::SecureInput),
            FakeInjectionOutcome::HardFailure => Err(InjectError::AllBackendsFailed),
        }
    }
}

/// Share one `FakeTextInjector` between a runner (which takes ownership as a
/// `Box<dyn TextInjector>`) and a test that still wants to read what landed.
impl TextInjector for Arc<FakeTextInjector> {
    fn probe(&self) -> Vec<InjectionBackend> {
        (**self).probe()
    }

    fn inject(
        &self,
        text: &str,
        target: &FocusedApp,
        strategy: InjectionStrategy,
    ) -> Result<InjectionReceipt, InjectError> {
        (**self).inject(text, target, strategy)
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

/// One recorded call against a `FakePermissionRequester`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRequestCall {
    Request(Capability),
    OpenSettings(Capability),
}

/// A `PermissionRequest` that records every call and, on `request`, reflects
/// the outcome into a shared `FakePermissionProbe` - simulating the user
/// answering the OS prompt so the onboarding re-check loop (UX.md 6) runs end
/// to end without any OS UI. `open_settings` only records (the user grants out
/// of band; a test then flips the probe).
pub struct FakePermissionRequester {
    probe: Arc<FakePermissionProbe>,
    resolves_to: PermissionState,
    unsupported: Vec<Capability>,
    calls: Mutex<Vec<PermissionRequestCall>>,
}

impl FakePermissionRequester {
    /// Grant-on-request against `probe` (the common happy path).
    pub fn new(probe: Arc<FakePermissionProbe>) -> Self {
        Self {
            probe,
            resolves_to: PermissionState::Granted,
            unsupported: Vec::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Script the state a `request` resolves the capability to (e.g. `Denied`
    /// to exercise the E1/E9 re-entry path).
    pub fn resolving_to(mut self, state: PermissionState) -> Self {
        self.resolves_to = state;
        self
    }

    /// Capabilities that reject `request` as unsupported (e.g. Windows typing).
    pub fn unsupported(mut self, capabilities: Vec<Capability>) -> Self {
        self.unsupported = capabilities;
        self
    }

    pub fn calls(&self) -> Vec<PermissionRequestCall> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

impl PermissionRequest for FakePermissionRequester {
    fn request(&self, capability: Capability) -> Result<(), PermissionRequestError> {
        if self.unsupported.contains(&capability) {
            return Err(PermissionRequestError::Unsupported(capability));
        }
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(PermissionRequestCall::Request(capability));
        }
        self.probe.set(capability, self.resolves_to);
        Ok(())
    }

    fn open_settings(&self, capability: Capability) -> Result<(), PermissionRequestError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(PermissionRequestCall::OpenSettings(capability));
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    // Review fix: restore must key on the changeCount of our *own* last
    // transient write, so a retry that writes the clipboard twice does not
    // fool the guard into thinking the user intervened.

    #[test]
    fn restore_after_clean_transient_write_restores_original() {
        let guard = FakeClipboardGuard::default();
        guard.user_write("original").unwrap();
        let snap = guard.snapshot().unwrap();

        guard.set_transient_text("dictated text").unwrap();

        assert_eq!(
            guard.restore_if_unchanged(snap).unwrap(),
            RestoreOutcome::Restored
        );
        assert_eq!(guard.snapshot().unwrap().text.as_deref(), Some("original"));
    }

    #[test]
    fn restore_survives_a_retried_transient_write() {
        let guard = FakeClipboardGuard::default();
        guard.user_write("original").unwrap();
        let snap = guard.snapshot().unwrap();

        // First injection attempt fails; the driver retries and writes the
        // transient clipboard a second time. Only the latest write is "ours".
        guard
            .set_transient_text("dictated text (attempt 1)")
            .unwrap();
        guard
            .set_transient_text("dictated text (attempt 2)")
            .unwrap();

        assert_eq!(
            guard.restore_if_unchanged(snap).unwrap(),
            RestoreOutcome::Restored
        );
        assert_eq!(guard.snapshot().unwrap().text.as_deref(), Some("original"));
    }

    #[test]
    fn restore_yields_to_a_user_write_between_transient_and_restore() {
        let guard = FakeClipboardGuard::default();
        guard.user_write("original").unwrap();
        let snap = guard.snapshot().unwrap();

        guard.set_transient_text("dictated text").unwrap();
        // User copies something of their own before we restore.
        guard.user_write("user's own copy").unwrap();

        assert_eq!(
            guard.restore_if_unchanged(snap).unwrap(),
            RestoreOutcome::UserModified
        );
        assert_eq!(
            guard.snapshot().unwrap().text.as_deref(),
            Some("user's own copy")
        );
    }
}
