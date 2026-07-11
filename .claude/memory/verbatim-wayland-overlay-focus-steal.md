---
name: verbatim-wayland-overlay-focus-steal
description: The overlay pill stole keyboard focus on native Wayland/Hyprland (focusable(false) is X11-only); FIXED with gtk-layer-shell (PR #78)
metadata: 
  node_type: memory
  type: project
  originSessionId: de4b2e6e-22e7-4fd7-9f89-c5763d4ca8fe
---

**FIXED 2026-07-11 (PR #78, branch `fix/wayland-overlay-focus-steal`, stacked on the panic-fix branch/PR #77).** Fix: promote the overlay `GtkWindow` (via Tauri `gtk_window()`) to a `gtk-layer-shell` surface on the overlay layer with `KeyboardMode::None` — the only protocol-level "never focus me" on Wayland — anchored bottom-center; `position_bottom_center` is a no-op on Linux (compositor owns layer placement); macOS/Windows keep `focusable(false)`. `gtk-layer-shell` is unmaintained (same gtk-rs 0.18 GTK3 stack Tauri ships); RUSTSEC-2024-0422/0423 added to deny.toml. Verified end-to-end on Hyprland with real uinput: overlay shown → sentinel lands byte-exact in the editor, pill never becomes active. Full gate green (fmt/clippy/test/deny). The layer-shell init must run before the surface first maps — done in `create_window` before the first `show()`.

Original bug (found 2026-07-11): on native Wayland/Hyprland the overlay pill **took compositor focus when it mapped**. Verified deterministically with hyprctl: focus a target (kitty), `verbatim trigger start`, and with the main window out of the way the active window becomes `verbatim | Verbatim Overlay` (focusHistoryID 0) every run; focus returns to the target only after `trigger stop`.

This is the spike-1 KDE focus-steal regression (ROADMAP.md line ~45) reproduced on wlroots. Cause: `overlay.rs` sets `.focusable(false)`, which tao compiles to `gtk_window_set_accept_focus(false)` — an **X11 concept that is a no-op under native Wayland** — and `gtk_window_set_focus_on_map` is left at its default. Nothing tells the compositor not to focus the overlay on map.

Impact is severe, not cosmetic: at INJECTING the uinput/portal Ctrl-V targets the compositor-focused window, so if the overlay holds focus the dictation pastes into the pill instead of the editor → end-to-end dictation broken on Hyprland.

Candidate fixes (design decision, unresolved): render the overlay as a wlroots layer-shell surface (gtk-layer-shell) which is non-focusable by protocol; or get `focus_on_map=false` onto the toplevel (no Tauri API today); or ship a compositor `noinitialfocus` window rule (not app-portable). The macOS `focusable(false)` path is correct and must not regress. See [[verbatim-hyprland-target]].
