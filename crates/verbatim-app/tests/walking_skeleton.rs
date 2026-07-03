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
use verbatim_engines::fake::{FakePolishBehavior, FakePolishEngine, FakeTranscriptionEngine};
use verbatim_engines::{
    AudioBuffer, EngineOptions, ModelHandle, PIPELINE_SAMPLE_RATE_HZ, PolishEngine,
    TranscriptionEngine,
};
use verbatim_platform::fake::{FakeAudioCapture, FakeFocusTracker, FakeTextInjector};

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
    let events = Arc::new(EventBus::default());
    let receiver = events.subscribe();
    let deps = RunnerDeps {
        audio: Box::new(FakeAudioCapture::new(one_second_fixture())),
        transcription: Box::new(loaded_asr("hello from verbatim")),
        polish: Box::new(loaded_polish()),
        injector: Box::new(injector),
        focus: Box::new(FakeFocusTracker::default()),
    };
    let (runner, handle) = SessionRunner::new(deps, RunnerConfig::default(), events);
    tokio::spawn(runner.run());
    (handle, receiver)
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

#[tokio::test]
async fn injection_failure_is_detected_honestly() {
    let injector = Arc::new(FakeTextInjector::default());
    injector.set_failing(true);
    let (handle, mut events) = spawn_runner(injector.clone());

    handle.trigger(Trigger::Start).await.unwrap();
    handle.trigger(Trigger::Stop).await.unwrap();

    assert_eq!(
        handle.status().await.unwrap().state,
        SessionState::Failed(ErrorId::E4),
        "failure must surface, never silent success"
    );
    assert!(injector.injected_texts().is_empty());

    let raised = drain(&mut events).into_iter().any(|event| {
        matches!(
            event,
            Event::ErrorRaised {
                id: ErrorId::E4,
                ..
            }
        )
    });
    assert!(raised, "the E4 clipboard-fallback error must be published");
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
