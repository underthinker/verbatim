//! Hotkey semantics: raw chord edge events become session triggers, once, on
//! top of the platform's `HotkeyEvent` stream (ARCHITECTURE.md 4.5).
//!
//! Two modes, sharing one accidental-press guard:
//!
//! - **Toggle**: each press flips recording on/off; releases are ignored.
//! - **Hold** (push-to-talk with tap-lock): a real hold records only while
//!   held; a quick tap (shorter than [`ACCIDENTAL_PRESS`]) instead *locks*
//!   recording on, and the next press stops it. This is what lets a user both
//!   push-to-talk and tap-to-lock with the same chord (UX.md 2).
//!
//! The machine is pure and time-injected so it is exhaustively unit-testable;
//! the driving actor feeds it `Instant::now()`.

use std::time::{Duration, Instant};

use verbatim_platform::HotkeyEvent;

use crate::runner::Trigger;

/// A press shorter than this is a tap, not a hold. Debounces accidental
/// brushes and distinguishes push-to-talk from tap-to-lock (UX.md 2).
pub const ACCIDENTAL_PRESS: Duration = Duration::from_millis(250);

/// How the chord drives recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyMode {
    /// Press starts, release stops - unless the press was a tap, which locks
    /// recording on until the next press.
    Hold,
    /// Press toggles recording; release does nothing.
    Toggle,
}

/// Internal edge-tracking state for [`HotkeyMode::Hold`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldState {
    /// Not recording, chord up.
    Idle,
    /// Chord down after starting; `since` dates the press for the tap check.
    Holding { since: Instant },
    /// Recording locked on by a tap; waiting for the next press to stop.
    Locked,
}

/// Translates raw hotkey edges into [`Trigger`]s per the configured mode.
#[derive(Debug)]
pub struct HotkeySemantics {
    mode: HotkeyMode,
    hold: HoldState,
}

impl HotkeySemantics {
    pub fn new(mode: HotkeyMode) -> Self {
        Self {
            mode,
            hold: HoldState::Idle,
        }
    }

    /// Feed one raw edge event, stamped with the time it was observed. Returns
    /// the trigger the session should act on, if any.
    pub fn on_event(&mut self, event: HotkeyEvent, now: Instant) -> Option<Trigger> {
        match self.mode {
            HotkeyMode::Toggle => match event {
                HotkeyEvent::Pressed => Some(Trigger::Toggle),
                HotkeyEvent::Released => None,
            },
            HotkeyMode::Hold => self.on_hold_event(event, now),
        }
    }

    fn on_hold_event(&mut self, event: HotkeyEvent, now: Instant) -> Option<Trigger> {
        match (self.hold, event) {
            // Begin a push-to-talk press: start recording, time the press.
            (HoldState::Idle, HotkeyEvent::Pressed) => {
                self.hold = HoldState::Holding { since: now };
                Some(Trigger::Start)
            }
            // Release of a held press. A real hold stops; a tap locks on.
            (HoldState::Holding { since }, HotkeyEvent::Released) => {
                if now.saturating_duration_since(since) >= ACCIDENTAL_PRESS {
                    self.hold = HoldState::Idle;
                    Some(Trigger::Stop)
                } else {
                    self.hold = HoldState::Locked;
                    None
                }
            }
            // Next press while locked stops recording.
            (HoldState::Locked, HotkeyEvent::Pressed) => {
                self.hold = HoldState::Idle;
                Some(Trigger::Stop)
            }
            // The release that follows a lock-stopping press, and any other
            // edge that does not move the machine, is inert.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn toggle_press_toggles_release_ignored() {
        let mut s = HotkeySemantics::new(HotkeyMode::Toggle);
        let now = t0();
        assert_eq!(s.on_event(HotkeyEvent::Pressed, now), Some(Trigger::Toggle));
        assert_eq!(s.on_event(HotkeyEvent::Released, now), None);
        assert_eq!(s.on_event(HotkeyEvent::Pressed, now), Some(Trigger::Toggle));
    }

    #[test]
    fn hold_real_hold_starts_then_stops() {
        let mut s = HotkeySemantics::new(HotkeyMode::Hold);
        let down = t0();
        assert_eq!(s.on_event(HotkeyEvent::Pressed, down), Some(Trigger::Start));
        let up = down + ACCIDENTAL_PRESS + Duration::from_millis(1);
        assert_eq!(s.on_event(HotkeyEvent::Released, up), Some(Trigger::Stop));
    }

    #[test]
    fn hold_quick_tap_locks_then_next_press_stops() {
        let mut s = HotkeySemantics::new(HotkeyMode::Hold);
        let down = t0();
        assert_eq!(s.on_event(HotkeyEvent::Pressed, down), Some(Trigger::Start));
        // Released well within the accidental-press window: a tap-lock.
        let up = down + Duration::from_millis(50);
        assert_eq!(s.on_event(HotkeyEvent::Released, up), None);
        // Recording is now locked on; the next press stops it.
        let stop = up + Duration::from_millis(500);
        assert_eq!(s.on_event(HotkeyEvent::Pressed, stop), Some(Trigger::Stop));
        // The release of that stopping press is inert.
        assert_eq!(s.on_event(HotkeyEvent::Released, stop), None);
    }

    #[test]
    fn hold_exact_threshold_counts_as_hold() {
        let mut s = HotkeySemantics::new(HotkeyMode::Hold);
        let down = t0();
        s.on_event(HotkeyEvent::Pressed, down);
        let up = down + ACCIDENTAL_PRESS;
        assert_eq!(s.on_event(HotkeyEvent::Released, up), Some(Trigger::Stop));
    }

    #[test]
    fn hold_stray_release_while_idle_is_inert() {
        let mut s = HotkeySemantics::new(HotkeyMode::Hold);
        assert_eq!(s.on_event(HotkeyEvent::Released, t0()), None);
    }
}
