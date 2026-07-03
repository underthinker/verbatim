//! The dictation session state machine, exactly mirroring UX.md section 2
//! (see ARCHITECTURE.md section 3).
//!
//! An explicit enum plus transition table: illegal transitions are
//! compile-time-visible here and must be treated as log-fatal in debug builds
//! by the driving actor. Concurrency (one recording + bounded pipeline tails)
//! is the session manager's job, not this machine's; each session moves
//! through this lifecycle alone.

use thiserror::Error;

use crate::error::ErrorId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// UX.md 2 states. `Cancel` returns straight to `Idle` (discard) per the UX
/// diagram; `Failed` is terminal for the session and carries the catalog ID
/// the surfaces present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    Idle,
    Arming,
    Recording,
    Finalizing,
    Transcribing,
    Polishing,
    Injecting,
    Failed(ErrorId),
}

/// Everything that can drive a session forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionInput {
    /// Hotkey engaged while idle (already debounced by the hotkey semantics
    /// layer: 250 ms accidental-press threshold, hold/toggle/lock).
    HotkeyEngaged,
    /// Mic stream opened.
    StreamOpened,
    /// Hotkey released (hold), pressed again (toggle), or 5 min cap hit.
    StopRequested,
    /// Input device disconnected mid-recording: stop gracefully and
    /// transcribe what was captured (E6 is a notice, not a dead end).
    DeviceLost,
    /// ESC pressed: discard and return to idle.
    Cancel,
    /// VAD tail flushed; audio is complete.
    TailFlushed,
    /// VAD saw no speech at all ("didn't catch anything", soft return).
    SilenceOnly,
    /// Transcript ready; `polish` reflects mode and per-app profile.
    TranscriptReady { polish: bool },
    /// Polished text ready within deadline and similarity guard.
    PolishReady,
    /// Polish rejected (deadline, similarity guard, engine unavailable):
    /// inject raw instead, never block (UX.md 2).
    PolishSkipped,
    /// Text verifiably delivered to the target app.
    Injected,
    /// A failure mapped to the UX error catalog.
    Fault(ErrorId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("illegal session transition: {input:?} while {from:?}")]
pub struct IllegalTransition {
    pub from: SessionState,
    pub input: SessionInput,
}

impl SessionState {
    /// The transition table. Anything not listed is illegal.
    pub fn apply(self, input: SessionInput) -> Result<SessionState, IllegalTransition> {
        use SessionInput as I;
        use SessionState as S;

        let next = match (self, input) {
            (S::Idle, I::HotkeyEngaged) => S::Arming,

            (S::Arming, I::StreamOpened) => S::Recording,
            (S::Arming, I::Cancel) => S::Idle,

            (S::Recording, I::StopRequested) => S::Finalizing,
            (S::Recording, I::DeviceLost) => S::Finalizing,
            (S::Recording, I::Cancel) => S::Idle,

            (S::Finalizing, I::TailFlushed) => S::Transcribing,
            (S::Finalizing, I::SilenceOnly) => S::Idle,

            (S::Transcribing, I::TranscriptReady { polish: true }) => S::Polishing,
            (S::Transcribing, I::TranscriptReady { polish: false }) => S::Injecting,

            (S::Polishing, I::PolishReady) => S::Injecting,
            (S::Polishing, I::PolishSkipped) => S::Injecting,
            // Polish never blocks: any fault there degrades to raw injection
            // (UX.md 2 POLISHING row; E10 is a tray notice, not a dead end).
            (S::Polishing, I::Fault(_)) => S::Injecting,

            (S::Injecting, I::Injected) => S::Idle,

            (
                S::Arming | S::Recording | S::Finalizing | S::Transcribing | S::Injecting,
                I::Fault(id),
            ) => S::Failed(id),

            (from, input) => return Err(IllegalTransition { from, input }),
        };
        Ok(next)
    }
}

/// One dictation session: identity plus current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictationSession {
    id: SessionId,
    state: SessionState,
}

