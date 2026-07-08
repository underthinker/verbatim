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

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use verbatim_engines::{
    PolishEngine, PolishOutcome, PolishProfile, PolishRejection, TranscribeOptions,
    TranscriptionEngine,
};
use verbatim_platform::{
    AudioCapture, FocusTracker, FocusedApp, InjectError, InjectionStrategy, TextInjector,
};

use crate::error::ErrorId;
use crate::event::{Event, EventBus};
use crate::session::{DictationSession, SessionId, SessionInput, SessionState};

/// Mailbox depth. Triggers are rare (human-paced) and processed to completion
/// one at a time, so a shallow bounded queue is plenty and keeps backpressure
/// honest.
const MAILBOX_CAPACITY: usize = 16;

/// Reserved profile id: force raw injection for the matched app (UX.md 5.1).
pub const RAW_PROFILE: &str = "raw";
/// Profile applied to any app without an explicit assignment (Phase E gives it a
/// real prompt asset; until then it is an empty, temperature-0 polish profile).
const DEFAULT_PROFILE: &str = "default";

/// Built-in terminal app ids that default to raw (UX.md 5.1). An explicit entry
/// in [`RunnerConfig::profiles`] overrides this - a user can opt a terminal back
/// into polishing, or force raw on anything else.
const TERMINAL_APP_IDS: &[&str] = &[
    // macOS bundle ids.
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "io.alacritty",
    // Windows executable names.
    "WindowsTerminal.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    // Linux app ids / desktop names.
    "org.gnome.Terminal",
    "org.kde.konsole",
    "konsole",
    "alacritty",
    "kitty",
];

/// Pick the polish profile id for `app_id`, or `None` to force raw. Explicit
/// per-app assignments win; otherwise terminals default to raw and every other
/// app gets the default profile (ARCHITECTURE.md 4.3, UX.md 5.1).
fn select_profile(config: &RunnerConfig, app_id: &str) -> Option<String> {
    if let Some(id) = config.profiles.get(app_id) {
        return (id != RAW_PROFILE).then(|| id.clone());
    }
    if TERMINAL_APP_IDS.contains(&app_id) {
        return None;
    }
    Some(DEFAULT_PROFILE.to_owned())
}

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
    /// Paused: activation triggers (`Start`, `Toggle` from idle) are ignored
    /// until resumed. Surfaces so the tray can reflect and toggle it (UX.md 7).
    pub paused: bool,
}

