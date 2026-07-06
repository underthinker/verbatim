// Typed mirror of the Rust bridge DTOs (crates/verbatim-app/src/bridge.rs).
// One Tauri event channel carries the whole core bus, 1:1 (ARCHITECTURE.md 4.9).

export const VERBATIM_EVENT = "verbatim://event";

export type SessionStateName =
  | "idle"
  | "arming"
  | "recording"
  | "finalizing"
  | "transcribing"
  | "polishing"
  | "injecting"
  | "failed";

export interface SessionStateDto {
  name: SessionStateName;
  /** UX error catalog ID (E1-E10); set only when `name` is "failed". */
  error: string | null;
}

export type UiEvent =
  | {
      type: "sessionTransition";
      session: number;
      from: SessionStateDto;
      to: SessionStateDto;
    }
  | { type: "inputLevel"; rms: number }
  | {
      type: "downloadProgress";
      modelId: string;
      receivedBytes: number;
      totalBytes: number | null;
    }
  | { type: "permissionChanged"; capability: string; state: string }
  | { type: "errorRaised"; session: number | null; id: string };

export function stateLabel(state: SessionStateDto): string {
  return state.error === null ? state.name : `${state.name} (${state.error})`;
}
