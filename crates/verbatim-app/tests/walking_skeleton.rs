//! Cross-crate walking-skeleton tests: the dictation loop driven end-to-end
//! through the fake platform and engine seams (ENGINEERING.md section 4).
//! The orchestration here is manual; the core session runner will absorb it
//! during M1 wire-up and these tests will shrink to driving that runner.

// Test-only crate; helper functions outside #[test] escape allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use verbatim_core::error::ErrorId;
use verbatim_core::session::{DictationSession, SessionId, SessionInput, SessionState};
use verbatim_engines::fake::FakeTranscriptionEngine;
use verbatim_engines::{
    AudioBuffer, EngineOptions, ModelHandle, PIPELINE_SAMPLE_RATE_HZ, TranscribeOptions,
    TranscriptionEngine,
};
use verbatim_platform::fake::{FakeAudioCapture, FakeFocusTracker, FakeTextInjector};
use verbatim_platform::{AudioCapture, FocusTracker, InjectionStrategy, TextInjector};

fn one_second_fixture() -> AudioBuffer {
    AudioBuffer {
        samples: vec![0.0; PIPELINE_SAMPLE_RATE_HZ as usize],
        sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
    }
}

fn loaded_engine(text: &str) -> FakeTranscriptionEngine {
    let mut engine = FakeTranscriptionEngine::speaking(text);
    engine
        .load(
            &ModelHandle {
                path: "fake".into(),
            },
            &EngineOptions::default(),
        )
        .unwrap();
    engine
}

#[test]
fn hotkey_to_injected_text_happy_path() {
    let mut capture = FakeAudioCapture::new(one_second_fixture());
    let engine = loaded_engine("hello from verbatim");
    let injector = FakeTextInjector::default();
    let focus = FakeFocusTracker::default();

    let mut session = DictationSession::new(SessionId(1));
    session.handle(SessionInput::HotkeyEngaged).unwrap();
    capture.start().unwrap();
    session.handle(SessionInput::StreamOpened).unwrap();

    session.handle(SessionInput::StopRequested).unwrap();
    let audio = capture.stop().unwrap();
    session.handle(SessionInput::TailFlushed).unwrap();

    let transcript = engine
        .transcribe(&audio, &TranscribeOptions::default())
        .unwrap();
    session
        .handle(SessionInput::TranscriptReady { polish: false })
        .unwrap();

    let target = focus.focused_app().unwrap();
    let receipt = injector
        .inject(&transcript.text(), &target, InjectionStrategy::Auto)
        .unwrap();
    assert!(receipt.verified, "receipts must be honest, not hopeful");

    assert_eq!(
        session.handle(SessionInput::Injected).unwrap(),
        SessionState::Idle
    );
    assert_eq!(
        injector.injected_texts(),
        vec!["hello from verbatim".to_owned()]
    );
}

#[test]
fn injection_failure_is_detected_honestly() {
    let mut capture = FakeAudioCapture::new(one_second_fixture());
    let engine = loaded_engine("this will not land");
    let injector = FakeTextInjector::default();
    injector.set_failing(true);
    let focus = FakeFocusTracker::default();

    let mut session = DictationSession::new(SessionId(2));
    session.handle(SessionInput::HotkeyEngaged).unwrap();
    capture.start().unwrap();
    session.handle(SessionInput::StreamOpened).unwrap();
    session.handle(SessionInput::StopRequested).unwrap();
    let audio = capture.stop().unwrap();
    session.handle(SessionInput::TailFlushed).unwrap();
    let transcript = engine
        .transcribe(&audio, &TranscribeOptions::default())
        .unwrap();
    session
        .handle(SessionInput::TranscriptReady { polish: false })
        .unwrap();

    let target = focus.focused_app().unwrap();
    let result = injector.inject(&transcript.text(), &target, InjectionStrategy::Auto);
    assert!(
        result.is_err(),
        "failure must surface, never silent success"
    );

    // The driver maps injection failure to E4 (clipboard fallback overlay).
    assert_eq!(
        session.handle(SessionInput::Fault(ErrorId::E4)).unwrap(),
        SessionState::Failed(ErrorId::E4)
    );
    assert!(injector.injected_texts().is_empty());
}

#[test]
fn cancel_discards_recording() {
    let mut capture = FakeAudioCapture::new(one_second_fixture());

    let mut session = DictationSession::new(SessionId(3));
    session.handle(SessionInput::HotkeyEngaged).unwrap();
    capture.start().unwrap();
    session.handle(SessionInput::StreamOpened).unwrap();

    capture.abort();
    assert!(!capture.is_capturing());
    assert_eq!(
        session.handle(SessionInput::Cancel).unwrap(),
        SessionState::Idle
    );
}
