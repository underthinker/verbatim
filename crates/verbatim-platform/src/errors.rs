use thiserror::Error;

use crate::types::InjectionBackend;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("no input device available")]
    NoDevice,
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("input device lost")]
    DeviceLost,
    #[error("not capturing")]
    NotCapturing,
    #[error("audio backend error: {0}")]
    Backend(String),
}

#[derive(Debug, Error)]
pub enum InjectError {
    /// Secure input field focused; refuse silently and use the clipboard (E5).
    #[error("secure input field focused; refusing to inject")]
    SecureInput,
    #[error("no writable focus target")]
    NoWritableTarget,
    #[error("all probed injection backends failed")]
    AllBackendsFailed,
    #[error("injection backend {backend:?} failed: {reason}")]
    Backend {
        backend: InjectionBackend,
        reason: String,
    },
}

#[derive(Debug, Error)]
pub enum HotkeyError {
    #[error("a binding is already registered")]
    AlreadyRegistered,
    #[error("chord unavailable or already taken: {0}")]
    ChordUnavailable(String),
    #[error("hotkey backend error: {0}")]
    Backend(String),
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard unavailable")]
    Unavailable,
    #[error("clipboard backend error: {0}")]
    Backend(String),
}

#[derive(Debug, Error)]
pub enum FocusError {
    #[error("focused app could not be determined")]
    Unknown,
}

#[derive(Debug, Error)]
pub enum AutostartError {
    #[error("autostart backend error: {0}")]
    Backend(String),
}
