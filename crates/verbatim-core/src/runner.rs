//! The session runner: the actor that owns one `DictationSession` and drives
//! it through the real pipeline stages behind the platform and engine traits
//! (ARCHITECTURE.md 4, ENGINEERING.md 4).
//!
//! One tokio task, one `mpsc` mailbox. Surfaces never touch the session
//! directly: they send trigger commands in and replay `Event`s out
//! (ARCHITECTURE.md 4.9). This absorbs the orchestration that previously lived
//! by hand in `walking_skeleton.rs`.
//!
//! The runner holds trait objects only - no OS types cross this boundary
//! (ARCHITECTURE.md 1). Real audio/ASR/injection impls swap in behind the same
//! traits during later M1 phases; the fakes remain the deterministic test seam.

use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use verbatim_engines::{
    PolishEngine, PolishOutcome, PolishProfile, TranscribeOptions, TranscriptionEngine,
};
use verbatim_platform::{AudioCapture, FocusTracker, InjectError, InjectionStrategy, TextInjector};

use crate::error::ErrorId;
use crate::event::{Event, EventBus};
use crate::session::{DictationSession, SessionId, SessionInput, SessionState};

/// Mailbox depth. Triggers are rare (human-paced) and processed to completion
/// one at a time, so a shallow bounded queue is plenty and keeps backpressure
/// honest.
const MAILBOX_CAPACITY: usize = 16;

/// A trigger the runner acts on. `Start`/`Stop`/`Toggle` are what the hotkey
/// layer and the CLI/IPC deliver; `Cancel` is the local ESC discard and is
/// deliberately never exposed over the trigger IPC (ENGINEERING.md 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Start,
    Stop,
    Toggle,
    Cancel,
}

/// A point-in-time snapshot of the running session, answered to `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerStatus {
    pub session: SessionId,
    pub state: SessionState,
}

/// Runtime knobs for the slice. Polish stays off by default (raw injection
/// path); the polished branch is fully wired for when a real polish engine and
/// per-app profiles land (M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerConfig {
    pub polish: bool,
    pub polish_deadline: Duration,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            polish: false,
            polish_deadline: Duration::from_millis(1500),
        }
    }
}

/// The platform and engine seams the runner drives, bundled so construction is
/// not a positional soup. All are trait objects: the runner is pure core.
pub struct RunnerDeps {
    pub audio: Box<dyn AudioCapture>,
    pub transcription: Box<dyn TranscriptionEngine>,
    pub polish: Box<dyn PolishEngine>,
    pub injector: Box<dyn TextInjector>,
    pub focus: Box<dyn FocusTracker>,
}

enum RunnerCommand {
    Trigger(Trigger),
    Query(oneshot::Sender<RunnerStatus>),
}

/// A cloneable handle to a running `SessionRunner`. Every surface (hotkey core,
/// CLI/IPC client, tests) drives the session exclusively through this.
#[derive(Clone)]
pub struct RunnerHandle {
    tx: mpsc::Sender<RunnerCommand>,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("session runner has stopped")]
    Stopped,
}

impl RunnerHandle {
    /// Deliver a trigger. Returns once the command is enqueued; the runner
    /// processes triggers in order, so a following `status` observes the
    /// completed effect of this one.
    pub async fn trigger(&self, trigger: Trigger) -> Result<(), RunnerError> {
        self.tx
            .send(RunnerCommand::Trigger(trigger))
            .await
            .map_err(|_| RunnerError::Stopped)
    }

    /// Ask the runner for its current session state.
    pub async fn status(&self) -> Result<RunnerStatus, RunnerError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(RunnerCommand::Query(reply_tx))
            .await
            .map_err(|_| RunnerError::Stopped)?;
        reply_rx.await.map_err(|_| RunnerError::Stopped)
    }
}

