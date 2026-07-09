//! Cross-crate walking-skeleton tests: the dictation loop driven end-to-end
//! through the core `SessionRunner` over the fake platform and engine seams
//! (ENGINEERING.md 4). These used to orchestrate the loop by hand; now they
//! drive the runner and observe it purely through the event bus, exactly as a
//! real surface would (ARCHITECTURE.md 4.9).

// Test-only crate; helper functions outside #[test] escape allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use tokio::sync::broadcast;

use verbatim_core::error::ErrorId;
use verbatim_core::event::{Event, EventBus};
use verbatim_core::runner::{RunnerConfig, RunnerDeps, RunnerHandle, SessionRunner, Trigger};
use verbatim_core::session::SessionState;
use verbatim_engines::PolishRejection;
use verbatim_engines::fake::{FakePolishBehavior, FakePolishEngine, FakeTranscriptionEngine};
use verbatim_engines::{
    AudioBuffer, EngineOptions, ModelHandle, PIPELINE_SAMPLE_RATE_HZ, PolishEngine,
    TranscriptionEngine,
};
use verbatim_platform::FocusedApp;
use verbatim_platform::fake::{
    FakeAudioCapture, FakeFocusTracker, FakeInjectionOutcome, FakeTextInjector,
};

fn one_second_fixture() -> AudioBuffer {
    AudioBuffer {
        samples: vec![0.0; PIPELINE_SAMPLE_RATE_HZ as usize],
        sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
    }
}

fn fake_model() -> ModelHandle {
    ModelHandle {
        path: "fake".into(),
    }
}

fn loaded_asr(text: &str) -> FakeTranscriptionEngine {
    let mut engine = FakeTranscriptionEngine::speaking(text);
    engine
        .load(&fake_model(), &EngineOptions::default())
        .unwrap();
    engine
}

fn loaded_polish() -> FakePolishEngine {
    let mut engine = FakePolishEngine::new(FakePolishBehavior::Echo);
    engine
        .load(&fake_model(), &EngineOptions::default())
        .unwrap();
    engine
}

/// Build a runner over the fakes and start its task, returning the handle, the
/// shared injector (to read what landed), and a fresh event subscription.
fn spawn_runner(injector: Arc<FakeTextInjector>) -> (RunnerHandle, broadcast::Receiver<Event>) {
    spawn_runner_with(injector, RunnerConfig::default(), loaded_polish())
}

/// Full control over config and polish engine so smoke tests can exercise the
/// polished branch (raw is the default path above).
fn spawn_runner_with(
    injector: Arc<FakeTextInjector>,
    config: RunnerConfig,
    polish: FakePolishEngine,
) -> (RunnerHandle, broadcast::Receiver<Event>) {
    spawn_runner_focused(injector, config, polish, FakeFocusTracker::default())
}

/// As [`spawn_runner_with`], but with a caller-supplied focus tracker so per-app
/// profile selection can be exercised (the frontmost app id drives it).
fn spawn_runner_focused(
    injector: Arc<FakeTextInjector>,
    config: RunnerConfig,
    polish: FakePolishEngine,
    focus: FakeFocusTracker,
) -> (RunnerHandle, broadcast::Receiver<Event>) {
    let events = Arc::new(EventBus::default());
    let receiver = events.subscribe();
    let deps = RunnerDeps {
        audio: Box::new(FakeAudioCapture::new(one_second_fixture())),
        transcription: Box::new(loaded_asr("hello from verbatim")),
        polish: Box::new(polish),
        injector: Box::new(injector),
        focus: Box::new(focus),
    };
    let (runner, handle) = SessionRunner::new(deps, config, events);
    tokio::spawn(runner.run());
    (handle, receiver)
}

fn focused_on(app_id: &str) -> FakeFocusTracker {
    let focus = FakeFocusTracker::default();
    focus.set_focused(FocusedApp {
        app_id: app_id.to_owned(),
        window_title: None,
    });
    focus
}

fn polished_config() -> RunnerConfig {
    RunnerConfig {
        polish: true,
        ..RunnerConfig::default()
    }
}

fn loaded_polish_with(behavior: FakePolishBehavior) -> FakePolishEngine {
    let mut engine = FakePolishEngine::new(behavior);
    engine
        .load(&fake_model(), &EngineOptions::default())
        .unwrap();
    engine
}

fn drain(receiver: &mut broadcast::Receiver<Event>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    events
}