impl DictationSession {
    pub fn new(id: SessionId) -> Self {
        Self {
            id,
            state: SessionState::Idle,
        }
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Apply an input, advancing the session. Returns the new state.
    pub fn handle(&mut self, input: SessionInput) -> Result<SessionState, IllegalTransition> {
        self.state = self.state.apply(input)?;
        Ok(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every reachable state, `Failed` represented once per catalog ID.
    fn all_states() -> Vec<SessionState> {
        let mut states = vec![
            SessionState::Idle,
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Transcribing,
            SessionState::Polishing,
            SessionState::Injecting,
        ];
        states.extend(ErrorId::ALL.map(SessionState::Failed));
        states
    }

    /// Every input, parametrized variants expanded.
    fn all_inputs() -> Vec<SessionInput> {
        let mut inputs = vec![
            SessionInput::HotkeyEngaged,
            SessionInput::StreamOpened,
            SessionInput::StopRequested,
            SessionInput::DeviceLost,
            SessionInput::Cancel,
            SessionInput::TailFlushed,
            SessionInput::SilenceOnly,
            SessionInput::TranscriptReady { polish: true },
            SessionInput::TranscriptReady { polish: false },
            SessionInput::PolishReady,
            SessionInput::PolishSkipped,
            SessionInput::Injected,
        ];
        inputs.extend(ErrorId::ALL.map(SessionInput::Fault));
        inputs
    }

    /// The specification restated as data: the complete set of legal
    /// transitions per UX.md section 2. The exhaustive test below asserts
    /// that the implementation allows exactly these and nothing else.
    fn legal_transitions() -> Vec<(SessionState, SessionInput, SessionState)> {
        use SessionInput as I;
        use SessionState as S;

        let mut legal = vec![
            (S::Idle, I::HotkeyEngaged, S::Arming),
            (S::Arming, I::StreamOpened, S::Recording),
            (S::Arming, I::Cancel, S::Idle),
            (S::Recording, I::StopRequested, S::Finalizing),
            (S::Recording, I::DeviceLost, S::Finalizing),
            (S::Recording, I::Cancel, S::Idle),
            (S::Finalizing, I::TailFlushed, S::Transcribing),
            (S::Finalizing, I::SilenceOnly, S::Idle),
            (
                S::Transcribing,
                I::TranscriptReady { polish: true },
                S::Polishing,
            ),
            (
                S::Transcribing,
                I::TranscriptReady { polish: false },
                S::Injecting,
            ),
            (S::Polishing, I::PolishReady, S::Injecting),
            (S::Polishing, I::PolishSkipped, S::Injecting),
            (S::Injecting, I::Injected, S::Idle),
        ];
        for id in ErrorId::ALL {
            legal.push((S::Arming, I::Fault(id), S::Failed(id)));
            legal.push((S::Recording, I::Fault(id), S::Failed(id)));
            legal.push((S::Finalizing, I::Fault(id), S::Failed(id)));
            legal.push((S::Transcribing, I::Fault(id), S::Failed(id)));
            legal.push((S::Polishing, I::Fault(id), S::Injecting));
            legal.push((S::Injecting, I::Fault(id), S::Failed(id)));
        }
        legal
    }

    /// Exhaustive: every (state, input) pair either matches the spec table
    /// or is rejected (ARCHITECTURE.md 3: exhaustive illegal-transition tests).
    #[test]
    fn transition_table_is_exactly_the_specification() {
        let legal = legal_transitions();
        for state in all_states() {
            for input in all_inputs() {
                let expected = legal
                    .iter()
                    .find(|(from, by, _)| *from == state && *by == input)
                    .map(|(_, _, to)| *to);
                match (state.apply(input), expected) {
                    (Ok(actual), Some(to)) => {
                        assert_eq!(actual, to, "{state:?} + {input:?}");
                    }
                    (Err(err), None) => {
                        assert_eq!(err, IllegalTransition { from: state, input });
                    }
                    (Ok(actual), None) => {
                        panic!("{state:?} + {input:?} must be illegal, got {actual:?}")
                    }
                    (Err(_), Some(to)) => {
                        panic!("{state:?} + {input:?} must reach {to:?}, got illegal")
                    }
                }
            }
        }
    }

    #[test]
    fn failed_is_terminal() {
        for id in ErrorId::ALL {
            for input in all_inputs() {
                assert!(
                    SessionState::Failed(id).apply(input).is_err(),
                    "Failed({id}) must not transition on {input:?}"
                );
            }
        }
    }

    #[test]
    fn happy_path_raw_mode_walks_to_idle() {
        let mut session = DictationSession::new(SessionId(1));
        for (input, expected) in [
            (SessionInput::HotkeyEngaged, SessionState::Arming),
            (SessionInput::StreamOpened, SessionState::Recording),
            (SessionInput::StopRequested, SessionState::Finalizing),
            (SessionInput::TailFlushed, SessionState::Transcribing),
            (
                SessionInput::TranscriptReady { polish: false },
                SessionState::Injecting,
            ),
            (SessionInput::Injected, SessionState::Idle),
        ] {
            assert_eq!(session.handle(input).unwrap(), expected);
        }
    }

    #[test]
    fn happy_path_polished_mode_walks_to_idle() {
        let mut session = DictationSession::new(SessionId(2));
        for input in [
            SessionInput::HotkeyEngaged,
            SessionInput::StreamOpened,
            SessionInput::StopRequested,
            SessionInput::TailFlushed,
            SessionInput::TranscriptReady { polish: true },
            SessionInput::PolishReady,
            SessionInput::Injected,
        ] {
            session.handle(input).unwrap();
        }
        assert_eq!(session.state(), SessionState::Idle);
    }
}
