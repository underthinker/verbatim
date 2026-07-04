//! `TextInjector` for Linux/Wayland: a capability-probed backend chain of
//! RemoteDesktop-portal paste -> uinput paste -> clipboard-only, with an
//! honest receipt (ARCHITECTURE.md 4.4, spike 1).
//!
//! Both event-capable backends deliver text by staging it as transient
//! clipboard content and synthesizing Ctrl-V; as on macOS, a cleanly
//! delivered chord is the strongest signal available and is reported as
//! `verified`. Clipboard-only is reported `verified = false` (E4).

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crate::errors::InjectError;
use crate::linux::clipboard::LinuxClipboardGuard;
use crate::linux::portal::{self, PortalSession};
use crate::linux::uinput::UinputKeyboard;
use crate::traits::{ClipboardGuard, TextInjector};
use crate::types::{FocusedApp, InjectionBackend, InjectionReceipt, InjectionStrategy};

/// Let the target app process the synthesized Ctrl-V before restoring the
/// clipboard (same rationale as the macOS `PASTE_SETTLE`).
const PASTE_SETTLE: Duration = Duration::from_millis(80);

/// Linux [`TextInjector`]. The portal session is established lazily on first
/// use (it may pop a consent dialog) and kept resident afterwards.
#[derive(Default)]
pub struct LinuxTextInjector {
    clipboard: LinuxClipboardGuard,
    portal: Mutex<Option<PortalSession>>,
    uinput: UinputKeyboard,
}

impl LinuxTextInjector {
    pub fn new() -> Self {
        Self::default()
    }

    fn paste_via(
        &self,
        backend: InjectionBackend,
        text: &str,
        send_chord: impl FnOnce() -> Result<(), InjectError>,
    ) -> Result<InjectionReceipt, InjectError> {
        let snapshot = self
            .clipboard
            .snapshot()
            .map_err(|err| clipboard_failure(backend, err))?;
        self.clipboard
            .set_transient_text(text)
            .map_err(|err| clipboard_failure(backend, err))?;

        send_chord()?;
        thread::sleep(PASTE_SETTLE);

        // A failed restore must not fail the injection; the text already landed.
        if let Err(err) = self.clipboard.restore_if_unchanged(snapshot) {
            tracing::warn!(?err, "clipboard restore after paste failed");
        }
        Ok(InjectionReceipt {
            backend,
            verified: true,
        })
    }

    fn inject_portal(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let mut guard = self.portal.lock().map_err(|_| InjectError::Backend {
            backend: InjectionBackend::LibeiPortal,
            reason: "portal session lock poisoned".to_owned(),
        })?;
        if guard.is_none() {
            *guard = Some(PortalSession::connect()?);
        }
        let session = guard.as_ref().ok_or(InjectError::Backend {
            backend: InjectionBackend::LibeiPortal,
            reason: "portal session missing after connect".to_owned(),
        })?;
        let receipt = self.paste_via(InjectionBackend::LibeiPortal, text, || {
            session.send_paste_chord()
        });
        if receipt.is_err() {
            // A dead session (revoked consent, portal restart) must not wedge
            // every later attempt; reconnect next time.
            *guard = None;
        }
        receipt
    }

    fn inject_uinput(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        self.paste_via(InjectionBackend::Uinput, text, || {
            self.uinput.send_paste_chord()
        })
    }

    /// Last resort: leave the text on the clipboard for the user to paste (E4).
    fn inject_clipboard_only(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let backend = InjectionBackend::ClipboardOnly;
        self.clipboard
            .set_persistent_text(text)
            .map_err(|err| clipboard_failure(backend, err))?;
        Ok(InjectionReceipt {
            backend,
            verified: false,
        })
    }

    fn try_backend(
        &self,
        backend: InjectionBackend,
        text: &str,
    ) -> Result<InjectionReceipt, InjectError> {
        match backend {
            InjectionBackend::LibeiPortal => self.inject_portal(text),
            InjectionBackend::Uinput => self.inject_uinput(text),
            InjectionBackend::ClipboardOnly => self.inject_clipboard_only(text),
            other => Err(InjectError::Backend {
                backend: other,
                reason: "backend not supported on Linux".to_owned(),
            }),
        }
    }
}

impl TextInjector for LinuxTextInjector {
    fn probe(&self) -> Vec<InjectionBackend> {
        let mut backends = Vec::new();
        if portal::portal_plausible() {
            backends.push(InjectionBackend::LibeiPortal);
        }
        if UinputKeyboard::available() {
            backends.push(InjectionBackend::Uinput);
        }
        backends.push(InjectionBackend::ClipboardOnly);
        backends
    }

    fn inject(
        &self,
        text: &str,
        target: &FocusedApp,
        strategy: InjectionStrategy,
    ) -> Result<InjectionReceipt, InjectError> {
        let backends = match strategy {
            InjectionStrategy::Auto => self.probe(),
            InjectionStrategy::Pinned(backend) => vec![backend],
        };
        if backends.is_empty() {
            return Err(InjectError::NoWritableTarget);
        }

        let mut last_error = None;
        for backend in backends {
            match self.try_backend(backend, text) {
                Ok(receipt) => return Ok(receipt),
                Err(err) => {
                    tracing::warn!(
                        ?backend,
                        app = %target.app_id,
                        ?err,
                        "injection backend failed; trying next"
                    );
                    last_error = Some(err);
                }
            }
        }
        Err(last_error.unwrap_or(InjectError::AllBackendsFailed))
    }
}

fn clipboard_failure(backend: InjectionBackend, err: crate::errors::ClipboardError) -> InjectError {
    InjectError::Backend {
        backend,
        reason: format!("clipboard error: {err}"),
    }
}
