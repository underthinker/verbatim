//! The daemon: it owns the tokio runtime, the `SessionRunner`, and the Unix
//! domain socket that trigger clients connect to (ARCHITECTURE.md 6).
//!
//! Phase 1 boots the runner on fakes; real audio/ASR/injection swap in behind
//! the same traits in later phases. The socket transport is Unix-only by
//! design for this slice - Windows IPC lands with the Windows backend.

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{SignalKind, signal};

use verbatim_core::event::EventBus;
use verbatim_core::runner::{RunnerConfig, RunnerDeps, RunnerHandle, SessionRunner};
use verbatim_engines::fake::{FakePolishBehavior, FakePolishEngine, FakeTranscriptionEngine};
use verbatim_engines::{
    AudioBuffer, EngineOptions, ModelHandle, PIPELINE_SAMPLE_RATE_HZ, PolishEngine,
    TranscriptionEngine,
};
use verbatim_platform::fake::{FakeAudioCapture, FakeFocusTracker, FakeTextInjector};

use crate::ipc::{Request, Response};

/// Build the fake pipeline the Phase 1 daemon runs on. Every seam is a
/// deterministic fake; real backends replace these one phase at a time.
pub fn fake_deps() -> RunnerDeps {
    let fixture = AudioBuffer {
        samples: vec![0.0; PIPELINE_SAMPLE_RATE_HZ as usize],
        sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
    };

    let mut transcription = FakeTranscriptionEngine::speaking("hello from verbatim");
    // Fakes never fail to load; ignore is honest here, not a swallowed error.
    let _ = transcription.load(&fake_model(), &EngineOptions::default());

    let mut polish = FakePolishEngine::new(FakePolishBehavior::Echo);
    let _ = polish.load(&fake_model(), &EngineOptions::default());

    RunnerDeps {
        audio: Box::new(FakeAudioCapture::new(fixture)),
        transcription: Box::new(transcription),
        polish: Box::new(polish),
        injector: Box::new(FakeTextInjector::default()),
        focus: Box::new(FakeFocusTracker::default()),
    }
}

fn fake_model() -> ModelHandle {
    ModelHandle {
        path: "fake".into(),
    }
}

/// Boot a runner (fakes) and serve the trigger socket at `path` until the
/// process is killed. Returns the shared event bus so an in-process host (the
/// tests, later the Tauri shell) can subscribe.
// Unused in the macOS global-hotkey build, which uses `serve_with_hotkey`, but
// still the entry for every other platform/feature combination and the tests.
#[cfg_attr(all(feature = "global-hotkey", target_os = "macos"), allow(dead_code))]
pub async fn serve(path: &Path, events: Arc<EventBus>) -> std::io::Result<()> {
    let (runner, handle) = SessionRunner::new(build_deps(), RunnerConfig::default(), events);
    tokio::spawn(runner.run());

    // Phase 6: GlobalShortcuts-portal hotkey (spike 1). Unlike the macOS
    // backend this needs no main-thread run loop; the portal listener lives on
    // its own thread and the backend only has to outlive the server. A
    // registration failure degrades to CLI-only triggers (GNOME < 48 lacks a
    // working GlobalShortcuts portal; the documented fallback is a custom
    // shortcut running `verbatim trigger`).
    #[cfg(all(feature = "real-injection", target_os = "linux"))]
    let _hotkey = {
        use std::time::Instant;

        use verbatim_core::hotkey::{HotkeyMode, HotkeySemantics};
        use verbatim_platform::linux::PortalHotkeyBackend;
        use verbatim_platform::{HotkeyBinding, HotkeyManager};

        let chord =
            std::env::var("VERBATIM_HOTKEY").unwrap_or_else(|_| "CTRL+ALT+SPACE".to_owned());
        let mode = match std::env::var("VERBATIM_HOTKEY_MODE").as_deref() {
            Ok("toggle") => HotkeyMode::Toggle,
            // Activated/Deactivated arrive as a pair, so push-to-talk holds work.
            _ => HotkeyMode::Hold,
        };

        let (edge_tx, mut edge_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let handle = handle.clone();
            tokio::spawn(async move {
                let mut semantics = HotkeySemantics::new(mode);
                while let Some(event) = edge_rx.recv().await {
                    if let Some(trigger) = semantics.on_event(event, Instant::now())
                        && handle.trigger(trigger).await.is_err()
                    {
                        break; // runner gone
                    }
                }
            });
        }

        let mut backend = PortalHotkeyBackend::new();
        match backend.register(
            &HotkeyBinding {
                chord: chord.clone(),
            },
            Box::new(move |event| {
                let _ = edge_tx.send(event);
            }),
        ) {
            Ok(()) => tracing::info!(%chord, ?mode, "portal global shortcut registered"),
            Err(err) => tracing::warn!(
                %chord, ?err,
                "portal hotkey registration failed; CLI triggers still work"
            ),
        }
        backend
    };

    serve_with_handle(path, handle).await
}

