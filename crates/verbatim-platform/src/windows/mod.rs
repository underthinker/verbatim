//! Windows implementations (windows doc stub in ROADMAP M1, plan phase 7).
//!
//! Injection chain, in probe order: `SendInput` `KEYEVENTF_UNICODE` typing ->
//! clipboard + synthesized Ctrl-V paste -> clipboard-only. `SendInput` reports
//! how many events the system actually inserted, so a short insert (UIPI
//! blocking input into an elevated window) is detected as an honest failure
//! and falls through the chain instead of being sold as success (E4).
//!
//! Clipboard discipline mirrors the macOS guard: snapshot, stage the dictated
//! text flagged with `ExcludeClipboardContentFromMonitorProcessing` so
//! clipboard managers skip it, paste, then restore keyed on
//! `GetClipboardSequenceNumber` - the user's clipboard survives unless they
//! changed it mid-flight, in which case their content wins.
//!
//! Hotkeys: `RegisterHotKey` chords on a dedicated message-loop thread (edges
//! synthesized from `WM_HOTKEY` plus `GetAsyncKeyState` release polling, since
//! `RegisterHotKey` only reports presses).

#[cfg(feature = "win-inject")]
mod clipboard;
#[cfg(feature = "win-inject")]
mod focus;
#[cfg(feature = "win-inject")]
mod hotkey;
#[cfg(feature = "win-inject")]
mod inject;
#[cfg(feature = "win-inject")]
mod permission;

#[cfg(feature = "win-inject")]
pub use clipboard::WinClipboardGuard;
#[cfg(feature = "win-inject")]
pub use focus::WinFocusTracker;
#[cfg(feature = "win-inject")]
pub use hotkey::WinHotkeyBackend;
#[cfg(feature = "win-inject")]
pub use inject::WinTextInjector;
#[cfg(feature = "win-inject")]
pub use permission::WinPermissionProbe;
