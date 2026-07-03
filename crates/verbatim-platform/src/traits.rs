use verbatim_engines::AudioBuffer;

use crate::errors::{
    AutostartError, CaptureError, ClipboardError, FocusError, HotkeyError, InjectError,
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

/// Frontmost-app tracking for the focus rule (E7) and per-app profiles.
pub trait FocusTracker: Send + Sync {
    fn focused_app(&self) -> Result<FocusedApp, FocusError>;
}

/// Launch-at-login management.
pub trait Autostart: Send + Sync {
    fn is_enabled(&self) -> Result<bool, AutostartError>;

    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError>;
}