fn transition_targets(events: &[Event]) -> Vec<SessionState> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::SessionTransition { to, .. } => Some(*to),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn hotkey_to_injected_text_happy_path() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, mut events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Recording
    );

    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()],
        "receipts must be honest: the text actually landed"
    );
    let drained = drain(&mut events);
    assert_eq!(
        transition_targets(&drained),
        vec![
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Transcribing,
            SessionState::Injecting,
            SessionState::Idle,
        ]
    );
    // Verified delivery records history: raw text, no polish on this path.
    let recorded = drained
        .iter()
        .find_map(|event| match event {
            Event::DictationRecorded { raw, polished, .. } => Some((raw.clone(), polished.clone())),
            _ => None,
        })
        .expect("a DictationRecorded event must be published on verified injection");
    assert_eq!(recorded, ("hello from verbatim".to_owned(), None));
}

/// The personal-dictionary post-pass re-cases confirmed terms in the injected
/// text deterministically, independent of the LLM - here on the raw path, where
/// no polish ran at all (UX.md 5.3). The recorded `raw` stays the verbatim
/// transcript; only the injected text carries the canonical casing.
#[tokio::test]
async fn dictionary_recases_terms_in_injected_text() {
    let injector = Arc::new(FakeTextInjector::default());
    let config = RunnerConfig {
        dictionary: vec!["Verbatim".to_owned()],
        ..RunnerConfig::default()
    };
    let (handle, mut events) = spawn_runner_with(injector.clone(), config, loaded_polish());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    assert_eq!(
        injector.injected_texts(),
        vec!["hello from Verbatim".to_owned()],
        "the dictionary post-pass must re-case the term in what actually lands"
    );
    let recorded = drain(&mut events)
        .iter()
        .find_map(|event| match event {
            Event::DictationRecorded { raw, .. } => Some(raw.clone()),
            _ => None,
        })
        .expect("a DictationRecorded event must be published");
    assert_eq!(
        recorded, "hello from verbatim",
        "history keeps the verbatim transcript; only injection is re-cased"
    );
}

/// Pause blocks activation triggers but leaves an in-flight session and its
/// stop path intact (UX.md 7 "pause Verbatim").
#[tokio::test]
async fn pause_blocks_start_and_resume_restores_it() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, _events) = spawn_runner(injector.clone());

    handle.set_paused(true).await.unwrap();
    assert!(handle.status().await.unwrap().paused);

    // Paused: Start is a no-op, the session never leaves Idle.
    handle.trigger(Trigger::Start).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    // Resume: the same trigger now records and injects.
    handle.set_paused(false).await.unwrap();
    handle.trigger(Trigger::Start).await.unwrap();
    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Recording
    );
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(injector.injected_texts(), vec!["hello from verbatim"]);
}

/// Total failure: not even the clipboard fallback could stage the text. The
/// failure must surface (E4), never a silent success.
#[tokio::test]
async fn injection_failure_is_detected_honestly() {
    let injector = Arc::new(FakeTextInjector::default());
    injector.set_outcome(FakeInjectionOutcome::HardFailure);
    let (handle, mut events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Failed(ErrorId::E4),
        "failure must surface, never silent success"
    );
    assert!(injector.injected_texts().is_empty());

    let drained = drain(&mut events);
    let raised = drained.iter().any(|event| {
        matches!(
            event,
            Event::ErrorRaised {
                id: ErrorId::E4,
                ..
            }
        )
    });
    assert!(raised, "the E4 error must be published");
    // A failed delivery is not history: no DictationRecorded on this path.
    assert!(
        !drained
            .iter()
            .any(|event| matches!(event, Event::DictationRecorded { .. })),
        "failed injection must not be recorded"
    );
}

/// A blocked primary backend (Windows UIPI against an elevated window, a
/// Wayland portal denial) falls back to the clipboard: the text is staged for
/// the user to paste, not silently lost, and E4 is surfaced honestly.
#[tokio::test]
async fn injection_falls_back_to_clipboard_on_backend_block() {
    let injector = Arc::new(FakeTextInjector::default());
    injector.set_outcome(FakeInjectionOutcome::ClipboardFallback);
    let (handle, mut events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Failed(ErrorId::E4),
        "unverified delivery must surface E4, never a silent success"
    );
    // Nothing was verifiably typed, but the text survived on the clipboard.
    assert!(injector.injected_texts().is_empty());
    assert_eq!(
        injector.clipboard_texts(),
        vec!["hello from verbatim".to_owned()],
        "the fallback must leave the text on the clipboard, not lose it"
    );

    let raised = drain(&mut events).into_iter().any(|event| {
        matches!(
            event,
            Event::ErrorRaised {
                id: ErrorId::E4,
                ..
            }
        )
    });
    assert!(raised, "the E4 clipboard-fallback notice must be published");
}