/// Boot the daemon with a real global hotkey driving dictation (Phase 5).
///
/// The `global-hotkey` crate delivers macOS edges only on the main thread's
/// run loop (see `verbatim_platform::hotkey`), so this owns the run loop on the
/// calling thread - which must be the process main thread - and runs the tokio
/// runtime on background workers. The runner, socket server, and hotkey
/// semantics task all live on tokio; the main thread does nothing but pump the
/// run loop and forward edges until the server shuts down on a signal.
#[cfg(all(feature = "global-hotkey", target_os = "macos"))]
pub fn serve_with_hotkey(path: &Path, events: Arc<EventBus>) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use verbatim_core::hotkey::{HotkeyMode, HotkeySemantics};
    use verbatim_platform::hotkey::{GlobalHotkeyBackend, MainThreadHotkey};
    use verbatim_platform::modifier_tap::{ModifierKey, ModifierTapBackend};
    use verbatim_platform::{HotkeyBinding, HotkeyCallback, HotkeyManager};

    // The trigger is overridable; the default is the right Option key as
    // push-to-talk. A bare right-side modifier is driven by a CGEventTap
    // (`modifier_tap`); any other value is a chord bound via `global-hotkey`.
    let chord = std::env::var("VERBATIM_HOTKEY").unwrap_or_else(|_| "RightOption".to_owned());
    let modifier = ModifierKey::parse(&chord);
    // Modifier keys default to push-to-talk (hold); chords default to toggle.
    let mode = match std::env::var("VERBATIM_HOTKEY_MODE").as_deref() {
        Ok("hold") => HotkeyMode::Hold,
        Ok("toggle") => HotkeyMode::Toggle,
        _ if modifier.is_some() => HotkeyMode::Hold,
        _ => HotkeyMode::Toggle,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _guard = runtime.enter();

    // Log every session transition so a manual hotkey test shows the key
    // driving the state machine live (Idle -> Recording -> ... -> Idle).
    let mut transitions = events.subscribe();
    runtime.spawn(async move {
        use verbatim_core::event::Event;
        while let Ok(event) = transitions.recv().await {
            if let Event::SessionTransition { from, to, .. } = event {
                tracing::info!(?from, ?to, "session transition");
            }
        }
    });

    let (runner, handle) = SessionRunner::new(build_deps(), RunnerConfig::default(), events);
    runtime.spawn(runner.run());

    // Edges cross from the main-thread run loop into tokio here; the semantics
    // task turns raw edges into triggers and drives the runner.
    let (edge_tx, mut edge_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        let handle = handle.clone();
        runtime.spawn(async move {
            let mut semantics = HotkeySemantics::new(mode);
            while let Some(event) = edge_rx.recv().await {
                if let Some(trigger) = semantics.on_event(event, Instant::now())
                    && handle.trigger(trigger).await.is_err()
                {
                    break; // runner gone
                }
            }
        });
    }

    // The socket server owns signal handling and socket cleanup; when it
    // returns, it flips the flag so the main-thread pump loop exits too.
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown = Arc::clone(&shutdown);
        let handle = handle.clone();
        let path = path.to_owned();
        runtime.spawn(async move {
            let _ = serve_with_handle(&path, handle).await;
            shutdown.store(true, Ordering::SeqCst);
        });
    }

    // Build the edge callback fresh per branch (it is consumed on registration).
    let make_callback = || -> HotkeyCallback {
        let edge_tx = edge_tx.clone();
        Box::new(move |event| {
            let _ = edge_tx.send(event);
        })
    };

    // Register on this (main) thread; both backends deliver edges through their
    // run-loop source, which the pump below services. A failure degrades to
    // CLI-only: a bare, unregistered backend just idles the run loop.
    let source: Box<dyn MainThreadHotkey> = match modifier {
        Some(key) => match ModifierTapBackend::new(key, make_callback()) {
            Ok(backend) => {
                tracing::info!(%chord, ?mode, "modifier-key push-to-talk registered");
                Box::new(backend)
            }
            Err(err) => {
                tracing::error!(%chord, ?err, "hotkey registration failed; CLI triggers still work");
                Box::new(GlobalHotkeyBackend::new())
            }
        },
        None => {
            let mut backend = GlobalHotkeyBackend::new();
            match backend.register(
                &HotkeyBinding {
                    chord: chord.clone(),
                },
                make_callback(),
            ) {
                Ok(()) => tracing::info!(%chord, ?mode, "global hotkey registered"),
                Err(err) => tracing::error!(
                    %chord, ?err,
                    "hotkey registration failed; CLI triggers still work"
                ),
            }
            Box::new(backend)
        }
    };

    // Menu-bar presence with a Quit item (ROADMAP M1). Best-effort: if the
    // status item cannot be installed the daemon still runs on the hotkey and
    // CLI, so a tray failure only logs and drops the menu-bar affordance.
    let tray = match verbatim_platform::tray::TrayBackend::new() {
        Ok(tray) => {
            tracing::info!("menu-bar tray installed");
            Some(tray)
        }
        Err(err) => {
            tracing::warn!(?err, "tray unavailable; hotkey and CLI still work");
            None
        }
    };

    while !shutdown.load(Ordering::SeqCst) {
        source.pump(Duration::from_millis(100));
        // The pump above serviced the run loop, so any Quit click is now queued.
        if let Some(tray) = &tray
            && tray.quit_requested()
        {
            tracing::info!("quit requested from tray");
            break;
        }
    }

    // On a tray quit the socket server task never ran its own cleanup, so clear
    // the socket here; a signal-driven shutdown already removed it (ignored).
    let _ = std::fs::remove_file(path);
    Ok(())
}

