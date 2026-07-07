//! macOS implementations of the platform seams.
//!
//! The real backends (transient-pasteboard paste + CGEventPost unicode typing
//! injection, changeCount-aware clipboard restore, NSWorkspace focus tracking,
//! TCC/AX permission probes) are compiled only under the `mac-inject` feature
//! (ARCHITECTURE.md 4.4/4.6, spikes 1-2). Default and headless builds keep the
//! fakes and pull no AppKit/CoreGraphics.
//!
//! Hotkeys remain planned per spike 2: Carbon `RegisterEventHotKey` (no TCC).

#[cfg(feature = "mac-inject")]
mod announce;
#[cfg(feature = "mac-inject")]
mod clipboard;
#[cfg(feature = "mac-inject")]
mod ffi;
#[cfg(feature = "mac-inject")]
mod focus;
#[cfg(feature = "mac-inject")]
mod inject;
#[cfg(feature = "mac-inject")]
mod permission;

#[cfg(feature = "mac-inject")]
pub use announce::MacAnnouncer;
#[cfg(feature = "mac-inject")]
pub use clipboard::MacClipboardGuard;
#[cfg(feature = "mac-inject")]
pub use focus::MacFocusTracker;
#[cfg(feature = "mac-inject")]
pub use inject::MacTextInjector;
#[cfg(feature = "mac-inject")]
pub use permission::MacPermissionProbe;