/// A secure input field (password box) focused at injection time: injection is
/// refused (E5) and the text is staged nowhere - never leaked to the clipboard.
#[tokio::test]
async fn secure_field_refuses_injection_and_reports_e5() {
    let injector = Arc::new(FakeTextInjector::default());
    injector.set_outcome(FakeInjectionOutcome::SecureField);
    let (handle, mut events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Failed(ErrorId::E5),
        "a secure field must refuse injection honestly"
    );
    assert!(injector.injected_texts().is_empty());
    assert!(
        injector.clipboard_texts().is_empty(),
        "a secure context must not leak the text onto the clipboard"
    );

    let raised = drain(&mut events).into_iter().any(|event| {
        matches!(
            event,
            Event::ErrorRaised {
                id: ErrorId::E5,
                ..
            }
        )
    });
    assert!(raised, "the E5 secure-field error must be published");
}

#[tokio::test]
async fn cancel_discards_recording() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, _events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Recording
    );

    handle.trigger(Trigger::Cancel).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert!(injector.injected_texts().is_empty());
}

#[tokio::test]
async fn polished_mode_injects_polished_text() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, mut events) = spawn_runner_with(
        injector.clone(),
        polished_config(),
        loaded_polish_with(FakePolishBehavior::Fixed("polished output".to_owned())),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["polished output".to_owned()],
        "polished text, not raw, must land"
    );
    let drained = drain(&mut events);
    assert_eq!(
        transition_targets(&drained),
        vec![
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Transcribing,
            SessionState::Polishing,
            SessionState::Injecting,
            SessionState::Idle,
        ]
    );
    // History records both raw and the polished text that actually landed.
    let recorded = drained
        .iter()
        .find_map(|event| match event {
            Event::DictationRecorded { raw, polished, .. } => Some((raw.clone(), polished.clone())),
            _ => None,
        })
        .expect("DictationRecorded must be published");
    assert_eq!(recorded.1.as_deref(), Some("polished output"));
    assert!(!recorded.0.is_empty(), "raw text must be recorded too");
}

#[tokio::test]
async fn polish_rejection_degrades_to_raw_never_blocks() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, mut events) = spawn_runner_with(
        injector.clone(),
        polished_config(),
        loaded_polish_with(FakePolishBehavior::Reject(PolishRejection::DeadlineMissed)),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()],
        "rejected polish falls back to raw; the session never dead-ends"
    );
    // Passed through Polishing, then degraded straight to Injecting.
    assert_eq!(
        transition_targets(&drain(&mut events)),
        vec![
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Transcribing,
            SessionState::Polishing,
            SessionState::Injecting,
            SessionState::Idle,
        ]
    );
}

#[tokio::test]
async fn over_edited_polish_trips_similarity_guard_and_degrades_to_raw() {
    let injector = Arc::new(FakeTextInjector::default());
    // The engine returns clean text, but it strayed far from the raw
    // transcript (rewording drift) - the caller-side guard must reject it.
    let (handle, mut events) = spawn_runner_with(
        injector.clone(),
        polished_config(),
        loaded_polish_with(FakePolishBehavior::Fixed(
            "The weather is sunny today and you have three meetings scheduled.".to_owned(),
        )),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()],
        "polish that trips the similarity guard falls back to raw"
    );
    // Records raw-only: guarded-out polish is not history.
    let recorded = drain(&mut events)
        .iter()
        .find_map(|event| match event {
            Event::DictationRecorded { raw, polished, .. } => Some((raw.clone(), polished.clone())),
            _ => None,
        })
        .expect("DictationRecorded must be published");
    assert_eq!(recorded.0, "hello from verbatim");
    assert_eq!(
        recorded.1, None,
        "guarded-out polish records no polished text"
    );
}

/// Per-app profiles (UX.md 5.1): a terminal is frontmost at target time, so
/// polish is skipped and the raw transcript lands even though polish is enabled
/// globally. The default-profile app (`polished_mode_injects_polished_text`
/// above) proves the other side of the branch.
#[tokio::test]
async fn terminal_app_forces_raw_even_with_polish_enabled() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, mut events) = spawn_runner_focused(
        injector.clone(),
        polished_config(),
        loaded_polish_with(FakePolishBehavior::Fixed("polished output".to_owned())),
        focused_on("com.apple.Terminal"),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()],
        "a terminal defaults to raw; polished text must not land there"
    );
    // Never entered Polishing: the profile forced raw before the polish step.
    assert_eq!(
        transition_targets(&drain(&mut events)),
        vec![
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Transcribing,
            SessionState::Injecting,
            SessionState::Idle,
        ]
    );
}

