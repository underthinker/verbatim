# Spike 1 - Wayland text injection and global hotkeys

Status: research-based findings (no Linux box in this session; validate on real hardware before architecture freeze).
Sources: [murmure Wayland tracking discussion](https://github.com/Kieirra/murmure/discussions/305), [Handy](https://github.com/cjpais/handy) issue tracker and [Ubuntu setup notes](https://github.com/danielrosehill/Handy-Ubuntu-Setup), [OpenWhispr #240](https://github.com/OpenWhispr/openwhispr/issues/240), [voxtype #306](https://github.com/peteonrails/voxtype/issues/306), [wayland.app virtual-keyboard-v1](https://wayland.app/protocols/virtual-keyboard-unstable-v1), [rustdesk libei discussion](https://github.com/rustdesk/rustdesk/discussions/4515).
Date: 2026-07-02.

## Question

Can Verbatim inject text and register global hotkeys reliably on Wayland (GNOME, KDE, wlroots), and which mechanisms should the architecture standardize on?

## Text injection: mechanism-by-mechanism verdict

| Mechanism | GNOME (Mutter) | KDE (KWin) | wlroots (Hyprland, Sway) | Notes |
|---|---|---|---|---|
| `virtual-keyboard-unstable-v1` (wtype) | **Refused upstream** | **Refused upstream** | Works | wlroots-only; dead end as a primary path |
| `/dev/uinput` direct (what ydotool does, minus the daemon) | Works | Works | Works | Kernel-level, compositor-independent; needs `input` group or udev `uaccess` rule; keymap mapping is on us |
| XDG RemoteDesktop portal + libei | GNOME >= 46 | Plasma >= 6.1 | Not yet (xdg-desktop-portal-wlr lacks libei support) | Permission dialog on first run; `restore_token` persists consent for silent reconnect; Flatpak-clean |
| ydotool (daemon) | Works | Works | Works | Root/daemon/socket-permission mess in practice (see voxtype #306); do not depend on it |
| Clipboard + paste keystroke | Race-prone | Race-prone | Race-prone | Still needs a synthetic Ctrl+V, which recurses into this same table; clipboard restore races documented in OpenWhispr #240 |
| Clipboard only (user pastes manually) | Works | Works | Works | Last-resort fallback, never the default |

Why competitors break (verified failure modes):

- Handy defaults to enigo, which silently fails on Wayland; users must manually install and select ydotool, and its overlay stole focus so typed output missed the active window.
- OpenWhispr tries xdotool first; under XWayland it exits 0 while injecting nothing into native Wayland windows, so the working fallback is never reached. Silent-success detection is a real trap.
- Murmure (the app with the best current Linux reputation) landed on direct `/dev/uinput` as primary, and hides its overlay before injecting to avoid the KDE focus-steal bug.

## Global hotkeys

- XDG GlobalShortcuts portal: reliable on KDE Plasma 6.x; inconsistent on GNOME due to an upstream Mutter portal bug (same bug affects Discord and OBS); needs GNOME 48+ to be even nominally available.
- Workable GNOME fallback: expose a CLI/IPC trigger (`verbatim trigger`) that users bind to a native GNOME custom shortcut; murmure ships this as its default on non-KDE compositors.
- Raw evdev key monitoring (what rdev-style hooks do) requires the same `input`-group access as uinput; viable as an opt-in power-user path, and hold-to-talk essentially requires it or the portal.

## Recommendation for ARCHITECTURE.md

1. `TextInjector` is a per-platform trait with an ordered, capability-probed backend chain; on Linux: libei/portal -> uinput -> wlroots virtual-keyboard -> clipboard-assisted paste -> clipboard-only. Probing must verify injection actually landed where possible, never trust exit codes.
2. Ship our own uinput and libei backends in-process (Rust: `evdev`/`uinput` and `reis` crates). No runtime dependency on ydotool/wtype binaries.
3. `HotkeyManager` on Linux: GlobalShortcuts portal (KDE), CLI/IPC trigger registration flow (GNOME), evdev opt-in for hold-to-talk.
4. Overlay must be non-focusable and hidden before injection (KDE focus-steal, Handy overlay bug).
5. Onboarding must include a Linux permissions step (udev rule / input group with one-click script, or portal consent) with a live "test injection" check.
6. X11 is comparatively trivial (XTEST) and not a risk item.

## Confidence and follow-up

Confidence: high on mechanism viability (multiple shipping apps confirm each row), low on our own implementation effort estimates.
Before M1 completes, re-run this spike as running code on Ubuntu 24.04 GNOME Wayland, Plasma 6, and Hyprland: inject into a GTK app, a Qt app, an Electron app, and a terminal; measure injection latency; confirm `restore_token` persistence.
