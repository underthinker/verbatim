//! User configuration (ARCHITECTURE.md 4.8): one versioned `config.toml` in the
//! platform *config* dir, all defaults in [`Config::default`].
//!
//! The config dir differs from the data dir on Windows/Linux (ENGINEERING.md
//! 5.2): config is `%APPDATA%` / `$XDG_CONFIG_HOME`, while models/history/logs
//! live under `%LOCALAPPDATA%` / `$XDG_DATA_HOME`. On macOS both are
//! `~/Library/Application Support/Verbatim`.
//!
//! `#[serde(default)]` on every field is the migration story: an older file
//! missing a newer key loads with that key's default rather than failing, and a
//! corrupt file falls back to `Config::default()` so a bad config never blocks
//! startup. `$VERBATIM_CONFIG_DIR` overrides the location for tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use verbatim_core::hotkey::HotkeyMode;
use verbatim_core::runner::RunnerConfig;

const CONFIG_FILE: &str = "config.toml";
const CONFIG_DIR_ENV: &str = "VERBATIM_CONFIG_DIR";

/// The current on-disk schema version. Bump when a field's meaning changes in a
/// way `#[serde(default)]` cannot absorb; add a migration keyed on this.
pub const SCHEMA_VERSION: u32 = 1;

/// How the chord drives recording, mirrored from [`HotkeyMode`] because core
/// stays serde-free (see `bridge.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyModeConfig {
    #[default]
    Hold,
    Toggle,
}

impl HotkeyModeConfig {
    pub fn to_core(self) -> HotkeyMode {
        match self {
            HotkeyModeConfig::Hold => HotkeyMode::Hold,
            HotkeyModeConfig::Toggle => HotkeyMode::Toggle,
        }
    }
}

/// The full user config. One annotated `Default` is the single source of every
/// default (ARCHITECTURE.md 4.8).
///
/// The personal dictionary and per-app profiles are both present below and wired
/// live into the runner via [`Config::to_runner_config`]. Dictionary auto-learn +
/// its one-click confirm flow wait on an auto-learn source (still an open
/// question), so for now every term is user-added and applies as soon as saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// On-disk schema version, for migration on upgrade.
    pub version: u32,
    /// Global hotkey chord in cross-platform textual form (`Ctrl+Alt+Space`).
    pub hotkey: String,
    /// Hold-to-talk vs press-to-toggle.
    pub hotkey_mode: HotkeyModeConfig,
    /// Chosen transcription model id (catalog key); `None` until onboarding or
    /// the model manager sets one.
    pub transcription_model: Option<String>,
    /// Chosen polish model id; `None` disables polish regardless of `polish`.
    pub polish_model: Option<String>,
    /// Inject polished text (`true`) or raw transcript (`false`).
    pub polish: bool,
    /// History retention in days; `0` means "off" - write nothing (UX.md 7).
    pub history_retention_days: u32,
    /// File-log verbosity (`tracing` directive: error/warn/info/debug/trace).
    pub log_level: String,
    /// Personal-dictionary terms in canonical casing (UX.md 5.3). Fed to the
    /// polish prompt and re-applied as a deterministic post-pass over the injected
    /// text, so a term like `PCM` never depends on the LLM alone.
    pub dictionary: Vec<String>,
    /// Per-app polish profile assignments: frontmost app id -> profile id
    /// (UX.md 5.1). The reserved id `raw` forces raw injection for that app;
    /// terminals default to raw even without an entry. Empty by default.
    pub profiles: BTreeMap<String, String>,
    /// Polish deadline in milliseconds, measured per machine by the onboarding
    /// calibration micro-benchmark (M3 Phase E). `None` uses the built-in default
    /// until the machine is calibrated.
    pub polish_deadline_ms: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            // Free on all three OSes (UX.md 3); onboarding conflict-checks it.
            hotkey: "Ctrl+Alt+Space".to_owned(),
            hotkey_mode: HotkeyModeConfig::Hold,
            transcription_model: None,
            polish_model: None,
            polish: true,
            history_retention_days: 7,
            log_level: "info".to_owned(),
            dictionary: Vec::new(),
            profiles: BTreeMap::new(),
            polish_deadline_ms: None,
        }
    }
}

impl Config {
    /// Load from the config dir, falling back to defaults on missing/corrupt
    /// file (a bad config must never block startup).
    pub fn load() -> Self {
        load_from(&config_dir())
    }

    /// Project the persisted config onto the runner's runtime knobs
    /// (ARCHITECTURE.md 4.8). The daemon/GUI build this at startup and re-send it
    /// on every save (`reconfigure`) so polish, dictionary, and per-app profiles
    /// apply without a restart. Hotkey/model live re-apply is out of scope here -
    /// the runner only owns the pipeline knobs, not OS hotkey registration.
    pub fn to_runner_config(&self) -> RunnerConfig {
        RunnerConfig {
            polish: self.polish,
            dictionary: self.dictionary.clone(),
            profiles: self.profiles.clone(),
            polish_deadline: self
                .polish_deadline_ms
                .map(|ms| std::time::Duration::from_millis(u64::from(ms)))
                .unwrap_or(RunnerConfig::default().polish_deadline),
        }
    }

