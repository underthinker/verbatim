//! Core layer: pure cross-platform Rust owning everything real — the session
//! state machine, pipelines, event bus, config, and history
//! (ARCHITECTURE.md section 1).
//!
//! Depends on the platform and engine layers through their traits only;
//! nothing here may carry `#[cfg(target_os)]`.

#![forbid(unsafe_code)]

pub mod error;
pub mod event;
pub mod runner;
pub mod session;