/// The actor. Construct with [`SessionRunner::new`], spawn [`SessionRunner::run`]
/// on a tokio task, and drive it through the returned [`RunnerHandle`].
pub struct SessionRunner {
    session: DictationSession,
    next_id: u64,
    audio: Box<dyn AudioCapture>,
    transcription: Box<dyn TranscriptionEngine>,
    polish: Box<dyn PolishEngine>,
    injector: Box<dyn TextInjector>,
    focus: Box<dyn FocusTracker>,
    config: RunnerConfig,
    events: Arc<EventBus>,
    rx: mpsc::Receiver<RunnerCommand>,
}

impl SessionRunner {
    pub fn new(
        deps: RunnerDeps,
        config: RunnerConfig,
        events: Arc<EventBus>,
    ) -> (Self, RunnerHandle) {
        let (tx, rx) = mpsc::channel(MAILBOX_CAPACITY);
        let runner = Self {
            session: DictationSession::new(SessionId(1)),
            next_id: 1,
            audio: deps.audio,
            transcription: deps.transcription,
            polish: deps.polish,
            injector: deps.injector,
            focus: deps.focus,
            config,
            events,
            rx,
        };
        (runner, RunnerHandle { tx })
    }

    /// Own the actor task until every handle is dropped.
    pub async fn run(mut self) {
        while let Some(command) = self.rx.recv().await {
            match command {
                RunnerCommand::Trigger(trigger) => self.handle_trigger(trigger),
                RunnerCommand::Query(reply) => {
                    let snapshot = RunnerStatus {
                        session: self.session.id(),
                        state: self.session.state(),
                    };
                    // A dropped receiver just means the asker gave up.
                    let _ = reply.send(snapshot);
                }
            }
        }
    }

    fn handle_trigger(&mut self, trigger: Trigger) {
        match trigger {
            Trigger::Start => self.on_start(),
            Trigger::Stop => self.on_stop(),
            Trigger::Toggle => match self.session.state() {
                SessionState::Recording => self.on_stop(),
                _ if self.is_active() => {
                    tracing::debug!(state = ?self.session.state(), "toggle ignored mid-transition");
                }
                _ => self.on_start(),
            },
            Trigger::Cancel => self.on_cancel(),
        }
    }

    fn on_start(&mut self) {
        if self.is_active() {
            tracing::debug!(state = ?self.session.state(), "start ignored: session already active");
            return;
        }
        self.reset_session();
        self.begin_recording();
    }

    fn on_stop(&mut self) {
        if self.session.state() != SessionState::Recording {
            tracing::debug!(state = ?self.session.state(), "stop ignored: not recording");
            return;
        }
        self.finish_recording();
    }

    fn on_cancel(&mut self) {
        match self.session.state() {
            SessionState::Arming | SessionState::Recording => {
                self.audio.abort();
                self.step(SessionInput::Cancel);
            }
            _ => {
                tracing::debug!(state = ?self.session.state(), "cancel ignored: nothing to discard")
            }
        }
    }

    /// Arm and open the mic stream, leaving the session in `Recording`. The
    /// recording stays open across the mailbox until a `Stop`/`Toggle` arrives.
    fn begin_recording(&mut self) {
        if self.step(SessionInput::HotkeyEngaged).is_none() {
            return;
        }
        if let Err(err) = self.audio.start() {
            tracing::warn!(?err, "audio start failed");
            self.fault(ErrorId::E1);
            return;
        }
        self.step(SessionInput::StreamOpened);
    }

