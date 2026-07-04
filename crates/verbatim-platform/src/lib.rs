//! Platform layer: trait definitions the core depends on, plus per-OS
//! implementations (ARCHITECTURE.md section 1).
//!
//! Rules: implementations may not leak OS types across the trait boundary,
//! and anything with `#[cfg(target_os)]` lives here, never in core.

#[cfg(feature = "cpal-audio")]
pub mod audio;
mod errors;
pub mod fake;
#[cfg(feature = "global-hotkey")]
pub mod hotkey;
#[cfg(all(feature = "global-hotkey", target_os = "macos"))]
pub mod modifier_tap;
mod traits;
mod types;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub use errors::*;
pub use traits::*;
pub use types::*;