/// An explicit per-app `raw` assignment forces raw on an app that would
/// otherwise be polished (UX.md 5.1).
#[tokio::test]
async fn explicit_raw_profile_forces_raw() {
    let injector = Arc::new(FakeTextInjector::default());
    let mut config = polished_config();
    config
        .profiles
        .insert("com.example.editor".to_owned(), "raw".to_owned());
    let (handle, _events) = spawn_runner_focused(
        injector.clone(),
        config,
        loaded_polish_with(FakePolishBehavior::Fixed("polished output".to_owned())),
        focused_on("com.example.editor"),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()],
        "an app assigned the raw profile injects raw, not polished"
    );
}

/// The raw-mode modifier (UX.md 5.1): `set_raw_override(true)` forces raw for the
/// next dictation regardless of profile or global polish, then is consumed - the
/// dictation after it polishes again.
#[tokio::test]
async fn raw_mode_modifier_forces_raw_for_one_dictation_then_clears() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, _events) = spawn_runner_with(
        injector.clone(),
        polished_config(),
        loaded_polish_with(FakePolishBehavior::Fixed("polished output".to_owned())),
    );

    // Modifier held: this dictation must inject raw.
    handle.set_raw_override(true).await.unwrap();
    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    // Modifier released: the next dictation polishes normally.
    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    assert_eq!(
        injector.injected_texts(),
        vec![
            "hello from verbatim".to_owned(),
            "polished output".to_owned()
        ],
        "raw modifier forces raw once, then clears so polish resumes"
    );
}

/// Live reconfigure (Phase D): a `reconfigure` mid-session flips the polish
/// toggle for the next dictation without a restart.
#[tokio::test]
async fn reconfigure_applies_polish_toggle_to_the_next_dictation() {
    let injector = Arc::new(FakeTextInjector::default());
    // Start with polish off (default): first dictation is raw.
    let (handle, _events) = spawn_runner_with(
        injector.clone(),
        RunnerConfig::default(),
        loaded_polish_with(FakePolishBehavior::Fixed("polished output".to_owned())),
    );

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    // Turn polish on live; the second dictation must now polish.
    handle.reconfigure(polished_config()).await.unwrap();
    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    assert_eq!(
        injector.injected_texts(),
        vec![
            "hello from verbatim".to_owned(),
            "polished output".to_owned()
        ],
        "reconfigure must apply to the next dictation without a restart"
    );
}

#[tokio::test]
async fn toggle_drives_a_full_start_stop_cycle() {
    let injector = Arc::new(FakeTextInjector::default());
    let (handle, _events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Toggle).await.unwrap();
    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Recording
    );

    handle.trigger(Trigger::Toggle).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()]
    );
}

/// A capture that yielded no samples - the hotkey released before the mic
/// stream opened, as happens when the OS holds `audio.start()` on a permission
/// prompt - returns softly to Idle. It must never reach the engine, whose
/// empty-buffer inference error would surface as E3 and offer a "Retry" over
/// no audio (UX.md 2: "no error dialog").
#[tokio::test]
async fn empty_capture_returns_to_idle_without_an_error() {
    let injector = Arc::new(FakeTextInjector::default());
    let events = Arc::new(EventBus::default());
    let mut receiver = events.subscribe();
    let deps = RunnerDeps {
        audio: Box::new(FakeAudioCapture::new(AudioBuffer {
            samples: Vec::new(),
            sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
        })),
        transcription: Box::new(loaded_asr("should never be reached")),
        polish: Box::new(loaded_polish()),
        injector: Box::new(injector.clone()),
        focus: Box::new(FakeFocusTracker::default()),
    };
    let (runner, handle) = SessionRunner::new(deps, RunnerConfig::default(), events);
    tokio::spawn(runner.run());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();
    assert_eq!(handle.status().await.unwrap().state, SessionState::Idle);

    assert!(
        injector.injected_texts().is_empty(),
        "nothing was captured, so nothing may be injected"
    );
    let drained = drain(&mut receiver);
    assert_eq!(
        transition_targets(&drained),
        vec![
            SessionState::Arming,
            SessionState::Recording,
            SessionState::Finalizing,
            SessionState::Idle,
        ],
        "an empty capture must skip the pipeline entirely"
    );
    assert!(
        !drained
            .iter()
            .any(|event| matches!(event, Event::ErrorRaised { .. })),
        "a silent recording is not an error"
    );
}
