//! `TextInjector` for Windows: a capability-probed backend chain of
//! `SendInput` unicode typing -> clipboard + Ctrl-V paste -> clipboard-only,
//! with an honest receipt (ARCHITECTURE.md 4.4, windows doc stub).
//!
//! Verification is best-effort by construction: `SendInput` returns how many
//! events the system inserted into the input stream, so a full insert is the
//! strongest signal we have and is reported as `verified`. A short insert
//! (typically UIPI refusing input into an elevated window with
//! `ERROR_ACCESS_DENIED`) is an honest backend failure that falls through the
//! chain. The clipboard-only last resort stages text for the user to paste
//! and is therefore reported `verified = false` (E4).

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL,
};

use crate::errors::InjectError;
use crate::traits::{ClipboardGuard, TextInjector};
use crate::types::{FocusedApp, InjectionBackend, InjectionReceipt, InjectionStrategy};
use crate::windows::clipboard::WinClipboardGuard;

/// Virtual keycode for the V key, used to synthesize Ctrl-V.
const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

/// Let the target app process the synthesized Ctrl-V before we restore the
/// clipboard. Short enough to be imperceptible, long enough to be reliable.
const PASTE_SETTLE: Duration = Duration::from_millis(40);

/// Windows [`TextInjector`]. Owns the clipboard discipline for its paste
/// backend; holds no OS state, so it is `Send + Sync`.
#[derive(Default)]
pub struct WinTextInjector {
    clipboard: WinClipboardGuard,
}

impl WinTextInjector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Type `text` directly as `KEYEVENTF_UNICODE` down/up pairs, one UTF-16
    /// code unit per pair (surrogate halves are delivered as separate events,
    /// which is the documented contract for astral characters).
    fn inject_typing(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let backend = InjectionBackend::SendInputUnicode;
        let mut inputs = Vec::new();
        for unit in text.encode_utf16() {
            for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                inputs.push(keyboard_input(VIRTUAL_KEY(0), unit, flags));
            }
        }
        if inputs.is_empty() {
            return Ok(InjectionReceipt {
                backend,
                verified: true,
            });
        }
        send_all(backend, &inputs)?;
        Ok(InjectionReceipt {
            backend,
            verified: true,
        })
    }

    /// Paste via the clipboard: snapshot, stage the dictated text flagged for
    /// clipboard managers to skip, synthesize Ctrl-V, then restore the user's
    /// clipboard unless they changed it in the meantime.
    fn inject_paste(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let backend = InjectionBackend::ClipboardAssistedPaste;
        let snapshot = self
            .clipboard
            .snapshot()
            .map_err(|err| clipboard_failure(backend, err))?;
        self.clipboard
            .set_transient_text(text)
            .map_err(|err| clipboard_failure(backend, err))?;

        let chord = [
            keyboard_input(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
            keyboard_input(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
            keyboard_input(VK_V, 0, KEYEVENTF_KEYUP),
            keyboard_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
        ];
        let sent = send_all(backend, &chord);
        thread::sleep(PASTE_SETTLE);

        // A failed restore must not fail the injection; the text (if the chord
        // landed) is already delivered.
        if let Err(err) = self.clipboard.restore_if_unchanged(snapshot) {
            tracing::warn!(?err, "clipboard restore after paste failed");
        }
        sent?;
        Ok(InjectionReceipt {
            backend,
            verified: true,
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
            InjectionBackend::SendInputUnicode => self.inject_typing(text),
            InjectionBackend::ClipboardAssistedPaste => self.inject_paste(text),
            InjectionBackend::ClipboardOnly => self.inject_clipboard_only(text),
            other => Err(InjectError::Backend {
                backend: other,
                reason: "backend not supported on Windows".to_owned(),
            }),
        }
    }
}

impl TextInjector for WinTextInjector {
    fn probe(&self) -> Vec<InjectionBackend> {
        // SendInput needs no permission grant on Windows; UIPI denial is
        // per-target (elevated window) and only observable at injection time,
        // so the full chain is always offered and failures fall through.
        vec![
            InjectionBackend::SendInputUnicode,
            InjectionBackend::ClipboardAssistedPaste,
            InjectionBackend::ClipboardOnly,
        ]
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

fn keyboard_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Send `inputs` and demand a full insert: a short count means the system
/// dropped part of the stream (UIPI against an elevated window reports
/// `ERROR_ACCESS_DENIED`), which must surface as failure, never as silent
/// partial delivery (spike 1 trap).
fn send_all(backend: InjectionBackend, inputs: &[INPUT]) -> Result<(), InjectError> {
    // SAFETY: plain FFI over a fully initialized INPUT slice.
    let inserted = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if inserted as usize == inputs.len() {
        return Ok(());
    }
    let last_error = unsafe { GetLastError() };
    Err(InjectError::Backend {
        backend,
        reason: format!(
            "SendInput inserted {inserted}/{} events (last error {}; elevated targets are blocked by UIPI)",
            inputs.len(),
            last_error.0
        ),
    })
}