    /// Stop capture and run the pipeline to injection: finalize -> transcribe
    /// -> (optional polish) -> inject. Every failure is mapped to the UX error
    /// catalog; injection never reports unverifiable success.
    fn finish_recording(&mut self) {
        if self.step(SessionInput::StopRequested).is_none() {
            return;
        }
        let audio = match self.audio.stop() {
            Ok(audio) => audio,
            Err(err) => {
                tracing::warn!(?err, "audio stop failed");
                self.fault(ErrorId::E6);
                return;
            }
        };
        if self.step(SessionInput::TailFlushed).is_none() {
            return;
        }
        let transcript = match self
            .transcription
            .transcribe(&audio, &TranscribeOptions::default())
        {
            Ok(transcript) => transcript,
            Err(err) => {
                tracing::error!(?err, "transcription failed");
                self.fault(ErrorId::E3);
                return;
            }
        };
        let raw = transcript.text();

        let want_polish = self.config.polish;
        if self
            .step(SessionInput::TranscriptReady {
                polish: want_polish,
            })
            .is_none()
        {
            return;
        }
        let text = if want_polish {
            self.run_polish(&raw)
        } else {
            raw
        };

        self.inject(&text);
    }

    /// Drive the `Polishing` state to `Injecting`, returning the text to inject.
    /// Polish never blocks: rejection or engine failure degrades to raw
    /// (UX.md 2, E10 is a notice not a dead end).
    fn run_polish(&mut self, raw: &str) -> String {
        match self
            .polish
            .polish(raw, &PolishProfile::default(), self.config.polish_deadline)
        {
            Ok(PolishOutcome::Polished { text }) => {
                self.step(SessionInput::PolishReady);
                text
            }
            Ok(PolishOutcome::Rejected { reason }) => {
                tracing::info!(?reason, "polish rejected; injecting raw");
                self.step(SessionInput::PolishSkipped);
                raw.to_owned()
            }
            Err(err) => {
                tracing::warn!(?err, "polish engine failed; injecting raw");
                self.fault(ErrorId::E10);
                raw.to_owned()
            }
        }
    }

    /// Inject from `Injecting`. Verified delivery walks to `Idle`; anything
    /// else routes to the honest-failure catalog (E5 secure field, E7 focus
    /// lost, E4 everything else) and leaves the session `Failed`.
    fn inject(&mut self, text: &str) {
        let target = match self.focus.focused_app() {
            Ok(target) => target,
            Err(err) => {
                tracing::warn!(?err, "focus target unknown");
                self.fault(ErrorId::E7);
                return;
            }
        };
        match self.injector.inject(text, &target, InjectionStrategy::Auto) {
            Ok(receipt) if receipt.verified => {
                self.step(SessionInput::Injected);
            }
            Ok(receipt) => {
                tracing::warn!(backend = ?receipt.backend, "injection reported unverified");
                self.fault(ErrorId::E4);
            }
            Err(InjectError::SecureInput) => {
                tracing::info!("secure input field focused; dictation paused");
                self.fault(ErrorId::E5);
            }
            Err(err) => {
                tracing::warn!(?err, "injection failed; clipboard fallback");
                self.fault(ErrorId::E4);
            }
        }
    }

    fn is_active(&self) -> bool {
        !matches!(
            self.session.state(),
            SessionState::Idle | SessionState::Failed(_)
        )
    }

    fn reset_session(&mut self) {
        self.next_id += 1;
        self.session = DictationSession::new(SessionId(self.next_id));
    }

    fn fault(&mut self, id: ErrorId) {
        self.step(SessionInput::Fault(id));
    }

    /// Apply an input, publish the transition (and any error), and return the
    /// new state. An illegal transition is a runner bug: log-fatal in debug,
    /// swallowed in release with no state change (session.rs contract).
    fn step(&mut self, input: SessionInput) -> Option<SessionState> {
        let from = self.session.state();
        match self.session.handle(input) {
            Ok(to) => {
                self.events.publish(Event::SessionTransition {
                    session: self.session.id(),
                    from,
                    to,
                });
                if let SessionState::Failed(id) = to {
                    self.events.publish(Event::ErrorRaised {
                        session: Some(self.session.id()),
                        id,
                    });
                }
                Some(to)
            }
            Err(err) => {
                tracing::error!(%err, "illegal session transition in runner");
                debug_assert!(false, "illegal session transition in runner: {err}");
                None
            }
        }
    }
}
