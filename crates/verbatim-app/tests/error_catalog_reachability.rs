//! Error-catalog reachability harness (M2 Phase C, criterion 2: "every UX error
//! state E1-E10 is reachable in a test harness and shows its designed response;
//! zero dead ends", UX.md 4).
//!
//! The state machine already proves every `Failed(ErrorId)` state is reachable
//! (`verbatim-core` session tests). This harness closes the loop on the *render*
//! side: it walks the whole taxonomy through the real surface path a UI uses -
//! `overlay::directive` for the in-the-moment overlay errors, and the shared
//! `error_catalog::present` mapping - and asserts each error arrives with plain
//! copy and exactly one wired primary action (or, for E5 alone, a deliberate
//! none). No error can reach a user as a bare code or an unhandled dead end.

use verbatim_app::error_catalog::{self, Surface};
use verbatim_app::overlay::{self, Directive, OverlayPhase};
use verbatim_core::error::ErrorId;
use verbatim_core::session::SessionState;

/// Every error in the taxonomy resolves to a designed, non-dead-end response.
#[test]
fn every_error_state_shows_a_designed_response() {
    for id in ErrorId::ALL {
        let p = error_catalog::present(id);

        assert_eq!(p.id, format!("{id:?}"), "{id:?} wire id mismatch");
        assert!(!p.copy.trim().is_empty(), "{id:?} renders empty copy");

        // Zero dead ends: exactly one primary action, except the deliberately
        // action-free secure-field case (E5), which is the only exception.
        match id {
            ErrorId::E5 => assert!(p.action.is_none(), "E5 is action-free by design"),
            _ => assert!(
                p.action.is_some(),
                "{id:?} is a dead end: no primary action"
            ),
        }
    }
}

/// Errors whose surface is the overlay pill actually render through the overlay
/// directive path when the session reaches `Failed(id)` - carrying the full
/// catalog presentation, not a bare ID. Non-overlay errors (download, guided
/// Linux fix, polish tray notice) are routed to their own surfaces instead.
#[test]
fn overlay_errors_render_their_presentation_through_the_directive_path() {
    for id in ErrorId::ALL {
        let presentation = error_catalog::present(id);
        // Non-overlay errors (download inline, guided Linux fix, polish tray
        // notice) are handled off the pill - skip them here.
        if presentation.surface != Surface::Overlay {
            continue;
        }
        // A failure can be entered from any active state; RECORDING is
        // representative (UX.md 2 allows a fault from every active state).
        match overlay::directive(SessionState::Recording, SessionState::Failed(id)) {
            Some(Directive::Show {
                phase: OverlayPhase::Error,
                error: Some(rendered),
            }) => {
                assert_eq!(rendered, presentation, "{id:?} overlay render drifted");
            }
            other => panic!("{id:?} is an overlay error but did not render: {other:?}"),
        }
    }
}
