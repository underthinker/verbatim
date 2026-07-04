//! Right-side modifier key as a push-to-talk trigger, via a `CGEventTap`.
//!
//! A bare modifier (right Option, right Command, ...) is *not* a chord Carbon's
//! `RegisterEventHotKey` can register, so the `global-hotkey` path cannot bind
//! it. Instead we install a listen-only event tap on `flagsChanged` and watch
//! for the specific right-side key's virtual keycode, emitting a
//! [`HotkeyEvent::Pressed`]/[`HotkeyEvent::Released`] edge on each transition -
//! exactly the stream core's hold semantics expect (push-to-talk).
//!
//! The tap needs the **Input Monitoring** TCC permission (spike 2,
//! `Capability::InputMonitoring`); `global-hotkey` needed none. Its run-loop
//! source is added to the current thread's run loop, so the daemon's existing
//! main-thread pump services it with no extra threading.

use std::time::Duration;

use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};

use crate::errors::HotkeyError;
use crate::hotkey::{MainThreadHotkey, pump_event_loop};
use crate::traits::HotkeyCallback;
use crate::types::HotkeyEvent;

/// A right-side modifier key usable as a push-to-talk trigger. Left-side keys
/// are intentionally excluded so the trigger never clashes with normal
/// modifier use during typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKey {
    RightOption,
    RightCommand,
    RightControl,
    RightShift,
}

impl ModifierKey {
    /// Parse a bare-modifier chord token, if it is one. Returns `None` for
    /// ordinary chords so the caller falls back to the `global-hotkey` path.
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "rightoption" | "rightalt" | "ralt" | "ropt" => Some(Self::RightOption),
            "rightcommand" | "rightcmd" | "rcmd" => Some(Self::RightCommand),
            "rightcontrol" | "rightctrl" | "rctrl" => Some(Self::RightControl),
            "rightshift" | "rshift" => Some(Self::RightShift),
            _ => None,
        }
    }

    /// Virtual keycode (`kVK_*`) of the physical right-side key.
    fn keycode(self) -> i64 {
        match self {
            Self::RightOption => 61,
            Self::RightCommand => 54,
            Self::RightControl => 62,
            Self::RightShift => 60,
        }
    }

    /// Device-independent flag that is set while this key's family is held.
    /// Read to tell a press transition from a release on `flagsChanged`.
    fn flag(self) -> CGEventFlags {
        match self {
            Self::RightOption => CGEventFlags::CGEventFlagAlternate,
            Self::RightCommand => CGEventFlags::CGEventFlagCommand,
            Self::RightControl => CGEventFlags::CGEventFlagControl,
            Self::RightShift => CGEventFlags::CGEventFlagShift,
        }
    }
}

/// A push-to-talk trigger bound to one right-side modifier key. The tap and its
/// run-loop source live for the backend's lifetime; dropping it tears the tap
/// down (the source is invalidated with the mach port).
pub struct ModifierTapBackend {
    // Kept alive so the tap stays installed; never touched after construction.
    _tap: CGEventTap<'static>,
    run_loop: CFRunLoop,
    source: core_foundation::runloop::CFRunLoopSource,
}

impl ModifierTapBackend {
    /// Install a tap for `key`, forwarding edges to `on_event`. Must be called
    /// on the thread whose run loop the daemon pumps (the main thread).
    ///
    /// Fails if Input Monitoring is not granted: the tap cannot be created, so
    /// we prompt for access first, then surface a typed error the caller logs.
    pub fn new(key: ModifierKey, on_event: HotkeyCallback) -> Result<Self, HotkeyError> {
        request_input_monitoring();

        let keycode = key.keycode();
        let flag = key.flag();
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::FlagsChanged],
            move |_proxy, _etype, event| {
                if event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) == keycode {
                    let pressed = event.get_flags().contains(flag);
                    on_event(if pressed {
                        HotkeyEvent::Pressed
                    } else {
                        HotkeyEvent::Released
                    });
                }
                // Listen-only: always pass the event through untouched.
                CallbackResult::Keep
            },
        )
        .map_err(|_| {
            HotkeyError::Backend(
                "could not create event tap; grant Input Monitoring to Verbatim in \
                 System Settings > Privacy & Security"
                    .to_owned(),
            )
        })?;

        let source = tap
            .mach_port()
            .create_runloop_source(0)
            .map_err(|_| HotkeyError::Backend("could not create run-loop source".to_owned()))?;
        let run_loop = CFRunLoop::get_current();
        // SAFETY: `kCFRunLoopCommonModes` is a framework-provided immortal
        // constant; the source outlives the loop registration (we hold it).
        run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();

        Ok(Self {
            _tap: tap,
            run_loop,
            source,
        })
    }
}

impl Drop for ModifierTapBackend {
    fn drop(&mut self) {
        // SAFETY: same immortal constant; removing a registered source is safe.
        self.run_loop
            .remove_source(&self.source, unsafe { kCFRunLoopCommonModes });
    }
}

impl MainThreadHotkey for ModifierTapBackend {
    fn pump(&self, timeout: Duration) {
        // The tap's callback fires from within the run loop; running it is the
        // whole job - no channel to drain on our side.
        pump_event_loop(timeout);
    }
}

/// Prompt for Input Monitoring if not already granted. Non-blocking: it returns
/// the current status and shows the system prompt at most once per app.
fn request_input_monitoring() {
    // SAFETY: both are parameterless CoreGraphics C entry points (macOS 10.15+)
    // that return a bool and have no other effect.
    unsafe {
        if !CGPreflightListenEventAccess() {
            let _ = CGRequestListenEventAccess();
        }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_right_modifier_tokens_case_insensitively() {
        assert_eq!(
            ModifierKey::parse("RightOption"),
            Some(ModifierKey::RightOption)
        );
        assert_eq!(
            ModifierKey::parse("  rightalt "),
            Some(ModifierKey::RightOption)
        );
        assert_eq!(ModifierKey::parse("ralt"), Some(ModifierKey::RightOption));
        assert_eq!(ModifierKey::parse("RCMD"), Some(ModifierKey::RightCommand));
        assert_eq!(ModifierKey::parse("rshift"), Some(ModifierKey::RightShift));
    }

    #[test]
    fn ordinary_chords_are_not_modifier_tokens() {
        assert_eq!(ModifierKey::parse("CmdOrCtrl+Shift+Space"), None);
        assert_eq!(ModifierKey::parse("F19"), None);
        // Left-side modifiers are intentionally unsupported as triggers.
        assert_eq!(ModifierKey::parse("LeftOption"), None);
    }
}
