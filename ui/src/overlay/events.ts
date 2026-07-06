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

export type OverlayEvent =
  | {
      kind: "phase";
      phase: OverlayPhase;
      /** UX error catalog ID (E1-E10); set only when `phase` is "error". */
      error: string | null;
    }
  | { kind: "level"; rms: number };
