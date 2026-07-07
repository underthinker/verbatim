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
}

export const getConfig = () => invoke<Config>("settings_get");

export const setConfig = (config: Config) =>
  invoke<void>("settings_set", { config });

/** Resolves when the chord is valid; rejects with the reason string otherwise. */
export const validateHotkey = (chord: string) =>
  invoke<void>("settings_validate_hotkey", { chord });
