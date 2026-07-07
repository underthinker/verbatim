//! Application shell library: everything the `verbatim` binary is made of.
//!
//! Split out of the binary so integration tests can exercise shell components
//! directly (e.g. the overlay window property assertions, which need to build
//! the real Tauri window on their own main thread).
//!
//! Security (ENGINEERING.md 8): the trigger IPC accepts trigger verbs only,
//! never text payloads; other processes must never be able to inject text
//! through us. The wire protocol enforces this - see `ipc`.

#![forbid(unsafe_code)]

pub mod bridge;
pub mod client;
pub mod config;
pub mod daemon;
pub mod error_catalog;
pub mod gui;
pub mod ipc;
pub mod onboarding;
pub mod overlay;
pub mod settings;
pub mod transport;
