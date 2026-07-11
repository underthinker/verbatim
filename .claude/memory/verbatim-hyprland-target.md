---
name: verbatim-hyprland-target
description: The Arch/Hyprland dev box is a Verbatim target platform; its capabilities and latency numbers
metadata: 
  node_type: memory
  type: project
  originSessionId: de4b2e6e-22e7-4fd7-9f89-c5763d4ca8fe
---

User's Arch Linux + Hyprland (wlroots, Wayland) desktop is a Verbatim **target** platform (stated 2026-07-11: "I use this linux machine for development").

Hardware: Ryzen 7 5700X3D (16T), 46 GB RAM, Radeon RX 5700 XT (Navi 10), Vulkan + cmake 4.4 + gcc 16 present.

Injection environment:
- `libei-1.0` v1.6.0 present, but **xdg-desktop-portal-hyprland has NO `org.freedesktop.portal.RemoteDesktop` interface** → `LinuxTextInjector` LibeiPortal backend always fails here, falls through.
- `/dev/uinput` needs the **`uinput` kernel module loaded** (`modprobe uinput`, persisted via `/etc/modules-load.d/uinput.conf`); the node's ACL already grants the active-session user rw. With the portal absent, uinput is the ONLY real-keystroke path on Hyprland.
- **Injection #18 real-keypress VERIFIED on Hyprland (2026-07-11):** with uinput loaded, `verbatim inject-selftest` probed `[LibeiPortal, Uinput, ClipboardOnly]`, LibeiPortal failed (no RemoteDesktop), fell to `backend=Uinput verified=true`, and the sentinel `Verbatim inject self-test OK 1234 你好` landed byte-exact in a Chromium textarea (read back via Playwright). Multibyte UTF-8 survives; target kept focus. Known limit: the injector sends Ctrl+V, so terminal targets (which paste on Ctrl+Shift+V) won't receive via this chord.

Latency (#16, resident `ggml-base.en`, 10s fixture, 20 iters), both **over** the 800 ms budget on this box:
- CPU: p50 894 ms / p95 920 ms
- Vulkan GPU: p50 1192 ms / p95 2207 ms (slower than CPU — GPU dispatch overhead dominates for base.en; matches the Windows RX 5700 XT note in ROADMAP).

Build deps needed on Arch for the Tauri crate: `webkit2gtk-4.1` (+ keep `libjxl` in sync via full `-Syu`). `pnpm` was not installed (installed via `npm i -g pnpm`; lands in `~/.local/bin`).

See [[verbatim-wayland-overlay-focus-steal]] and [[verbatim-gui-tao-panic-fix]].
