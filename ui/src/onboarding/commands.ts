// Typed wrappers over the onboarding Tauri commands
// (crates/verbatim-app/src/gui.rs) and the model DTOs
// (crates/verbatim-app/src/onboarding.rs). No business logic here: every
// decision that matters lives in the Rust onboarding service.
import { invoke } from "@tauri-apps/api/core";

/** Mirrors `verbatim_platform::Capability` (camelCase over the wire). */
export type Capability = "microphone" | "textInjection" | "inputMonitoring";

/** Mirrors `verbatim_platform::PermissionState` (Debug-formatted). */
export type PermissionState = "Granted" | "Denied" | "Undetermined" | "NotNeeded";

/** Mirrors `onboarding::ModelInfo`. */
export interface ModelInfo {
  id: string;
  name: string;
  kind: "transcription" | "polish";
  sizeBytes: number;
}

export const permission = (capability: Capability) =>
  invoke<PermissionState>("onboarding_permission", { capability });

export const requestPermission = (capability: Capability) =>
  invoke<PermissionState>("onboarding_request_permission", { capability });

export const openSettings = (capability: Capability) =>
  invoke<void>("onboarding_open_settings", { capability });

export const recommendedModel = () =>
  invoke<ModelInfo>("onboarding_recommended_model");

export const catalog = () => invoke<ModelInfo[]>("onboarding_catalog");

export const downloadModel = (modelId: string) =>
  invoke<string>("onboarding_download_model", { modelId });

export const complete = (
  transcriptionModel: string | null,
  polishModel: string | null,
) => invoke<void>("onboarding_complete", { transcriptionModel, polishModel });

/** Human-readable size for the model cards (UX.md 6 step 4 "size shown"). */
export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}