/// Deps the served daemon runs on: fakes by default, with each real backend
/// swapped in behind its own feature so phases land one seam at a time. Tests
/// call `fake_deps` directly and are unaffected.
fn build_deps() -> RunnerDeps {
    #[allow(unused_mut)]
    let mut deps = fake_deps();
    // Phase 2: real microphone.
    #[cfg(feature = "real-audio")]
    {
        deps.audio = Box::new(verbatim_platform::audio::CpalAudioCapture::new());
    }
    // Phase 3: real whisper.cpp transcription, if a model is configured.
    #[cfg(feature = "real-transcription")]
    {
        if let Some(engine) = real_transcription() {
            deps.transcription = engine;
        }
    }
    // Phase 4: real macOS text injection + focus tracking. The injector owns
    // its own clipboard discipline; both stay behind their probed capabilities.
    #[cfg(all(feature = "real-injection", target_os = "macos"))]
    {
        deps.injector = Box::new(verbatim_platform::macos::MacTextInjector::new());
        deps.focus = Box::new(verbatim_platform::macos::MacFocusTracker::new());
    }
    // Phase 6: real Linux injection (portal -> uinput -> clipboard, spike 1).
    #[cfg(all(feature = "real-injection", target_os = "linux"))]
    {
        deps.injector = Box::new(verbatim_platform::linux::LinuxTextInjector::new());
        deps.focus = Box::new(verbatim_platform::linux::LinuxFocusTracker::new());
    }
    deps
}

/// Build and load the whisper.cpp engine from `VERBATIM_WHISPER_MODEL`. A
/// missing path or a load failure degrades to the fake transcription with a
/// warning so the daemon still boots during development.
#[cfg(feature = "real-transcription")]
fn real_transcription() -> Option<Box<dyn TranscriptionEngine>> {
    let Some(path) = std::env::var_os("VERBATIM_WHISPER_MODEL").map(std::path::PathBuf::from)
    else {
        tracing::warn!("VERBATIM_WHISPER_MODEL not set; keeping fake transcription");
        return None;
    };
    let mut engine = verbatim_engines::WhisperCppEngine::new();
    match engine.load(&ModelHandle { path }, &EngineOptions::default()) {
        Ok(()) => Some(Box::new(engine)),
        Err(err) => {
            tracing::error!(
                ?err,
                "whisper model load failed; keeping fake transcription"
            );
            None
        }
    }
}

