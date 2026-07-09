//! Windows `AccessibilityAnnouncer`: screen-reader detection plus transient
//! overlay announcements raised as UI Automation notification events (UX.md 8).
//!
//! The overlay window is click-through and never activated, so its webview
//! `aria-live` region is not in any screen reader's monitored tree; the
//! OS-level announcement here is the only path that reaches Narrator, NVDA, or
//! JAWS.
//!
//! `SPI_GETSCREENREADER` is the flag every mainstream Windows screen reader
//! sets while it runs, and `UiaRaiseNotificationEvent` is the supported way for
//! a non-UIA-native app to speak a string without moving focus. The app owns no
//! UIA provider tree of its own, so the notification is raised against the host
//! provider Windows synthesizes for the overlay's HWND.
//!
//! CI compiles this on the Windows `real-injection` package job, but the
//! announcement itself can only be confirmed with a screen reader actually
//! running - the same manual on-device sign-off the injection and permission
//! seams carry.

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    NotificationKind_Other, NotificationProcessing_MostRecent, UiaHostProviderFromHwnd,
    UiaRaiseNotificationEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETSCREENREADER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, BSTR};

use crate::traits::AccessibilityAnnouncer;

/// Announcer bound to the overlay window's host UIA provider.
///
/// Holds the window handle as a plain `isize` rather than an `HWND`: the raw
/// pointer newtype is neither `Send` nor `Sync`, and nothing may cross the
/// trait boundary as an OS type anyway. The handle is only ever rehydrated on
/// the main thread, where the overlay driver dispatches `announce`.
pub struct WinAnnouncer {
    hwnd: isize,
}

impl WinAnnouncer {
    /// `hwnd` is the overlay window handle, taken from Tauri at setup. A stale
    /// or zero handle makes `UiaHostProviderFromHwnd` fail, which `announce`
    /// swallows after logging - an announcement never interrupts dictation.
    pub fn new(hwnd: isize) -> Self {
        Self { hwnd }
    }
}

impl AccessibilityAnnouncer for WinAnnouncer {
    fn screen_reader_active(&self) -> bool {
        let mut enabled = BOOL(0);
        // SAFETY: SPI_GETSCREENREADER writes one BOOL into `pvparam`; the
        // out-pointer is a live, correctly sized local. No update flags,
        // because this reads the flag rather than setting it.
        let queried = unsafe {
            SystemParametersInfoW(
                SPI_GETSCREENREADER,
                0,
                Some((&raw mut enabled).cast::<c_void>()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        match queried {
            Ok(()) => enabled.as_bool(),
            Err(err) => {
                tracing::warn!(?err, "SPI_GETSCREENREADER query failed");
                false
            }
        }
    }

    fn announce(&self, message: &str) {
        let hwnd = HWND(self.hwnd as *mut c_void);
        // SAFETY: `hwnd` is the overlay window handle captured at setup; the
        // call validates it and returns an error for a dead or foreign window.
        let provider = match unsafe { UiaHostProviderFromHwnd(hwnd) } {
            Ok(provider) => provider,
            Err(err) => {
                tracing::warn!(?err, "a11y announce skipped: no UIA host provider");
                return;
            }
        };

        let display = BSTR::from(message);
        // An empty activity id: overlay states are one-shot announcements, not
        // steps of a screen-reader-tracked activity.
        let activity = BSTR::new();

        // `MostRecent` drops queued announcements in favour of this one, so a
        // fast ARMING -> LISTENING -> TRANSCRIBING run speaks the live state
        // rather than reading a stale backlog.
        //
        // SAFETY: `provider` is the host provider just obtained, and both BSTRs
        // outlive the call.
        if let Err(err) = unsafe {
            UiaRaiseNotificationEvent(
                &provider,
                NotificationKind_Other,
                NotificationProcessing_MostRecent,
                &display,
                &activity,
            )
        } {
            tracing::warn!(?err, "a11y announce failed");
        }
    }
}