    /// Persist to the config dir, creating it if needed.
    pub fn save(&self) -> std::io::Result<()> {
        save_to(&config_dir(), self)
    }

    /// Reject an invalid hotkey chord before persisting a rebind (UX.md 3).
    /// This is syntax + reserved-list validation only.
    ///
    /// ponytail: real OS-registration conflict detection needs the platform
    /// hotkey manager (feature `global-hotkey`); wire it in when rebind must
    /// reflect chords already claimed by other apps. Until then a syntactically
    /// valid, non-reserved chord is accepted.
    pub fn validate_hotkey(chord: &str) -> Result<(), HotkeyError> {
        parse_chord(chord)
    }
}

/// Why a proposed hotkey chord was rejected (UX.md 3 conflict check).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("hotkey cannot be empty")]
    Empty,
    #[error("hotkey needs at least one modifier (Ctrl/Alt/Shift/Cmd) plus a key")]
    NoModifier,
    #[error("hotkey needs a non-modifier key")]
    NoKey,
    #[error("unknown key token: {0}")]
    UnknownToken(String),
    #[error("{0} is reserved by the OS and cannot be bound")]
    Reserved(String),
}

const MODIFIERS: &[&str] = &[
    "ctrl", "control", "alt", "option", "shift", "cmd", "super", "meta",
];
// Bare right-side modifiers the macOS CGEventTap backend drives as
// push-to-talk triggers. Mirrors `verbatim_platform::modifier_tap::
// ModifierKey::parse` (not importable here: that module is macOS- and
// feature-gated, while validation must compile everywhere).
#[cfg(target_os = "macos")]
const MODIFIER_TAP_TOKENS: &[&str] = &[
    "rightoption",
    "rightalt",
    "ralt",
    "ropt",
    "rightcommand",
    "rightcmd",
    "rcmd",
    "rightcontrol",
    "rightctrl",
    "rctrl",
    "rightshift",
    "rshift",
];
// A conservative reserved set: chords the OS almost always claims. Not
// exhaustive - the real per-OS probe supersedes this (see validate_hotkey).
const RESERVED: &[&str] = &["ctrl+alt+delete", "cmd+q", "cmd+tab", "alt+f4"];

/// Parse and validate a chord: at least one modifier and exactly one
/// non-modifier key, every token recognized, not in the reserved set.
fn parse_chord(chord: &str) -> Result<(), HotkeyError> {
    let trimmed = chord.trim();
    if trimmed.is_empty() {
        return Err(HotkeyError::Empty);
    }
    let normalized = trimmed.to_lowercase();
    if RESERVED.contains(&normalized.as_str()) {
        return Err(HotkeyError::Reserved(trimmed.to_owned()));
    }
    // A bare right-side modifier is a valid macOS binding (push-to-talk via
    // the modifier tap), not a chord.
    #[cfg(target_os = "macos")]
    if MODIFIER_TAP_TOKENS.contains(&normalized.as_str()) {
        return Ok(());
    }

    let mut modifiers = 0;
    let mut keys = 0;
    for token in normalized.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(HotkeyError::UnknownToken(chord.to_owned()));
        }
        if MODIFIERS.contains(&token) {
            modifiers += 1;
        } else if is_key_token(token) {
            keys += 1;
        } else {
            return Err(HotkeyError::UnknownToken(token.to_owned()));
        }
    }
    if modifiers == 0 {
        return Err(HotkeyError::NoModifier);
    }
    if keys == 0 {
        return Err(HotkeyError::NoKey);
    }
    Ok(())
}

/// A recognizable non-modifier key: a single alphanumeric, or a named key.
fn is_key_token(token: &str) -> bool {
    const NAMED: &[&str] = &[
        "space",
        "tab",
        "enter",
        "return",
        "esc",
        "escape",
        "backspace",
        "delete",
        "home",
        "end",
        "pageup",
        "pagedown",
        "up",
        "down",
        "left",
        "right",
        "globe",
    ];
    if NAMED.contains(&token) {
        return true;
    }
    // Function keys F1-F24.
    if let Some(num) = token.strip_prefix('f')
        && let Ok(n) = num.parse::<u8>()
    {
        return (1..=24).contains(&n);
    }
    // Single letter or digit.
    token.chars().count() == 1 && token.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Persistent per-user *config* directory (ENGINEERING.md 5.2), overridable via
/// `$VERBATIM_CONFIG_DIR`.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        return PathBuf::from(dir);
    }
    platform_config_dir()
}

