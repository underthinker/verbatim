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
pub async fn serve(path: &Path, events: Arc<EventBus>) -> std::io::Result<()> {
    // Phase 2: the real microphone replaces only the capture seam behind the
    // `real-audio` feature; the rest of the pipeline stays fake for now.
    #[cfg(feature = "real-audio")]
    let deps = {
        let mut deps = fake_deps();
        deps.audio = Box::new(verbatim_platform::audio::CpalAudioCapture::new());
        deps
    };
    #[cfg(not(feature = "real-audio"))]
    let deps = fake_deps();
    let (runner, handle) = SessionRunner::new(deps, RunnerConfig::default(), events);
    tokio::spawn(runner.run());
    serve_with_handle(path, handle).await
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
