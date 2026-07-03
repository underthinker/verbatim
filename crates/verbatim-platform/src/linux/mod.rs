//! Linux implementations (M1 wire-up pending).
//!
//! Planned per spike 1: GlobalShortcuts portal + `verbatim trigger` IPC +
//! opt-in evdev listener for hotkeys; injection chain libei portal ->
//! uinput -> wlroots virtual-keyboard-v1 -> clipboard-assisted paste ->
//! clipboard-only. In-process backends only; never shell out to
//! ydotool/wtype, never trust exit codes as success.
