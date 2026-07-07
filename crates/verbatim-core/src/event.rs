//! The core event bus (ARCHITECTURE.md 4.9): one typed `Event` enum on a
//! `tokio::sync::broadcast`. The overlay, tray, logs, and webviews are pure
//! consumers; no surface queries state, they replay events.

use tokio::sync::broadcast;

use verbatim_platform::{Capability, PermissionState};

use crate::error::ErrorId;
use crate::session::{SessionId, SessionState};

#[derive(Debug, Clone)]
pub enum Event {
    SessionTransition {
        session: SessionId,
        from: SessionState,
        to: SessionState,
    },
    /// Live input level for the overlay waveform.
    InputLevel { rms: f32 },
    DownloadProgress {
        model_id: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    PermissionChanged {
        capability: Capability,
        state: PermissionState,
    },
    ErrorRaised {
        session: Option<SessionId>,
        id: ErrorId,
    },
    /// A dictation was delivered (verified injection). The app-layer history
    /// store persists these; core itself is path-free (ARCHITECTURE.md 4.8).
    /// `polished` is the injected polished text when polish ran, else `None`
    /// (raw-only, or polish degraded to raw).
    DictationRecorded {
        session: SessionId,
        app_id: String,
        raw: String,
        polished: Option<String>,
    },
}

pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish to all current subscribers; returns how many received it.
    /// Publishing with no subscribers is fine (e.g. headless CLI).
    pub fn publish(&self, event: Event) -> usize {
        self.sender.send(event).unwrap_or(0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_reach_all_subscribers() {
        let bus = EventBus::default();
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();

        let delivered = bus.publish(Event::SessionTransition {
            session: SessionId(1),
            from: SessionState::Idle,
            to: SessionState::Arming,
        });
        assert_eq!(delivered, 2);

        for receiver in [&mut first, &mut second] {
            match receiver.try_recv().unwrap() {
                Event::SessionTransition { from, to, .. } => {
                    assert_eq!(from, SessionState::Idle);
                    assert_eq!(to, SessionState::Arming);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }

    #[test]
    fn publish_without_subscribers_is_not_an_error() {
        let bus = EventBus::default();
        assert_eq!(bus.publish(Event::InputLevel { rms: 0.5 }), 0);
    }
}
