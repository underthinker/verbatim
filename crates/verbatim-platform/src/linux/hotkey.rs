//! `HotkeyManager` via the GlobalShortcuts portal (spike 1).
//!
//! Reliable on KDE Plasma; GNOME needs 48+ for a working implementation, and
//! the documented fallback there is a GNOME custom shortcut invoking
//! `verbatim trigger`. The portal delivers Activated/Deactivated signal
//! pairs, which map directly onto the raw `Pressed`/`Released` edge events
//! core builds hold/toggle semantics on.

use std::sync::Arc;
use std::thread::JoinHandle;

use ashpd::desktop::Session;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use tokio::sync::Notify;

use crate::errors::HotkeyError;
use crate::traits::{HotkeyCallback, HotkeyManager};
use crate::types::{HotkeyBinding, HotkeyEvent};

const SHORTCUT_ID: &str = "push-to-talk";
const SHORTCUT_DESCRIPTION: &str = "Verbatim push-to-talk";

fn backend_error(err: impl ToString) -> HotkeyError {
    HotkeyError::Backend(err.to_string())
}

/// GlobalShortcuts-portal [`HotkeyManager`]. The portal session lives on a
/// dedicated listener thread with its own current-thread runtime; `register`
/// returns once binding succeeded (or failed) there.
#[derive(Default)]
pub struct PortalHotkeyBackend {
    shutdown: Option<Arc<Notify>>,
    listener: Option<JoinHandle<()>>,
}

impl PortalHotkeyBackend {
    pub fn new() -> Self {
        Self::default()
    }

    async fn bind(chord: &str) -> Result<(GlobalShortcuts, Session<GlobalShortcuts>), HotkeyError> {
        let proxy = GlobalShortcuts::new().await.map_err(backend_error)?;
        let session = proxy
            .create_session(Default::default())
            .await
            .map_err(backend_error)?;
        let shortcut = NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESCRIPTION).preferred_trigger(chord);
        proxy
            .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
            .await
            .map_err(backend_error)?
            .response()
            .map_err(backend_error)?;
        Ok((proxy, session))
    }

    async fn listen(
        proxy: GlobalShortcuts,
        session: Session<GlobalShortcuts>,
        on_event: HotkeyCallback,
        shutdown: Arc<Notify>,
    ) {
        let (activated, deactivated) =
            match futures_util::try_join!(proxy.receive_activated(), proxy.receive_deactivated()) {
                Ok(streams) => streams,
                Err(err) => {
                    tracing::warn!(?err, "global-shortcuts signal streams unavailable");
                    return;
                }
            };
        let mut activated = std::pin::pin!(activated);
        let mut deactivated = std::pin::pin!(deactivated);
        loop {
            tokio::select! {
                Some(signal) = activated.next() => {
                    if signal.shortcut_id() == SHORTCUT_ID {
                        on_event(HotkeyEvent::Pressed);
                    }
                }
                Some(signal) = deactivated.next() => {
                    if signal.shortcut_id() == SHORTCUT_ID {
                        on_event(HotkeyEvent::Released);
                    }
                }
                () = shutdown.notified() => break,
                else => break,
            }
        }
        if let Err(err) = session.close().await {
            tracing::debug!(?err, "global-shortcuts session close failed");
        }
    }
}

impl HotkeyManager for PortalHotkeyBackend {
    fn register(
        &mut self,
        binding: &HotkeyBinding,
        on_event: HotkeyCallback,
    ) -> Result<(), HotkeyError> {
        if self.listener.is_some() {
            return Err(HotkeyError::AlreadyRegistered);
        }

        let chord = binding.chord.clone();
        let shutdown = Arc::new(Notify::new());
        let thread_shutdown = Arc::clone(&shutdown);
        // Binding happens on the listener thread (the session must live on
        // its runtime); relay the outcome back so register() stays honest.
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), HotkeyError>>(1);

        let listener = std::thread::Builder::new()
            .name("verbatim-portal-hotkey".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        let _ = ready_tx.send(Err(backend_error(err)));
                        return;
                    }
                };
                runtime.block_on(async move {
                    match Self::bind(&chord).await {
                        Ok((proxy, session)) => {
                            let _ = ready_tx.send(Ok(()));
                            Self::listen(proxy, session, on_event, thread_shutdown).await;
                        }
                        Err(err) => {
                            let _ = ready_tx.send(Err(err));
                        }
                    }
                });
            })
            .map_err(backend_error)?;

        match ready_rx.recv() {
            Ok(Ok(())) => {
                self.shutdown = Some(shutdown);
                self.listener = Some(listener);
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = listener.join();
                Err(err)
            }
            Err(_) => {
                let _ = listener.join();
                Err(backend_error("hotkey listener thread died during bind"))
            }
        }
    }

    fn unregister(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.notify_one();
        }
        if let Some(listener) = self.listener.take()
            && let Err(err) = listener.join()
        {
            tracing::warn!(?err, "hotkey listener thread panicked");
        }
    }
}

impl Drop for PortalHotkeyBackend {
    fn drop(&mut self) {
        self.unregister();
    }
}
