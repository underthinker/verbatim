---
name: verbatim-gui-tao-panic-fix
description: verbatim gui aborted at startup on GTK/wlroots; fixed by deferring overlay click-through to after first show
metadata: 
  node_type: memory
  type: project
  originSessionId: de4b2e6e-22e7-4fd7-9f89-c5763d4ca8fe
---

`verbatim gui` aborted at startup on Hyprland/any GTK+wlroots (SIGABRT, exit 134) before this fix. Root cause: `overlay::create_window` built the overlay with `.visible(false)` then called `window.set_ignore_cursor_events(true)` immediately. On GTK the hidden window's GDK surface is not realized, and tao 0.35.3 `event_loop.rs:457` (`WindowRequest::CursorIgnoreEvents(true)`) does `window.window().unwrap()` on the `None` GDK window → panic in a no-unwind GLib dispatch → abort. Fine on macOS/Windows (tolerate the call on a hidden window).

Fix (2026-07-11, on working tree, `crates/verbatim-app/src/overlay.rs`): removed the creation-time call; added `mark_click_through()` guarded by an `Arc<AtomicBool>` threaded through `spawn_driver`→`apply`, called once right after the overlay's first successful `window.show()`. Passes fmt/clippy/test. Not yet committed/PR'd as of this note.

Verified: after the fix `verbatim gui` launches on Hyprland (daemon listening, `verbatim status` → Idle, no panic).
