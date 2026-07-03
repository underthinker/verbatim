//! The top-level error taxonomy, mapped 1:1 to the UX error catalog
//! (UX.md section 4). Every user-visible failure carries one of these IDs.

use std::fmt;

/// UX-facing error identifiers E1-E10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorId {
    /// Microphone missing or permission denied.
    E1,
    /// No speech model downloaded.
    E2,
    /// Transcription engine crash/failure (recording preserved).
    E3,
    /// Text injection failed; text lands on the clipboard.
    E4,
    /// Secure input field focused; injection refused silently.
    E5,
    /// Input device disconnected mid-recording.
    E6,
    /// Focused app changed between hotkey-up and injection.
    E7,
    /// Model download failure (network, disk).
    E8,
    /// Linux: no injection permission (uinput/portal).
    E9,
    /// Polish model missing/failed while polish enabled (silent raw fallback).
    E10,
}

impl ErrorId {
    pub const ALL: [ErrorId; 10] = [
        ErrorId::E1,
        ErrorId::E2,
        ErrorId::E3,
        ErrorId::E4,
        ErrorId::E5,
        ErrorId::E6,
        ErrorId::E7,
        ErrorId::E8,
        ErrorId::E9,
        ErrorId::E10,
    ];

    /// Short internal summary; user-facing overlay copy lives with the UI.
    pub fn summary(self) -> &'static str {
        match self {
            ErrorId::E1 => "microphone missing or permission denied",
            ErrorId::E2 => "no speech model downloaded",
            ErrorId::E3 => "transcription engine failure (recording preserved)",
            ErrorId::E4 => "text injection failed (clipboard fallback)",
            ErrorId::E5 => "secure input field focused (injection refused)",
            ErrorId::E6 => "input device disconnected mid-recording",
            ErrorId::E7 => "focus changed between recording and injection",
            ErrorId::E8 => "model download failure",
            ErrorId::E9 => "no injection permission on Linux",
            ErrorId::E10 => "polish unavailable (silent raw fallback)",
        }
    }
}

impl fmt::Display for ErrorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
