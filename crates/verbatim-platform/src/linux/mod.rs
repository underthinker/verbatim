//! Linux implementations (spike 1).
//!
//! Injection chain, in probe order: RemoteDesktop portal keyboard injection
//! (the consent-ful path on GNOME >= 46 and Plasma >= 6.1, with
//! `restore_token` persistence for silent reconnect) -> `/dev/uinput` virtual
//! keyboard -> clipboard-only. Both event-capable backends deliver text as
//! clipboard-assisted paste (stage transient text, synthesize Ctrl-V), the
//! same discipline as the macOS transient-pasteboard backend. The wlroots
//! `virtual-keyboard-unstable-v1` leg of the spike-1 chain is deferred: the
//! M1 acceptance targets (GNOME Wayland, KDE) are covered by the portal and
//! uinput legs.
//!
//! Everything is in-process; never shell out to ydotool/wtype and never trust
//! exit codes as delivery (spike 1 silent-failure trap).
//!
//! Hotkeys: GlobalShortcuts portal (reliable on KDE; GNOME needs 48+, where
//! the documented fallback is a GNOME custom shortcut running
//! `verbatim trigger`).

#[cfg(feature = "linux-inject")]
mod clipboard;
#[cfg(feature = "linux-inject")]
mod focus;
#[cfg(feature = "linux-inject")]
mod hotkey;
#[cfg(feature = "linux-inject")]
mod inject;
#[cfg(feature = "linux-inject")]
mod permission;
#[cfg(feature = "linux-inject")]
mod portal;
#[cfg(feature = "linux-inject")]
mod uinput;

#[cfg(feature = "linux-inject")]
pub use clipboard::LinuxClipboardGuard;
#[cfg(feature = "linux-inject")]
pub use focus::LinuxFocusTracker;
#[cfg(feature = "linux-inject")]
pub use hotkey::PortalHotkeyBackend;
#[cfg(feature = "linux-inject")]
pub use inject::LinuxTextInjector;
#[cfg(feature = "linux-inject")]
pub use permission::LinuxPermissionProbe;
