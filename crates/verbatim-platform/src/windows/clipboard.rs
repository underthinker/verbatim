//! `ClipboardGuard` over the Win32 clipboard, with the transient/restore
//! discipline that keeps a paste-based injection from stomping the user's
//! clipboard (ARCHITECTURE.md 4.4).
//!
//! The restore decision keys on `GetClipboardSequenceNumber` at our *own* most
//! recent transient write: a different number means the user (or another app)
//! wrote in between and their content wins. This mirrors `MacClipboardGuard`
//! and `FakeClipboardGuard` so the fake stays a faithful stand-in.

use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::w;

use crate::errors::ClipboardError;
use crate::traits::ClipboardGuard;
use crate::types::{ClipboardSnapshot, RestoreOutcome};

/// The clipboard is a contended global resource; another process holding it
/// makes `OpenClipboard` fail transiently, so retry briefly before giving up.
const OPEN_RETRIES: u32 = 10;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

/// A Win32-clipboard-backed [`ClipboardGuard`]. Holds no OS handle (the
/// clipboard is opened per call), only the sequence number of our last
/// transient write, so it is trivially `Send + Sync`.
#[derive(Default)]
pub struct WinClipboardGuard {
    /// `GetClipboardSequenceNumber` right after our own last transient write.
    transient_sequence: AtomicU32,
}

impl WinClipboardGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Write `text` as an ordinary clipboard entry (no monitor-exclusion
    /// marker). Used by the clipboard-only injection fallback, where the user
    /// pastes manually (E4).
    pub fn set_persistent_text(&self, text: &str) -> Result<(), ClipboardError> {
        let _open = OpenGuard::open()?;
        write_text(text)
    }
}

impl ClipboardGuard for WinClipboardGuard {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
        let _open = OpenGuard::open()?;
        let text = read_text();
        Ok(ClipboardSnapshot {
            change_count: u64::from(unsafe { GetClipboardSequenceNumber() }),
            text,
        })
    }

    fn set_transient_text(&self, text: &str) -> Result<(), ClipboardError> {
        {
            let _open = OpenGuard::open()?;
            write_text(text)?;
            // Best-effort: flag the entry so clipboard managers/history skip it
            // (the Win32 equivalent of org.nspasteboard.TransientType). Failing
            // to mark it must not fail the injection.
            let exclude = unsafe {
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
            };
            if exclude != 0
                && let Ok(flag) = global_from_bytes(&0u32.to_ne_bytes())
                && unsafe { SetClipboardData(exclude, Some(HANDLE(flag.0))) }.is_err()
            {
                // The system did not take ownership; free our allocation.
                let _ = unsafe { GlobalFree(Some(flag)) };
            }
        }
        // Read the sequence number outside the open/close so it reflects the
        // committed write.
        self.transient_sequence
            .store(unsafe { GetClipboardSequenceNumber() }, Ordering::SeqCst);
        Ok(())
    }

    fn restore_if_unchanged(
        &self,
        snapshot: ClipboardSnapshot,
    ) -> Result<RestoreOutcome, ClipboardError> {
        if unsafe { GetClipboardSequenceNumber() } != self.transient_sequence.load(Ordering::SeqCst)
        {
            // The user copied something after our transient write; theirs wins.
            return Ok(RestoreOutcome::UserModified);
        }
        let _open = OpenGuard::open()?;
        unsafe { EmptyClipboard() }.map_err(|err| ClipboardError::Backend(err.to_string()))?;
        if let Some(text) = snapshot.text {
            write_text_no_empty(&text)?;
        }
        Ok(RestoreOutcome::Restored)
    }
}

/// RAII open/close around the global clipboard, with retry against transient
/// contention from other processes.
struct OpenGuard;

impl OpenGuard {
    fn open() -> Result<Self, ClipboardError> {
        for attempt in 0..OPEN_RETRIES {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_RETRIES {
                thread::sleep(OPEN_RETRY_DELAY);
            }
        }
        Err(ClipboardError::Unavailable)
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// Read the current `CF_UNICODETEXT` content, if any. Requires the clipboard
/// to be open.
fn read_text() -> Option<String> {
    let format = u32::from(CF_UNICODETEXT.0);
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let hglobal = HGLOBAL(handle.0);
    let ptr = unsafe { GlobalLock(hglobal) }.cast::<u16>();
    if ptr.is_null() {
        return None;
    }
    // SAFETY: CF_UNICODETEXT is a NUL-terminated UTF-16 buffer owned by the
    // clipboard; we only read up to the terminator while holding the lock.
    let text = unsafe {
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    };
    let _ = unsafe { GlobalUnlock(hglobal) };
    Some(text)
}

/// Empty the clipboard and place `text` as `CF_UNICODETEXT`. Requires the
/// clipboard to be open.
fn write_text(text: &str) -> Result<(), ClipboardError> {
    unsafe { EmptyClipboard() }.map_err(|err| ClipboardError::Backend(err.to_string()))?;
    write_text_no_empty(text)
}

fn write_text_no_empty(text: &str) -> Result<(), ClipboardError> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    // SAFETY: the u16 buffer is reinterpreted as bytes for the copy only.
    let bytes = unsafe { std::slice::from_raw_parts(units.as_ptr().cast::<u8>(), units.len() * 2) };
    let hglobal = global_from_bytes(bytes)?;
    if let Err(err) =
        unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(hglobal.0))) }
    {
        // The system did not take ownership; free our allocation.
        let _ = unsafe { GlobalFree(Some(hglobal)) };
        return Err(ClipboardError::Backend(err.to_string()));
    }
    Ok(())
}

/// Allocate a movable global buffer holding `bytes`, as `SetClipboardData`
/// requires. Ownership passes to the system on a successful set.
fn global_from_bytes(bytes: &[u8]) -> Result<HGLOBAL, ClipboardError> {
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
        .map_err(|err| ClipboardError::Backend(err.to_string()))?;
    let ptr = unsafe { GlobalLock(hglobal) }.cast::<u8>();
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(Some(hglobal)) };
        return Err(ClipboardError::Backend("GlobalLock failed".to_owned()));
    }
    // SAFETY: the allocation is at least bytes.len() and locked for the copy.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    }
    let _ = unsafe { GlobalUnlock(hglobal) };
    Ok(hglobal)
}