#[cfg(target_os = "macos")]
fn platform_config_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join("Library/Application Support")
            .join("Verbatim"),
        None => std::env::temp_dir().join("Verbatim"),
    }
}

#[cfg(target_os = "windows")]
fn platform_config_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(base) => PathBuf::from(base).join("Verbatim"),
        None => std::env::temp_dir().join("Verbatim"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_config_dir() -> PathBuf {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("verbatim");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/verbatim");
    }
    std::env::temp_dir().join("verbatim")
}

pub(crate) fn load_from(dir: &Path) -> Config {
    let path = dir.join(CONFIG_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub(crate) fn save_to(dir: &Path, config: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = toml::to_string_pretty(config)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    std::fs::write(dir.join(CONFIG_FILE), text)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("verbatim-conf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_config_defaults() {
        assert_eq!(load_from(&temp_dir("missing")), Config::default());
    }

    #[test]
    fn config_round_trips_through_toml() {
        let dir = temp_dir("roundtrip");
        let config = Config {
            hotkey_mode: HotkeyModeConfig::Toggle,
            polish: false,
            history_retention_days: 0,
            transcription_model: Some("whisper-small.en".to_owned()),
            dictionary: vec!["PCM".to_owned(), "gRPC".to_owned()],
            profiles: BTreeMap::from([
                ("com.apple.Terminal".to_owned(), "raw".to_owned()),
                ("com.tinyspeck.slackmacgap".to_owned(), "email".to_owned()),
            ]),
            ..Config::default()
        };
        save_to(&dir, &config).expect("save");
        assert_eq!(load_from(&dir), config);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn to_runner_config_projects_polish_dictionary_and_profiles() {
        let config = Config {
            polish: true,
            dictionary: vec!["PCM".to_owned()],
            profiles: BTreeMap::from([("com.apple.Terminal".to_owned(), "raw".to_owned())]),
            polish_deadline_ms: Some(640),
            ..Config::default()
        };
        let runner = config.to_runner_config();
        assert!(runner.polish);
        assert_eq!(runner.dictionary, vec!["PCM".to_owned()]);
        assert_eq!(
            runner
                .profiles
                .get("com.apple.Terminal")
                .map(String::as_str),
            Some("raw")
        );
        // Calibrated deadline projects through; None would keep the default.
        assert_eq!(
            runner.polish_deadline,
            std::time::Duration::from_millis(640)
        );
    }

    #[test]
    fn uncalibrated_deadline_keeps_the_runner_default() {
        let runner = Config::default().to_runner_config();
        assert_eq!(
            runner.polish_deadline,
            RunnerConfig::default().polish_deadline
        );
    }

    #[test]
    fn corrupt_config_falls_back_to_default() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(CONFIG_FILE), b"not = valid = toml =").expect("write");
        assert_eq!(load_from(&dir), Config::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn older_file_missing_keys_loads_with_defaults() {
        let dir = temp_dir("partial");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Only one key present; everything else must fill from Default.
        std::fs::write(dir.join(CONFIG_FILE), b"hotkey = \"Ctrl+Shift+D\"\n").expect("write");
        let loaded = load_from(&dir);
        assert_eq!(loaded.hotkey, "Ctrl+Shift+D");
        assert_eq!(loaded.polish, Config::default().polish);
        assert_eq!(loaded.history_retention_days, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_hotkeys_accepted() {
        for chord in [
            "Ctrl+Alt+Space",
            "cmd+shift+d",
            "Alt+F5",
            "Ctrl+Alt+Up",
            "super+g",
        ] {
            assert!(
                Config::validate_hotkey(chord).is_ok(),
                "{chord} should be valid"
            );
        }
    }

    /// A bare right-side modifier is the macOS push-to-talk binding
    /// (`modifier_tap`); the validator must not reject it as a chord.
    #[cfg(target_os = "macos")]
    #[test]
    fn bare_right_modifiers_accepted_on_macos() {
        for token in ["RightOption", "rcmd", "RightShift", "rctrl"] {
            assert!(
                Config::validate_hotkey(token).is_ok(),
                "{token} should be valid"
            );
        }
    }

    #[test]
    fn invalid_hotkeys_rejected() {
        assert_eq!(Config::validate_hotkey("").unwrap_err(), HotkeyError::Empty);
        assert_eq!(
            Config::validate_hotkey("Space").unwrap_err(),
            HotkeyError::NoModifier
        );
        assert_eq!(
            Config::validate_hotkey("Ctrl+Alt").unwrap_err(),
            HotkeyError::NoKey
        );
        assert!(matches!(
            Config::validate_hotkey("Ctrl+Splat").unwrap_err(),
            HotkeyError::UnknownToken(_)
        ));
        assert!(matches!(
            Config::validate_hotkey("Ctrl+Alt+Delete").unwrap_err(),
            HotkeyError::Reserved(_)
        ));
    }
}
