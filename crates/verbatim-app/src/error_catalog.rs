//! The user-facing error catalog (M2 Phase C): every `ErrorId` (E1-E10) mapped
//! 1:1 to its designed response - the surface it shows on, its plain-language
//! copy, and its single primary action (UX.md 4).
//!
//! Core owns the *taxonomy* (`verbatim_core::error::ErrorId`) but stays free of
//! user-facing copy on purpose; the words and the action a user can take live
//! here in the app/UI layer. This is the one exhaustive mapping: `present` is a
//! total match over `ErrorId` with no default arm, so adding an error to the
//! taxonomy fails to compile until it is given a designed response. That
//! compile-time totality is criterion 2 ("zero dead ends"): no error can reach
//! a user without copy and - unless it is deliberately action-free (E5) - a
//! wired primary action.

use serde::Serialize;

use verbatim_core::error::ErrorId;

/// Where an error's designed response is shown. Not every error goes to the
/// overlay: a download failure belongs inline in the model manager, a missing
/// polish model is a quiet tray notice, and the Linux typing-permission fix is
/// an onboarding-grade pane (UX.md 4 / 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Surface {
    /// The dictation overlay pill (the default for in-the-moment failures).
    Overlay,
    /// Inline within the model manager window (E8 resumable download).
    ModelManagerInline,
    /// An onboarding-grade guided-fix pane (E9 Linux typing setup).
    GuidedFix,
    /// A one-time, non-modal tray notice, not a per-dictation error (E10).
    TrayNotice,
}

/// The single primary action offered for an error - exactly one per UX.md 4,
/// or `None` only where the design deliberately offers none (E5, secure field:
/// the text is already on the clipboard and no user action is wanted).
///
/// Each variant names an *intent*, not an implementation; the surfaces that
/// render it own the actual deep link / re-check loop / retry wiring. Carried
/// to the webview as data so no UI hard-codes the error-to-action map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PrimaryAction {
    /// Deep-link to the OS microphone permission pane, then live re-check (E1).
    OpenMicPermission,
    /// Open the model manager with the recommended model preselected (E2).
    OpenModelManager,
    /// Re-run transcription from the preserved audio; a second failure then
    /// offers an engine/backend switch (E3).
    RetryTranscription,
    /// Hint the manual paste shortcut; the text is already on the clipboard
    /// (E4, and E7 which degrades to E4).
    PasteHint,
    /// Open the input-device picker after a mid-recording disconnect (E6).
    OpenInputDevicePicker,
    /// Resume the interrupted model download with byte progress (E8).
    ResumeDownload,
    /// Open the guided Linux typing-permission setup (E9).
    SetUpTyping,
    /// Deep-link to the polish settings after a silent raw fallback (E10).
    OpenPolishSettings,
}

impl PrimaryAction {
    /// The button/affordance label shown to the user (UX.md 4 primary actions).
    pub fn label(self) -> &'static str {
        match self {
            PrimaryAction::OpenMicPermission => "Open microphone settings",
            PrimaryAction::OpenModelManager => "Download model",
            PrimaryAction::RetryTranscription => "Retry",
            PrimaryAction::PasteHint => "Paste anyway",
            PrimaryAction::OpenInputDevicePicker => "Choose microphone",
            PrimaryAction::ResumeDownload => "Resume download",
            PrimaryAction::SetUpTyping => "Set up typing",
            PrimaryAction::OpenPolishSettings => "Polish settings",
        }
    }
}

/// The complete designed response for one error: where it shows, what it says,
/// and the one thing the user can do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPresentation {
    /// The catalog ID, e.g. `"E4"` - stable across the wire.
    pub id: &'static str,
    pub surface: Surface,
    /// Plain-language copy: no jargon, no blame, no raw error codes (UX.md 4).
    pub copy: &'static str,
    /// The single primary action, or `None` where none is wanted by design.
    pub action: Option<PrimaryAction>,
}

