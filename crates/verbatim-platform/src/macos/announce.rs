//! macOS `AccessibilityAnnouncer`: VoiceOver detection plus transient overlay
//! announcements posted through NSAccessibility (UX.md 8).
//!
//! The overlay window is click-through and never focused, so its webview
//! `aria-live` region is invisible to VoiceOver; this OS-level announcement is
//! the only path that reaches it. `announce` must run on the main thread (the
//! NSAccessibility contract); the overlay driver hops there before calling it.
//!
//! CI compiles this on the macOS `real-injection` package job, but the
//! announcement itself can only be confirmed with VoiceOver actually running -
//! the same manual on-device sign-off the injection and permission seams carry.

use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeID, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{MainThreadMarker, NSDictionary, NSString};

use crate::traits::AccessibilityAnnouncer;

// AppKit's NSAccessibility C entry point plus the announcement userInfo key.
// These are `NSString * const` globals and a plain C function; objc2-app-kit
// links AppKit, so the symbols resolve.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSAccessibilityAnnouncementRequestedNotification: *const NSString;
    static NSAccessibilityAnnouncementKey: *const NSString;

    fn NSAccessibilityPostNotificationWithUserInfo(
        element: *mut AnyObject,
        notification: *const NSString,
        user_info: *mut AnyObject,
    );
}

// CFPreferences read for the VoiceOver on/off flag, and the CFBoolean helpers
// to interpret it safely (guard the type before reading the value).
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFPreferencesCopyAppValue(key: CFStringRef, application_id: CFStringRef) -> CFTypeRef;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFBooleanGetTypeID() -> CFTypeID;
}

/// Zero-state announcer; holds no ObjC state, so it is `Send + Sync`.
#[derive(Default)]
pub struct MacAnnouncer;

impl MacAnnouncer {
    pub fn new() -> Self {
        Self
    }
}

impl AccessibilityAnnouncer for MacAnnouncer {
    fn screen_reader_active(&self) -> bool {
        // VoiceOver writes `voiceOverOnOffKey` (a CFBoolean) into the
        // com.apple.universalaccess domain; a true value means it is running.
        let key = CFString::from_static_string("voiceOverOnOffKey");
        let domain = CFString::from_static_string("com.apple.universalaccess");
        // SAFETY: both args are valid CFStringRefs for their lifetimes; the
        // returned value is owned (Copy rule) and released below.
        let value = unsafe {
            CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
        };
        if value.is_null() {
            return false;
        }
        // SAFETY: `value` is non-null; only read it as a boolean once its type
        // is confirmed, then release the owned reference.
        unsafe {
            let is_bool = CFGetTypeID(value) == CFBooleanGetTypeID();
            let on = is_bool && CFBooleanGetValue(value) != 0;
            CFRelease(value);
            on
        }
    }

    fn announce(&self, message: &str) {
        // NSAccessibility posting is main-thread only; the caller dispatches
        // here via `run_on_main_thread`, but bail rather than risk an off-main
        // call if that ever changes.
        let Some(mtm) = MainThreadMarker::new() else {
            tracing::warn!("a11y announce skipped: not on the main thread");
            return;
        };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let text = NSString::from_str(message);

        // SAFETY: the extern statics are non-null AppKit globals for the process
        // lifetime; deref to borrow the key/notification NSStrings.
        let (key, notification) = unsafe {
            (
                &*NSAccessibilityAnnouncementKey,
                &*NSAccessibilityAnnouncementRequestedNotification,
            )
        };

        // userInfo = @{ NSAccessibilityAnnouncementKey: message }. Priority is
        // left at the system default (medium) - a single-entry NSString dict.
        let user_info = NSDictionary::from_slices(&[key], &[&*text]);

        // SAFETY: element is the shared app (a valid accessibility element),
        // notification is the AppKit constant, user_info is a valid dictionary.
        unsafe {
            NSAccessibilityPostNotificationWithUserInfo(
                Retained::as_ptr(&app) as *mut AnyObject,
                notification,
                Retained::as_ptr(&user_info) as *mut AnyObject,
            );
        }
    }
}