/// Runtime knobs for the slice. Polish stays off by default (raw injection
/// path); the polished branch is fully wired for when a real polish engine and
/// per-app profiles land (M3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerConfig {
    pub polish: bool,
    pub polish_deadline: Duration,
    /// User-confirmed personal-dictionary terms (canonical casing), applied as a
    /// deterministic post-pass to the injected text (UX.md 5.3). Empty by default.
    pub dictionary: Vec<String>,
    /// Per-app polish profile assignments: frontmost app id -> profile id
    /// (UX.md 5.1). The reserved id [`RAW_PROFILE`] forces raw injection for that
    /// app; terminals default to raw even without an entry. Empty by default.
    pub profiles: BTreeMap<String, String>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            polish: false,
            polish_deadline: Duration::from_millis(1500),
            dictionary: Vec::new(),
            profiles: BTreeMap::new(),
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
    SetPaused(bool),
    /// Replace the runtime config in place (live re-apply of a saved config,
    /// Phase D). Applies to the next dictation; an in-flight one is untouched.
    Reconfigure(Box<RunnerConfig>),
    /// Force raw injection for the next dictation regardless of profile - the
    /// raw-mode modifier held with the hotkey (UX.md 5.1).
    SetRawOverride(bool),
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

    /// Pause or resume activation. While paused the runner ignores triggers
    /// that would start a new dictation; an in-flight session is untouched and
    /// `Stop`/`Cancel` still work (UX.md 7 "pause Verbatim").
    pub async fn set_paused(&self, paused: bool) -> Result<(), RunnerError> {
        self.tx
            .send(RunnerCommand::SetPaused(paused))
            .await
            .map_err(|_| RunnerError::Stopped)
    }

    /// Replace the runner's runtime config, live. The daemon/GUI re-send the
    /// projected persisted config on every save so polish, dictionary, and
    /// per-app profiles apply without a restart (Phase D). An in-flight dictation
    /// keeps the config it started with; the next one uses the new one.
    pub async fn reconfigure(&self, config: RunnerConfig) -> Result<(), RunnerError> {
        self.tx
            .send(RunnerCommand::Reconfigure(Box::new(config)))
            .await
            .map_err(|_| RunnerError::Stopped)
    }

    /// Force raw injection for the next dictation only (the raw-mode modifier,
    /// UX.md 5.1). Consumed when that dictation completes or is cancelled.
    pub async fn set_raw_override(&self, raw: bool) -> Result<(), RunnerError> {
        self.tx
            .send(RunnerCommand::SetRawOverride(raw))
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
    paused: bool,
    /// Latched by the raw-mode modifier; consumed by the next dictation.
    raw_override: bool,
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
            paused: false,
            raw_override: false,
        };
        (runner, RunnerHandle { tx })
    }

    /// Own the actor task until every handle is dropped.
    pub async fn run(mut self) {
        while let Some(command) = self.rx.recv().await {
            match command {
                RunnerCommand::Trigger(trigger) => self.handle_trigger(trigger),
                RunnerCommand::SetPaused(paused) => self.paused = paused,
                RunnerCommand::Reconfigure(config) => self.config = *config,
                RunnerCommand::SetRawOverride(raw) => self.raw_override = raw,
                RunnerCommand::Query(reply) => {
                    let snapshot = RunnerStatus {
                        session: self.session.id(),
                        state: self.session.state(),
                        paused: self.paused,
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
        if self.paused {
            tracing::debug!("start ignored: paused");
            return;
        }
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
                // A discarded dictation consumes any pending raw-mode latch too.
                self.raw_override = false;
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

        // Resolve the injection target once, up front: it both selects the
        // per-app polish profile and is the app we inject into - the frontmost
        // app at target time (ARCHITECTURE.md 4.3). A lost target is E7.
        let target = match self.focus.focused_app() {
            Ok(target) => target,
            Err(err) => {
                tracing::warn!(?err, "focus target unknown");
                self.fault(ErrorId::E7);
                return;
            }
        };

        // The raw-mode modifier forces raw for this one dictation regardless of
        // profile (UX.md 5.1); consume it so it never leaks into the next.
        let raw_override = std::mem::take(&mut self.raw_override);
        let profile =
            select_profile(&self.config, &target.app_id).map(|id| self.polish_profile(id));
        let want_polish = self.config.polish && !raw_override && profile.is_some();

        if self
            .step(SessionInput::TranscriptReady {
                polish: want_polish,
            })
            .is_none()
        {
            return;
        }
        let (text, did_polish) = match (want_polish, &profile) {
            (true, Some(profile)) => self.run_polish(&raw, profile),
            _ => (raw.clone(), false),
        };
        // Deterministic dictionary post-pass over whatever we inject - polished,
        // raw fallback, or raw-mode - so confirmed terms never depend on the LLM
        // (UX.md 5.3). `raw` recorded in history stays the verbatim transcript.
        let text = crate::dictionary::apply_dictionary(&text, &self.config.dictionary);

        // Record only on verified delivery: a failed injection is not history.
        if self.inject(&text, &target) {
            let polished = did_polish.then(|| text.clone());
            self.events.publish(Event::DictationRecorded {
                session: self.session.id(),
                app_id: target.app_id,
                raw,
                polished,
            });
        }
    }

    /// Build the [`PolishProfile`] for a selected profile id. Prompt/few-shot
    /// content stays empty until the versioned asset loader lands (Phase E); the
    /// personal dictionary is fed through so it reaches the prompt (ARCHITECTURE.md
    /// 4.3) in addition to the deterministic post-pass.
    fn polish_profile(&self, id: String) -> PolishProfile {
        PolishProfile {
            id,
            dictionary: self.config.dictionary.clone(),
            ..PolishProfile::default()
        }
    }

    /// Drive the `Polishing` state to `Injecting`, returning the text to inject
    /// and whether polish actually produced it. Polish never blocks: rejection
    /// or engine failure degrades to raw (UX.md 2, E10 is a notice not a dead
    /// end), reported as `false` so history records it raw-only.
    fn run_polish(&mut self, raw: &str, profile: &PolishProfile) -> (String, bool) {
        match self
            .polish
            .polish(raw, profile, self.config.polish_deadline)
        {
            Ok(PolishOutcome::Polished { text })
                if crate::polish_guard::within_guard(raw, &text) =>
            {
                self.step(SessionInput::PolishReady);
                (text, true)
            }
            Ok(PolishOutcome::Polished { .. }) => {
                tracing::info!(
                    reason = ?PolishRejection::SimilarityGuard,
                    "polish exceeded similarity guard; injecting raw"
                );
                self.step(SessionInput::PolishSkipped);
                (raw.to_owned(), false)
            }
            Ok(PolishOutcome::Rejected { reason }) => {
                tracing::info!(?reason, "polish rejected; injecting raw");
                self.step(SessionInput::PolishSkipped);
                (raw.to_owned(), false)
            }
            Err(err) => {
                tracing::warn!(?err, "polish engine failed; injecting raw");
                self.fault(ErrorId::E10);
                (raw.to_owned(), false)
            }
        }
    }

    /// Inject into the pre-resolved `target` from `Injecting`. Verified delivery
    /// walks to `Idle` and returns `true`; anything else routes to the
    /// honest-failure catalog (E5 secure field, E4 else), leaves the session
    /// `Failed`, and returns `false`. (E7 focus-lost is handled up front in
    /// `finish_recording`, where the target is resolved.)
    fn inject(&mut self, text: &str, target: &FocusedApp) -> bool {
        match self.injector.inject(text, target, InjectionStrategy::Auto) {
            Ok(receipt) if receipt.verified => {
                self.step(SessionInput::Injected);
                true
            }
            Ok(receipt) => {
                tracing::warn!(backend = ?receipt.backend, "injection reported unverified");
                self.fault(ErrorId::E4);
                false
            }
            Err(InjectError::SecureInput) => {
                tracing::info!("secure input field focused; dictation paused");
                self.fault(ErrorId::E5);
                false
            }
            Err(err) => {
                tracing::warn!(?err, "injection failed; clipboard fallback");
                self.fault(ErrorId::E4);
                false
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(profiles: &[(&str, &str)]) -> RunnerConfig {
        RunnerConfig {
            profiles: profiles
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..RunnerConfig::default()
        }
    }

    #[test]
    fn terminal_app_defaults_to_raw() {
        // A known terminal bundle id resolves to raw with no explicit entry.
        assert_eq!(select_profile(&cfg(&[]), "com.apple.Terminal"), None);
    }

    #[test]
    fn unmapped_app_gets_the_default_profile() {
        assert_eq!(
            select_profile(&cfg(&[]), "com.example.editor"),
            Some(DEFAULT_PROFILE.to_owned())
        );
    }

    #[test]
    fn explicit_assignment_wins_over_terminal_default() {
        // A user can opt a terminal back into polishing under a named profile.
        assert_eq!(
            select_profile(
                &cfg(&[("com.apple.Terminal", "email")]),
                "com.apple.Terminal"
            ),
            Some("email".to_owned())
        );
    }

    #[test]
    fn explicit_raw_forces_raw_on_any_app() {
        assert_eq!(
            select_profile(
                &cfg(&[("com.example.editor", RAW_PROFILE)]),
                "com.example.editor"
            ),
            None
        );
    }
}
