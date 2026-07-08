// Typed wrappers over the Settings Tauri commands (crates/verbatim-app/src/gui.rs)
// and the Config DTO (crates/verbatim-app/src/settings.rs). No business logic:
// validation and persistence live in Rust; this only marshals.
import { invoke } from "@tauri-apps/api/core";

/** Mirrors `settings::HotkeyModeConfig` (snake_case over the wire). */
export type HotkeyMode = "hold" | "toggle";

/** Mirrors `settings::Config`. Field names are the serde (snake_case) keys. */
export interface Config {
  version: number;
  hotkey: string;
  hotkey_mode: HotkeyMode;
  transcription_model: string | null;
  polish_model: string | null;
  polish: boolean;
  history_retention_days: number;
  log_level: string;
  dictionary: string[];
}

export const getConfig = () => invoke<Config>("settings_get");

export const setConfig = (config: Config) =>
  invoke<void>("settings_set", { config });

/** Resolves when the chord is valid; rejects with the reason string otherwise. */
export const validateHotkey = (chord: string) =>
  invoke<void>("settings_validate_hotkey", { chord });

/** Mirrors `models::ManagedModel` (camelCase over the wire). */
export interface ManagedModel {
  id: string;
  name: string;
  kind: "transcription" | "polish";
  sizeBytes: number;
  installed: boolean;
  onDiskBytes: number | null;
  isDefault: boolean;
}

export const listModels = () => invoke<ManagedModel[]>("models_list");

export const modelsDiskUsage = () => invoke<number>("models_disk_usage");

export const downloadModel = (modelId: string) =>
  invoke<string>("models_download", { modelId });

export const deleteModel = (modelId: string) =>
  invoke<void>("models_delete", { modelId });

export const setDefaultModel = (modelId: string) =>
  invoke<void>("models_set_default", { modelId });

/** Human-readable byte size for model rows (UX.md 7 disk usage). */
export function formatBytes(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  return `${Math.round(bytes / 1e6)} MB`;
}

/** Mirrors `history::HistoryEntry` (camelCase over the wire). */
export interface HistoryEntry {
  id: number;
  appId: string;
  raw: string;
  polished: string | null;
  createdAt: number;
}

export const listHistory = () => invoke<HistoryEntry[]>("history_list");

export const clearHistory = () => invoke<void>("history_clear");
