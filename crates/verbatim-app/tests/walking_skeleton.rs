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
    let events = Arc::new(EventBus::default());
    let receiver = events.subscribe();
    let deps = RunnerDeps {
        audio: Box::new(FakeAudioCapture::new(one_second_fixture())),
        transcription: Box::new(loaded_asr("hello from verbatim")),
        polish: Box::new(polish),
        injector: Box::new(injector),
        focus: Box::new(FakeFocusTracker::default()),
    };
    let (runner, handle) = SessionRunner::new(deps, config, events);
    tokio::spawn(runner.run());
    (handle, receiver)
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
