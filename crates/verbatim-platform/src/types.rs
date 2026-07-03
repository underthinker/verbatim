/// A hotkey chord in a cross-platform textual form, e.g. `Ctrl+Alt+Space`.
/// Parsing/validation lands with the real hotkey managers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotkeyBinding {
    pub chord: String,
}

/// Raw hotkey edge events. Hold/toggle/double-tap-lock semantics are
/// implemented once in core on top of these (ARCHITECTURE.md 4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

/// The frontmost application at a point in time (injection target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedApp {
    /// Stable per-OS application identifier (bundle id, exe name, app id).
    pub app_id: String,
    pub window_title: Option<String>,
}

/// Injection mechanisms across all platforms; `TextInjector::probe` returns
/// the capability-probed, ordered subset for the running system
/// (ARCHITECTURE.md 4.4, spikes 1 and 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InjectionBackend {
    /// macOS transient-pasteboard paste with changeCount-aware restore.
    TransientPasteboardPaste,
    /// macOS CGEventPost unicode typing.
    CgEventTyping,
    /// Windows SendInput with KEYEVENTF_UNICODE.
    SendInputUnicode,
    /// Linux libei via the RemoteDesktop portal.
    LibeiPortal,
    /// Linux /dev/uinput virtual keyboard.
    Uinput,
    /// wlroots virtual-keyboard-v1 protocol.
    WlrVirtualKeyboard,
    /// Clipboard set + synthesized paste chord.
    ClipboardAssistedPaste,
    /// Clipboard set only; the user pastes manually (last resort, E4).
    ClipboardOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionStrategy {
    /// Walk the probed backend chain, first healthy backend wins.
    Auto,
    /// A per-app profile pinned one backend.
    Pinned(InjectionBackend),
}

/// Honest receipt of what actually happened; never trust exit codes as
/// success (spike 1 silent-failure trap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InjectionReceipt {
    pub backend: InjectionBackend,
    /// Whether delivery was positively verified, not merely not-errored.
    pub verified: bool,
}

/// Opaque clipboard state used to restore user content after a paste-based
/// injection (transient/restore discipline, ARCHITECTURE.md 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    /// Monotonic change counter at snapshot time (NSPasteboard changeCount
    /// or equivalent).
    pub change_count: u64,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored,
    /// The user (or another app) changed the clipboard after our snapshot;
    /// their content wins and no restore happens.
    UserModified,
}

/// Capabilities the permission subsystem probes (ARCHITECTURE.md 4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Microphone,
    /// Accessibility (macOS), uinput/portal (Linux); nothing on Windows.
    TextInjection,
    /// Only needed for opt-in triggers like the macOS Globe key (spike 2).
    InputMonitoring,
}

/// Probed without prompting (spike 2 preflight APIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    Undetermined,
    NotNeeded,
}
