//! Serde DTOs mirroring `verbatim_core::event::Event` for the webview event
//! bridge (ARCHITECTURE.md 4.9): the core bus is forwarded to the Tauri event
//! system 1:1, and these are the wire shapes the TypeScript side types against
//! (`ui/src/events.ts`).
//!
//! Core stays serde-free on purpose: the bus is an in-process contract, and
//! this adapter is the single place its shapes are committed to a wire format.

use serde::Serialize;

use verbatim_core::event::Event;
use verbatim_core::session::SessionState;

/// The single Tauri event channel carrying every core bus event.
pub const EVENT_CHANNEL: &str = "verbatim://event";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateDto {
    name: &'static str,
    /// UX error catalog ID (E1-E10); set only when `name` is `"failed"`.
    error: Option<String>,
}

impl From<SessionState> for SessionStateDto {
    fn from(state: SessionState) -> Self {
        let (name, error) = match state {
            SessionState::Idle => ("idle", None),
            SessionState::Arming => ("arming", None),
            SessionState::Recording => ("recording", None),
            SessionState::Finalizing => ("finalizing", None),
            SessionState::Transcribing => ("transcribing", None),
            SessionState::Polishing => ("polishing", None),
            SessionState::Injecting => ("injecting", None),
            SessionState::Failed(id) => ("failed", Some(format!("{id:?}"))),
        };
        Self { name, error }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiEvent {
    SessionTransition {
        session: u64,
        from: SessionStateDto,
        to: SessionStateDto,
    },
    InputLevel {
        rms: f32,
    },
    DownloadProgress {
        model_id: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    PermissionChanged {
        capability: String,
        state: String,
    },
    ErrorRaised {
        session: Option<u64>,
        id: String,
    },
    DictationRecorded {
        session: u64,
        app_id: String,
        raw: String,
        polished: Option<String>,
    },
}

impl From<Event> for UiEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::SessionTransition { session, from, to } => UiEvent::SessionTransition {
                session: session.0,
                from: from.into(),
                to: to.into(),
            },
            Event::InputLevel { rms } => UiEvent::InputLevel { rms },
            Event::DownloadProgress {
                model_id,
                received_bytes,
                total_bytes,
            } => UiEvent::DownloadProgress {
                model_id,
                received_bytes,
                total_bytes,
            },
            Event::PermissionChanged { capability, state } => UiEvent::PermissionChanged {
                capability: format!("{capability:?}"),
                state: format!("{state:?}"),
            },
            Event::ErrorRaised { session, id } => UiEvent::ErrorRaised {
                session: session.map(|s| s.0),
                id: format!("{id:?}"),
            },
            Event::DictationRecorded {
                session,
                app_id,
                raw,
                polished,
            } => UiEvent::DictationRecorded {
                session: session.0,
                app_id,
                raw,
                polished,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verbatim_core::error::ErrorId;
    use verbatim_core::session::SessionId;

    fn json(event: Event) -> serde_json::Value {
        // Test-only: DTOs are plain data, serialization cannot fail.
        #[allow(clippy::unwrap_used)]
        serde_json::to_value(UiEvent::from(event)).unwrap()
    }

    #[test]
    fn session_transition_matches_the_ts_contract() {
        let value = json(Event::SessionTransition {
            session: SessionId(7),
            from: SessionState::Recording,
            to: SessionState::Failed(ErrorId::E4),
        });
        assert_eq!(
            value,
            serde_json::json!({
                "type": "sessionTransition",
                "session": 7,
                "from": { "name": "recording", "error": null },
                "to": { "name": "failed", "error": "E4" },
            })
        );
    }

    #[test]
    fn download_progress_uses_camel_case_fields() {
        let value = json(Event::DownloadProgress {
            model_id: "whisper-base".into(),
            received_bytes: 10,
            total_bytes: None,
        });
        assert_eq!(
            value,
            serde_json::json!({
                "type": "downloadProgress",
                "modelId": "whisper-base",
                "receivedBytes": 10,
                "totalBytes": null,
            })
        );
    }

    #[test]
    fn error_raised_carries_the_catalog_id() {
        let value = json(Event::ErrorRaised {
            session: None,
            id: ErrorId::E9,
        });
        assert_eq!(
            value,
            serde_json::json!({
                "type": "errorRaised",
                "session": null,
                "id": "E9",
            })
        );
    }
}
