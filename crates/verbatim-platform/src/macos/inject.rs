//! `TextInjector` for macOS: a capability-probed backend chain of
//! transient-pasteboard paste -> CGEventPost unicode typing -> clipboard-only,
//! with an honest receipt (ARCHITECTURE.md 4.4, spike 1).
//!
//! Verification is best-effort by construction: neither a synthesized Cmd-V nor
//! posted unicode keystrokes can be positively confirmed to have landed in the
//! target, so a clean post is the strongest signal we have and is reported as
//! `verified`. The clipboard-only last resort stages text for the user to paste
//! and is therefore reported `verified = false` (E4).

use std::thread;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::errors::InjectError;
use crate::macos::clipboard::MacClipboardGuard;
use crate::macos::ffi::{ax_trusted, secure_input_enabled};
use crate::traits::{ClipboardGuard, TextInjector};
use crate::types::{FocusedApp, InjectionBackend, InjectionReceipt, InjectionStrategy};

/// `kVK_ANSI_V`: virtual keycode for the V key, used to synthesize Cmd-V.
const KEYCODE_V: u16 = 0x09;

/// CoreGraphics caps `CGEventKeyboardSetUnicodeString` at 20 UTF-16 units per
/// event; chunk longer text so nothing is silently truncated.
const MAX_UTF16_PER_EVENT: usize = 20;

/// Let the target app process the synthesized Cmd-V before we restore the
/// clipboard. Short enough to be imperceptible, long enough to be reliable.
const PASTE_SETTLE: Duration = Duration::from_millis(40);

/// macOS [`TextInjector`]. Owns the clipboard discipline for its paste backend;
/// holds no ObjC state, so it is `Send + Sync`.
#[derive(Default)]
pub struct MacTextInjector {
    clipboard: MacClipboardGuard,
}

impl MacTextInjector {
    pub fn new() -> Self {
        Self::default()
    }

    fn event_source() -> Result<CGEventSource, InjectError> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|()| {
            InjectError::Backend {
                backend: InjectionBackend::CgEventTyping,
                reason: "CGEventSource creation failed".to_owned(),
            }
        })
    }

    /// Paste via the transient pasteboard: snapshot, stage the dictated text as
    /// transient, synthesize Cmd-V, then restore the user's clipboard unless
    /// they changed it in the meantime.
    fn inject_paste(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let backend = InjectionBackend::TransientPasteboardPaste;
        let snapshot = self
            .clipboard
            .snapshot()
            .map_err(|err| clipboard_failure(backend, err))?;
        self.clipboard
            .set_transient_text(text)
            .map_err(|err| clipboard_failure(backend, err))?;

        self.post_cmd_v()?;
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

    fn post_cmd_v(&self) -> Result<(), InjectError> {
        let source = Self::event_source()?;
        let backend = InjectionBackend::TransientPasteboardPaste;
        let paste_event =
            |keydown: bool| -> Result<(), InjectError> {
                let event = CGEvent::new_keyboard_event(source.clone(), KEYCODE_V, keydown)
                    .map_err(|()| InjectError::Backend {
                        backend,
                        reason: "CGEvent keyboard event creation failed".to_owned(),
                    })?;
                event.set_flags(CGEventFlags::CGEventFlagCommand);
                event.post(CGEventTapLocation::HID);
                Ok(())
            };
        paste_event(true)?;
        paste_event(false)?;
        Ok(())
    }

    /// Type `text` directly as unicode via CGEventPost, chunked to CoreGraphics'
    /// per-event UTF-16 limit.
    fn inject_typing(&self, text: &str) -> Result<InjectionReceipt, InjectError> {
        let backend = InjectionBackend::CgEventTyping;
        let source = Self::event_source()?;

        for chunk in chunk_by_utf16(text, MAX_UTF16_PER_EVENT) {
            for keydown in [true, false] {
                // Keycode is ignored once a unicode string is attached; 0 is fine.
                let event =
                    CGEvent::new_keyboard_event(source.clone(), 0, keydown).map_err(|()| {
                        InjectError::Backend {
                            backend,
                            reason: "CGEvent keyboard event creation failed".to_owned(),
                        }
                    })?;
                event.set_string(chunk);
                event.post(CGEventTapLocation::HID);
            }
        }
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
            InjectionBackend::TransientPasteboardPaste => self.inject_paste(text),
            InjectionBackend::CgEventTyping => self.inject_typing(text),
            InjectionBackend::ClipboardOnly => self.inject_clipboard_only(text),
            other => Err(InjectError::Backend {
                backend: other,
                reason: "backend not supported on macOS".to_owned(),
            }),
        }
    }
}

impl TextInjector for MacTextInjector {
    fn probe(&self) -> Vec<InjectionBackend> {
        // Synthetic events into other apps require Accessibility trust. Without
        // it, only the clipboard-only path is honest.
        if ax_trusted() {
            vec![
                InjectionBackend::TransientPasteboardPaste,
                InjectionBackend::CgEventTyping,
                InjectionBackend::ClipboardOnly,
            ]
        } else {
            vec![InjectionBackend::ClipboardOnly]
        }
    }

    fn inject(
        &self,
        text: &str,
        target: &FocusedApp,
        strategy: InjectionStrategy,
    ) -> Result<InjectionReceipt, InjectError> {
        // Secure input (password field) anywhere: refuse to inject (E5).
        if secure_input_enabled() {
            return Err(InjectError::SecureInput);
        }

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

/// Split `text` into `&str` chunks each at most `max_units` UTF-16 code units,
/// never breaking a `char`.
fn chunk_by_utf16(text: &str, max_units: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut units = 0;
    let mut last = 0;
    for (idx, ch) in text.char_indices() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max_units && idx > start {
            chunks.push(&text[start..idx]);
            start = idx;
            units = 0;
        }
        units += ch_units;
        last = idx + ch.len_utf8();
    }
    if start < last {
        chunks.push(&text[start..last]);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunking_splits_on_utf16_boundary_without_breaking_chars() {
        let text = "abcdefghij"; // 10 ascii units
        assert_eq!(chunk_by_utf16(text, 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn chunking_counts_surrogate_pairs_as_two_units() {
        // Each emoji is 2 UTF-16 units, so only one fits per 3-unit chunk.
        let text = "😀😀😀";
        let chunks = chunk_by_utf16(text, 3);
        assert_eq!(chunks, vec!["😀", "😀", "😀"]);
    }

    #[test]
    fn chunking_handles_empty_and_short_text() {
        assert!(chunk_by_utf16("", 20).is_empty());
        assert_eq!(chunk_by_utf16("hi", 20), vec!["hi"]);
    }
}
