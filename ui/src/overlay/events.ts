// Typed mirror of the overlay driver DTOs
// (crates/verbatim-app/src/overlay.rs). The overlay is driven directly from
// the Rust bus on its own channel; it never invokes commands.

export const OVERLAY_EVENT = "verbatim://overlay";

export type OverlayPhase =
  | "arming"
  | "recording"
  | "finalizing"
  | "processing"
  | "success"
  | "nothingHeard"
  | "error";

export type Surface =
  | "overlay"
  | "modelManagerInline"
  | "guidedFix"
  | "trayNotice";

/** The single primary action offered for an error (UX.md 4). `null` action
 * only for E5 (secure field), which is deliberately action-free. */
export type PrimaryAction =
  | { kind: "openMicPermission" }
  | { kind: "openModelManager" }
  | { kind: "retryTranscription" }
  | { kind: "pasteHint" }
  | { kind: "openInputDevicePicker" }
  | { kind: "resumeDownload" }
  | { kind: "setUpTyping" }
  | { kind: "openPolishSettings" };

/** The full designed response for an error - copy + single primary action
 * (crates/verbatim-app/src/error_catalog.rs). The overlay renders from this;
 * it never maps error IDs to copy or actions itself. */
export interface ErrorPresentation {
  id: string;
  surface: Surface;
  copy: string;
  action: PrimaryAction | null;
}

export type OverlayEvent =
  | {
      kind: "phase";
      phase: OverlayPhase;
      /** Full error catalog response; set only when `phase` is "error". */
      error: ErrorPresentation | null;
    }
  | { kind: "level"; rms: number };