/// Serve an already-constructed runner. Split out so tests can subscribe to the
/// bus and hold the handle before any client connects.
pub async fn serve_with_handle(path: &Path, handle: RunnerHandle) -> std::io::Result<()> {
    let listener = bind(path)?;
    let path = path.to_owned();
    tracing::info!(path = %path.display(), "verbatim daemon listening");

    let mut term = signal(SignalKind::terminate())?;
    let mut int = signal(SignalKind::interrupt())?;

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _addr) = result?;
                let handle = handle.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, handle).await {
                        tracing::warn!(error = %err, "connection error");
                    }
                });
            }
            _ = term.recv() => {
                tracing::info!("received SIGTERM, shutting down");
                break;
            }
            _ = int.recv() => {
                tracing::info!("received SIGINT, shutting down");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Bind the listening socket owner-only, clearing any stale socket file first.
fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A leftover socket from a previous run would make bind fail with EADDRINUSE.
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let listener = UnixListener::bind(path)?;
    restrict_to_owner(path)?;
    Ok(listener)
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

async fn handle_connection(stream: UnixStream, handle: RunnerHandle) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }

    let response = match Request::parse(&line) {
        Ok(Request::Trigger(verb)) => match handle.trigger(verb.to_trigger()).await {
            Ok(()) => match handle.status().await {
                Ok(status) => Response::Accepted(state_token(status.state)),
                Err(_) => Response::Error("runner stopped".to_owned()),
            },
            Err(_) => Response::Error("runner stopped".to_owned()),
        },
        Ok(Request::Status) => match handle.status().await {
            Ok(status) => Response::Status(state_token(status.state)),
            Err(_) => Response::Error("runner stopped".to_owned()),
        },
        Err(token) => {
            tracing::warn!(%token, "rejected unrecognized command");
            Response::Error(format!("unrecognized command: {token}"))
        }
    };

    writer.write_all(response.encode().as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

fn state_token(state: verbatim_core::session::SessionState) -> String {
    format!("{state:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use verbatim_core::event::Event;
    use verbatim_core::session::SessionState;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    // Short path: Unix socket paths are capped near 104 bytes on macOS, so we
    // stay in /tmp rather than the (long) sandboxed temp dir.
    fn temp_socket() -> PathBuf {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        PathBuf::from(format!(
            "/tmp/vbtm-test-{}-{unique}.sock",
            std::process::id()
        ))
    }

    async fn connect_retry(path: &Path) -> UnixStream {
        for _ in 0..100 {
            if let Ok(stream) = UnixStream::connect(path).await {
                return stream;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("daemon never came up at {}", path.display());
    }

    /// One request/response round trip over the socket.
    async fn request(path: &Path, line: &str) -> String {
        let stream = connect_retry(path).await;
        let (reader, mut writer) = stream.into_split();
        writer.write_all(line.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
        let mut reader = BufReader::new(reader);
        let mut reply = String::new();
        reader.read_line(&mut reply).await.unwrap();
        reply.trim_end().to_owned()
    }

    fn spawn_served(path: &Path) -> (RunnerHandle, tokio::sync::broadcast::Receiver<Event>) {
        let events = Arc::new(EventBus::default());
        let receiver = events.subscribe();
        let (runner, handle) =
            SessionRunner::new(fake_deps(), RunnerConfig::default(), Arc::clone(&events));
        tokio::spawn(runner.run());
        {
            let handle = handle.clone();
            let path = path.to_path_buf();
            tokio::spawn(async move {
                let _ = serve_with_handle(&path, handle).await;
            });
        }
        (handle, receiver)
    }

    #[tokio::test]
    async fn trigger_round_trip_drives_the_session() {
        let path = temp_socket();
        let (_handle, mut events) = spawn_served(&path);

        assert_eq!(request(&path, "start\n").await, "accepted Recording");
        assert_eq!(request(&path, "status\n").await, "status Recording");
        assert_eq!(request(&path, "stop\n").await, "accepted Idle");

        // The socket really drove the runner through a full dictation cycle.
        let mut reached_idle = false;
        while let Ok(event) = events.try_recv() {
            if let Event::SessionTransition {
                to: SessionState::Idle,
                ..
            } = event
            {
                reached_idle = true;
            }
        }
        assert!(reached_idle, "the loop should have returned to Idle");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn non_verb_payload_is_rejected_and_moves_nothing() {
        let path = temp_socket();
        let (_handle, mut events) = spawn_served(&path);

        let reply = request(&path, "inject: pwn the focused app\n").await;
        assert!(
            reply.starts_with("error"),
            "hostile payload must be rejected, got: {reply}"
        );

        // Nothing was triggered, so no session transition was ever published.
        let moved = std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, Event::SessionTransition { .. }));
        assert!(!moved, "a rejected payload must not move the session");

        let _ = std::fs::remove_file(&path);
    }
}
