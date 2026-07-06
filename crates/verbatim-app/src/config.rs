//! Persisted onboarding/setup state (UX.md 6).
//!
//! The app layer owns filesystem paths and persistence; core and engines stay
//! path-free. State is JSON in the per-OS application-data directory
//! (ENGINEERING.md 5.2) and never leaves the machine (security posture).
//! `$VERBATIM_DATA_DIR` overrides the location so tests stay off the real
//! user directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "onboarding.json";
const DATA_DIR_ENV: &str = "VERBATIM_DATA_DIR";

/// What first-run onboarding has recorded. `completed` gates whether the shell
/// launches straight into the main surface or into onboarding; the model ids
/// are the user's choices from steps 4 and 6.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingState {
    pub completed: bool,
    pub transcription_model: Option<String>,
    pub polish_model: Option<String>,
}

impl OnboardingState {
    /// Load from the real data dir, defaulting to "not yet onboarded" on any
    /// missing/corrupt file - a bad state file must never block startup.
    pub fn load() -> Self {
        load_from(&data_dir())
    }

    /// Persist to the real data dir, creating it if needed.
    pub fn save(&self) -> std::io::Result<()> {
        save_to(&data_dir(), self)
    }
}

/// Persistent per-user data directory (ENGINEERING.md 5.2), overridable via
/// `$VERBATIM_DATA_DIR`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    platform_data_dir()
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join("Library/Application Support")
            .join("Verbatim"),
        None => std::env::temp_dir().join("Verbatim"),
    }
}

#[cfg(target_os = "windows")]
fn platform_data_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(base) => PathBuf::from(base).join("Verbatim"),
        None => std::env::temp_dir().join("Verbatim"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_data_dir() -> PathBuf {
    if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(base).join("verbatim");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/verbatim");
    }
    std::env::temp_dir().join("verbatim")
}

fn load_from(dir: &Path) -> OnboardingState {
    let path = dir.join(STATE_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => OnboardingState::default(),
    }
}

fn save_to(dir: &Path, state: &OnboardingState) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(state)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    std::fs::write(dir.join(STATE_FILE), json)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("verbatim-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_state_defaults_to_not_onboarded() {
        let state = load_from(&temp_dir("missing"));
        assert_eq!(state, OnboardingState::default());
        assert!(!state.completed);
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = temp_dir("roundtrip");
        let state = OnboardingState {
            completed: true,
            transcription_model: Some("whisper-small.en".to_owned()),
            polish_model: None,
        };
        save_to(&dir, &state).expect("save");
        assert_eq!(load_from(&dir), state);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_state_falls_back_to_default_not_panic() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(STATE_FILE), b"{ not json").expect("write");
        assert_eq!(load_from(&dir), OnboardingState::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
