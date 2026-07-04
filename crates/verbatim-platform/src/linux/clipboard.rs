//! `ClipboardGuard` for Wayland via wl-clipboard-rs (data-control protocol).
//!
//! Wayland has no pasteboard change counter, so the snapshot/restore
//! discipline is content-based: we remember the text we staged, and restore
//! the snapshot only while the clipboard still holds exactly that staged
//! text. Any other content means the user (or another app) took over and
//! their content wins (ARCHITECTURE.md 4.4).
//!
//! Transient marking: clipboard managers on KDE honor the
//! `x-kde-passwordManagerHint` MIME as "do not record"; we offer it alongside
//! the text, the closest Wayland equivalent of
//! `org.nspasteboard.TransientType`.

use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use wl_clipboard_rs::copy::{self, MimeSource, Options, Source};
use wl_clipboard_rs::paste::{self, get_contents};

use crate::errors::ClipboardError;
use crate::traits::ClipboardGuard;
use crate::types::{ClipboardSnapshot, RestoreOutcome};

const KDE_PASSWORD_MANAGER_HINT: &str = "x-kde-passwordManagerHint";

/// Wayland [`ClipboardGuard`]. Holds no Wayland connection between calls;
/// wl-clipboard-rs connects per operation.
#[derive(Default)]
pub struct LinuxClipboardGuard {
    /// Emulated change counter, bumped on every write we make.
    change_count: AtomicU64,
    /// The transient text we last staged, so restore can tell "still ours"
    /// from "user changed it".
    staged: Mutex<Option<String>>,
}

impl LinuxClipboardGuard {
    pub fn new() -> Self {
        Self::default()
    }

    fn read_text() -> Result<Option<String>, ClipboardError> {
        match get_contents(
            paste::ClipboardType::Regular,
            paste::Seat::Unspecified,
            paste::MimeType::Text,
        ) {
            Ok((mut pipe, _mime)) => {
                let mut bytes = Vec::new();
                pipe.read_to_end(&mut bytes)
                    .map_err(|err| ClipboardError::Backend(err.to_string()))?;
                Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
            }
            Err(
                paste::Error::NoSeats | paste::Error::ClipboardEmpty | paste::Error::NoMimeType,
            ) => Ok(None),
            Err(err) => Err(ClipboardError::Backend(err.to_string())),
        }
    }

    fn write_text(&self, text: &str, transient: bool) -> Result<(), ClipboardError> {
        let mut sources = vec![MimeSource {
            source: Source::Bytes(text.as_bytes().into()),
            mime_type: copy::MimeType::Text,
        }];
        if transient {
            sources.push(MimeSource {
                source: Source::Bytes(b"secret".as_slice().into()),
                mime_type: copy::MimeType::Specific(KDE_PASSWORD_MANAGER_HINT.to_owned()),
            });
        }
        Options::new()
            .copy_multi(sources)
            .map_err(|err| ClipboardError::Backend(err.to_string()))?;
        self.change_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// Non-transient write for the clipboard-only last resort (E4): the user
    /// pastes manually, so the text must survive in clipboard history.
    pub fn set_persistent_text(&self, text: &str) -> Result<(), ClipboardError> {
        if let Ok(mut staged) = self.staged.lock() {
            *staged = None;
        }
        self.write_text(text, false)
    }
}

impl ClipboardGuard for LinuxClipboardGuard {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
        Ok(ClipboardSnapshot {
            change_count: self.change_count.load(Ordering::SeqCst),
            text: Self::read_text()?,
        })
    }

    fn set_transient_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.write_text(text, true)?;
        let mut staged = self
            .staged
            .lock()
            .map_err(|_| ClipboardError::Backend("staged-text lock poisoned".to_owned()))?;
        *staged = Some(text.to_owned());
        Ok(())
    }

    fn restore_if_unchanged(
        &self,
        snapshot: ClipboardSnapshot,
    ) -> Result<RestoreOutcome, ClipboardError> {
        let staged = {
            let mut guard = self
                .staged
                .lock()
                .map_err(|_| ClipboardError::Backend("staged-text lock poisoned".to_owned()))?;
            guard.take()
        };
        let Some(staged) = staged else {
            // Nothing of ours is on the clipboard; leave whatever is there.
            return Ok(RestoreOutcome::UserModified);
        };
        if Self::read_text()? != Some(staged) {
            return Ok(RestoreOutcome::UserModified);
        }
        match snapshot.text {
            Some(text) => self.write_text(&text, false)?,
            None => copy::clear(copy::ClipboardType::Regular, copy::Seat::All)
                .map_err(|err| ClipboardError::Backend(err.to_string()))?,
        }
        Ok(RestoreOutcome::Restored)
    }
}