/// Resolve an error to its designed response. Exhaustive by construction - no
/// default arm - so the taxonomy and the catalog cannot drift apart (criterion
/// 2). Copy and actions are the UX.md 4 table verbatim in intent.
pub fn present(id: ErrorId) -> ErrorPresentation {
    match id {
        ErrorId::E1 => ErrorPresentation {
            id: "E1",
            surface: Surface::Overlay,
            copy: "Verbatim can't hear you yet - microphone access is off.",
            action: Some(PrimaryAction::OpenMicPermission),
        },
        ErrorId::E2 => ErrorPresentation {
            id: "E2",
            surface: Surface::Overlay,
            copy: "One-time setup: download a speech model (about 150 MB).",
            action: Some(PrimaryAction::OpenModelManager),
        },
        ErrorId::E3 => ErrorPresentation {
            id: "E3",
            surface: Surface::Overlay,
            copy: "Transcription hit a snag - your recording is saved.",
            action: Some(PrimaryAction::RetryTranscription),
        },
        ErrorId::E4 => ErrorPresentation {
            id: "E4",
            surface: Surface::Overlay,
            copy: "Couldn't type into this app - your text is on the clipboard.",
            action: Some(PrimaryAction::PasteHint),
        },
        ErrorId::E5 => ErrorPresentation {
            id: "E5",
            surface: Surface::Overlay,
            copy: "That looks like a password field, so Verbatim stayed out. \
                   Text is on the clipboard.",
            // Deliberately action-free: the text is already on the clipboard
            // and no follow-up is wanted (UX.md 4, "none needed").
            action: None,
        },
        ErrorId::E6 => ErrorPresentation {
            id: "E6",
            surface: Surface::Overlay,
            copy: "Your mic disconnected - transcribed what I heard.",
            action: Some(PrimaryAction::OpenInputDevicePicker),
        },
        ErrorId::E7 => ErrorPresentation {
            // Focus changed between recording and injection; degrades to the
            // E4 clipboard fallback and shares its response (UX.md 4).
            id: "E7",
            surface: Surface::Overlay,
            copy: "The active app changed - your text is on the clipboard.",
            action: Some(PrimaryAction::PasteHint),
        },
        ErrorId::E8 => ErrorPresentation {
            id: "E8",
            surface: Surface::ModelManagerInline,
            copy: "Download interrupted - pick up where it left off.",
            action: Some(PrimaryAction::ResumeDownload),
        },
        ErrorId::E9 => ErrorPresentation {
            id: "E9",
            surface: Surface::GuidedFix,
            copy: "Verbatim needs permission to type on Linux - a quick one-time setup.",
            action: Some(PrimaryAction::SetUpTyping),
        },
        ErrorId::E10 => ErrorPresentation {
            id: "E10",
            surface: Surface::TrayNotice,
            copy: "Polish is unavailable right now, so Verbatim typed the raw text.",
            action: Some(PrimaryAction::OpenPolishSettings),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reachability harness (criterion 2, "zero dead ends"): every error in
    /// the taxonomy resolves to a designed response with plain copy and, unless
    /// deliberately action-free, exactly one wired primary action. No error can
    /// reach a user as a bare code or an unhandled state.
    #[test]
    fn every_error_has_a_designed_response_with_no_dead_ends() {
        for id in ErrorId::ALL {
            let p = present(id);

            assert!(!p.copy.trim().is_empty(), "{id:?} has empty copy");
            // No jargon / raw codes leak into user copy (UX.md 4).
            assert!(
                !p.copy.contains("0x") && !p.copy.contains("Error "),
                "{id:?} copy leaks a raw error code: {:?}",
                p.copy
            );
            assert_eq!(p.id, format!("{id:?}"), "{id:?} wire id mismatch");

            // Exactly one primary action, except E5 which is action-free by
            // design - and E5 is the *only* permitted exception.
            match id {
                ErrorId::E5 => assert!(p.action.is_none(), "E5 should be action-free"),
                _ => assert!(
                    p.action.is_some(),
                    "{id:?} is a dead end: no primary action"
                ),
            }
        }
    }

    #[test]
    fn presentation_serializes_to_the_ts_contract() {
        // Test-only: DTOs are plain data, serialization cannot fail.
        #[allow(clippy::unwrap_used)]
        let value = serde_json::to_value(present(ErrorId::E4)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "id": "E4",
                "surface": "overlay",
                "copy": "Couldn't type into this app - your text is on the clipboard.",
                "action": { "kind": "pasteHint" },
            })
        );
    }

    #[test]
    fn action_free_error_serializes_null_action() {
        #[allow(clippy::unwrap_used)]
        let value = serde_json::to_value(present(ErrorId::E5)).unwrap();
        assert_eq!(value["action"], serde_json::Value::Null);
    }

    #[test]
    fn non_overlay_surfaces_are_routed_off_the_pill() {
        // A handful of errors are not in-the-moment overlay failures (UX.md 4).
        assert_eq!(present(ErrorId::E8).surface, Surface::ModelManagerInline);
        assert_eq!(present(ErrorId::E9).surface, Surface::GuidedFix);
        assert_eq!(present(ErrorId::E10).surface, Surface::TrayNotice);
    }
}
