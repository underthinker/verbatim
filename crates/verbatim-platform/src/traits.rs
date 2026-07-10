use verbatim_engines::AudioBuffer;

use crate::errors::{
    AutostartError, CaptureError, ClipboardError, FocusError, HotkeyError, InjectError,
    PermissionRequestError,
};
use crate::types::{
    Capability, ClipboardSnapshot, FocusedApp, HotkeyBinding, HotkeyEvent, InjectionBackend,
    InjectionReceipt, InjectionStrategy, PermissionState, RestoreOutcome,
};

pub type HotkeyCallback = Box<dyn Fn(HotkeyEvent) + Send + Sync>;

/// Global hotkey registration delivering raw down/up events.
///
/// Hold/toggle/double-tap semantics live in core, once, on top of these
/// events (ARCHITECTURE.md 4.5).
pub trait HotkeyManager: Send + Sync {
    fn register(
        &mut self,
        binding: &HotkeyBinding,
        on_event: HotkeyCallback,
    ) -> Result<(), HotkeyError>;

    fn unregister(&mut self);
}

/// Microphone capture producing 16 kHz mono f32 buffers (ARCHITECTURE.md 4.1).
pub trait AudioCapture: Send + Sync {
    fn start(&mut self) -> Result<(), CaptureError>;

    /// Stop and return everything captured since `start`.
    fn stop(&mut self) -> Result<AudioBuffer, CaptureError>;

    /// Discard the in-flight recording (cancel path).
    fn abort(&mut self);

    fn is_capturing(&self) -> bool;

    /// RMS of the most recent audio the backend saw, in [0.0, 1.0]; 0.0 when
    /// not capturing.
    ///
    /// Polled by the core while Recording and republished as
    /// `Event::InputLevel` for the overlay waveform (ARCHITECTURE.md 4.1).
    /// The core cannot subscribe to the capture callback directly - the
    /// dependency runs core -> platform - so the level is pulled, not pushed.
    fn input_level(&self) -> f32;
}

/// Text injection with an honest receipt (ARCHITECTURE.md 4.4).
///
/// Implementations walk their platform backend chain in probe order and must
/// never report success they cannot verify (spike 1).
pub trait TextInjector: Send + Sync {
    /// Capability-probed backends, in fallback order.
    fn probe(&self) -> Vec<InjectionBackend>;

    fn inject(
        &self,
        text: &str,
        target: &FocusedApp,
        strategy: InjectionStrategy,
    ) -> Result<InjectionReceipt, InjectError>;
}

/// Snapshot/restore discipline around paste-based injection so user clipboard
/// content survives (ARCHITECTURE.md 4.4).
pub trait ClipboardGuard: Send + Sync {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError>;

    /// Place text marked transient (`org.nspasteboard.TransientType` or
    /// platform equivalent) so clipboard managers ignore it.
    fn set_transient_text(&self, text: &str) -> Result<(), ClipboardError>;

    /// Restore the snapshot unless the clipboard changed since (changeCount
    /// comparison); user modifications always win.
    fn restore_if_unchanged(
        &self,
        snapshot: ClipboardSnapshot,
    ) -> Result<RestoreOutcome, ClipboardError>;
}

/// Per-capability permission state, probed without prompting
/// (ARCHITECTURE.md 4.6, spike 2 preflight APIs).
pub trait PermissionProbe: Send + Sync {
    fn probe(&self, capability: Capability) -> PermissionState;
}

/// User-initiated permission requests (ARCHITECTURE.md 4.6; UX.md 6 onboarding
/// steps 2-3). Distinct from the read-only `PermissionProbe`: these surface OS
/// UI and are only ever called from an explicit user action. The re-check after
/// the OS UI closes is done by polling `PermissionProbe`, so these return once
/// the request is dispatched, not when the user decides.
pub trait PermissionRequest: Send + Sync {
    /// Trigger the OS permission prompt where the platform offers one
    /// (microphone). Capabilities without a direct prompt (macOS Accessibility,
    /// Linux typing) fall back to opening their settings pane.
    fn request(&self, capability: Capability) -> Result<(), PermissionRequestError>;

    /// Open the OS settings pane for `capability` so the user can grant it -
    /// the deep link for the onboarding re-check loop and the E1/E9 re-entry.
    fn open_settings(&self, capability: Capability) -> Result<(), PermissionRequestError>;
}

/// Frontmost-app tracking for the focus rule (E7) and per-app profiles.
pub trait FocusTracker: Send + Sync {
    fn focused_app(&self) -> Result<FocusedApp, FocusError>;
}

/// Screen-reader awareness for the overlay (UX.md 8). Lets the overlay driver
/// announce transient states through the OS accessibility notification API and
/// know when assistive tech is active so auto-dismiss timeouts are extended.
///
/// The overlay window is click-through and never focused, so its webview
/// `aria-live` region is not in any screen reader's monitored tree; the OS-level
/// announcement here is the only path that reaches VoiceOver / Narrator / Orca.
pub trait AccessibilityAnnouncer: Send + Sync {
    /// Whether a screen reader is currently running. Drives the default-on
    /// announcement behavior and the extended auto-dismiss timeouts.
    fn screen_reader_active(&self) -> bool;

    /// Post `message` as a transient announcement (spoken without moving focus).
    /// Best-effort: an announcement failure must never interrupt dictation, so
    /// this returns nothing and swallows OS errors after logging them.
    fn announce(&self, message: &str);
}

/// Launch-at-login management.
pub trait Autostart: Send + Sync {
    fn is_enabled(&self) -> Result<bool, AutostartError>;

    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError>;
}
