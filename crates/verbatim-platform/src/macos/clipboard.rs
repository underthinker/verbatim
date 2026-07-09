//! `ClipboardGuard` over the general `NSPasteboard`, with the transient/restore
//! discipline that keeps a paste-based injection from stomping the user's
//! clipboard (ARCHITECTURE.md 4.4).
//!
//! The restore decision keys on the `changeCount` produced by our *own* most
//! recent transient write: anything past that means the user (or another app)
//! wrote in between and their content wins. This mirrors `FakeClipboardGuard`
//! so the fake stays a faithful stand-in.

use std::sync::atomic::{AtomicIsize, Ordering};

use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSData, NSString};

use crate::errors::ClipboardError;
use crate::traits::ClipboardGuard;
use crate::types::{ClipboardSnapshot, RestoreOutcome};

/// `org.nspasteboard.TransientType`: the marker clipboard managers honor to
/// skip an entry (http://nspasteboard.org).
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

/// A general-pasteboard-backed [`ClipboardGuard`]. Holds no ObjC object (the
/// pasteboard is fetched per call), only the change counter of our last
/// transient write, so it is trivially `Send + Sync`.
#[derive(Default)]
pub struct MacClipboardGuard {
    /// `changeCount` (NSInteger/`isize`) of our own last transient write.
    transient_change_count: AtomicIsize,
}

impl MacClipboardGuard {
    pub fn new() -> Self {
        Self::default()
    }

    fn general() -> Retained<NSPasteboard> {
        NSPasteboard::generalPasteboard()
    }

    /// Whether the pasteboard still holds our own most recent transient write,
    /// i.e. nobody has written since. The paste backend restores asynchronously,
    /// so a second dictation can arrive while our staged text is still up;
    /// snapshotting then would capture our own text as "the user's clipboard"
    /// and the delayed restore would hand it back to them.
    pub fn holds_our_transient(&self) -> bool {
        let transient = self.transient_change_count.load(Ordering::SeqCst);
        // A zero counter means we have never staged anything.
        if transient == 0 {
            return false;
        }
        Self::general().changeCount() == transient
    }

    /// Write `text` as an ordinary (non-transient) clipboard entry. Used by the
    /// clipboard-only injection fallback, where the user pastes manually (E4).
    pub fn set_persistent_text(&self, text: &str) -> Result<(), ClipboardError> {
        let pb = Self::general();
        let value = NSString::from_str(text);
        // SAFETY: clearing then setting a string for the standard string type.
        unsafe {
            pb.clearContents();
            if !pb.setString_forType(&value, NSPasteboardTypeString) {
                return Err(ClipboardError::Backend(
                    "NSPasteboard rejected setString:forType:".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

impl ClipboardGuard for MacClipboardGuard {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
        let pb = Self::general();
        // SAFETY: reading changeCount and the string value off the pasteboard.
        let (change_count, text) = unsafe {
            let change_count = pb.changeCount();
            let text = pb
                .stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string());
            (change_count, text)
        };
        Ok(ClipboardSnapshot {
            change_count: change_count.max(0) as u64,
            text,
        })
    }

    fn set_transient_text(&self, text: &str) -> Result<(), ClipboardError> {
        let pb = Self::general();
        let value = NSString::from_str(text);
        let transient = NSString::from_str(TRANSIENT_TYPE);
        let empty = NSData::new();
        // SAFETY: clear, set the dictated text, then flag the entry transient so
        // clipboard managers ignore it.
        let change_count = unsafe {
            pb.clearContents();
            if !pb.setString_forType(&value, NSPasteboardTypeString) {
                return Err(ClipboardError::Backend(
                    "NSPasteboard rejected setString:forType:".to_owned(),
                ));
            }
            // Best-effort: an old clipboard manager without the transient type
            // still gets the text; failing to mark it must not fail injection.
            let _ = pb.setData_forType(Some(&empty), &transient);
            pb.changeCount()
        };
        self.transient_change_count
            .store(change_count, Ordering::SeqCst);
        Ok(())
    }

    fn restore_if_unchanged(
        &self,
        snapshot: ClipboardSnapshot,
    ) -> Result<RestoreOutcome, ClipboardError> {
        let pb = Self::general();
        // SAFETY: comparing changeCount, then restoring the prior text.
        unsafe {
            if pb.changeCount() > self.transient_change_count.load(Ordering::SeqCst) {
                // The user copied something after our transient write; theirs wins.
                return Ok(RestoreOutcome::UserModified);
            }
            pb.clearContents();
            if let Some(text) = snapshot.text {
                let value = NSString::from_str(&text);
                if !pb.setString_forType(&value, NSPasteboardTypeString) {
                    return Err(ClipboardError::Backend(
                        "NSPasteboard rejected setString:forType: during restore".to_owned(),
                    ));
                }
            }
        }
        Ok(RestoreOutcome::Restored)
    }
}
