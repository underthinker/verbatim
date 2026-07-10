//! Hotkey wiring end-to-end over fakes: a raw chord edge from the platform
//! `HotkeyManager` flows through core's hold/toggle semantics into the
//! `SessionRunner`, exactly as the daemon wires it (Phase 5). The real macOS
//! backend swaps in behind the `global-hotkey` feature without touching this
//! path; its OS-level key delivery is a manual check (synthetic input cannot
//! drive a system global hotkey), so this test covers the wiring contract.

// Test-only crate; helpers outside #[test] escape allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use verbatim_core::event::EventBus;
use verbatim_core::hotkey::{HotkeyMode, HotkeySemantics};
use verbatim_core::runner::{RunnerConfig, RunnerDeps, RunnerHandle, SessionRunner};
use verbatim_core::session::SessionState;
use verbatim_engines::fake::{FakePolishBehavior, FakePolishEngine, FakeTranscriptionEngine};
use verbatim_engines::{EngineOptions, ModelHandle, PolishEngine, TranscriptionEngine};
use verbatim_platform::fake::{
    FakeAudioCapture, FakeFocusTracker, FakeHotkeyManager, FakeTextInjector,
};
use verbatim_platform::{HotkeyBinding, HotkeyManager};

fn fake_model() -> ModelHandle {
    ModelHandle {
        path: "fake".into(),
    }
}

fn spawn_runner() -> RunnerHandle {
    let events = Arc::new(EventBus::default());
    let mut transcription = FakeTranscriptionEngine::speaking("hello from verbatim");
    transcription
        .load(&fake_model(), &EngineOptions::default())
        .unwrap();
    let mut polish = FakePolishEngine::new(FakePolishBehavior::Echo);
    polish
        .load(&fake_model(), &EngineOptions::default())
        .unwrap();

    let deps = RunnerDeps {
        audio: Box::new(FakeAudioCapture::speaking()),
        transcription: Box::new(transcription),
        polish: Box::new(polish),
        injector: Box::new(FakeTextInjector::default()),
        focus: Box::new(FakeFocusTracker::default()),
    };
    let (runner, handle) = SessionRunner::new(deps, RunnerConfig::default(), events);
    tokio::spawn(runner.run());
    handle
}

/// Wire a `HotkeyManager`'s edges through `HotkeySemantics` into the runner,
/// the same shape as `daemon::serve_with_hotkey`. Returns nothing; the manager
/// is moved in and kept alive by the spawned forwarding task's ownership chain.
fn wire_hotkey(
    mut manager: FakeHotkeyManager,
    mode: HotkeyMode,
    handle: RunnerHandle,
) -> Arc<FakeHotkeyManager> {
    let (edge_tx, mut edge_rx) = tokio::sync::mpsc::unbounded_channel();
    manager
        .register(
            &HotkeyBinding {
                chord: "CmdOrCtrl+Shift+Space".to_owned(),
            },
            Box::new(move |event| {
                let _ = edge_tx.send(event);
            }),
        )
        .unwrap();
    tokio::spawn(async move {
        let mut semantics = HotkeySemantics::new(mode);
        while let Some(event) = edge_rx.recv().await {
            if let Some(trigger) = semantics.on_event(event, Instant::now())
                && handle.trigger(trigger).await.is_err()
            {
                break;
            }
        }
    });
    Arc::new(manager)
}

async fn wait_for_state(handle: &RunnerHandle, want: SessionState) {
    for _ in 0..200 {
        if handle.status().await.unwrap().state == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("session never reached {want:?}");
}

#[tokio::test]
async fn toggle_press_starts_then_next_press_stops() {
    let handle = spawn_runner();
    let manager = wire_hotkey(
        FakeHotkeyManager::default(),
        HotkeyMode::Toggle,
        handle.clone(),
    );

    // First chord tap: recording begins.
    manager.press();
    manager.release();
    wait_for_state(&handle, SessionState::Recording).await;

    // Second chord tap: recording stops and the loop runs back to idle.
    manager.press();
    manager.release();
    wait_for_state(&handle, SessionState::Idle).await;
}

#[tokio::test]
async fn hold_push_to_talk_records_while_held() {
    // The default right-Option trigger: hold to record, release to stop. The
    // release must land past the 250 ms accidental-press window to count as a
    // real hold rather than a tap-lock, so we wait it out.
    let handle = spawn_runner();
    let manager = wire_hotkey(
        FakeHotkeyManager::default(),
        HotkeyMode::Hold,
        handle.clone(),
    );

    manager.press();
    wait_for_state(&handle, SessionState::Recording).await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    manager.release();
    wait_for_state(&handle, SessionState::Idle).await;
}
