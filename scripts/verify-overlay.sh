#!/usr/bin/env bash
# Overlay verification for M2 acceptance criterion (docs/ROADMAP.md, "Overlay
# never takes focus ... and respects reduced-motion").
#
# Drives a real session against a real window server and asserts the overlay
# never becomes key and never pulls the frontmost app away from the dictation
# target. macOS is checked here; the KDE Plasma 6 case (the spike-1 focus-steal
# regression) needs a Linux desktop and stays a manual checklist.
#
# The reduced-motion half of the criterion is not checked here: overlay.css
# stills the pill under a `prefers-reduced-motion: reduce` media query that
# covers every descendant, so no per-rule assertion can go stale.
#
# The session runs on the default (fake) capture + engine backends, so no
# microphone, model, or permission grant is required: the overlay is driven by
# the same `SessionRunner` transitions a real dictation produces.
#
# Usage:
#   scripts/verify-overlay.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VERBATIM="$REPO_ROOT/target/debug/verbatim"
VITE_PORT=1420
GUI_PID=""
VITE_PID=""

# The Tauri shell does not quit on SIGTERM, and a survivor would bind the
# trigger socket out from under the next run.
cleanup() {
  [ -n "$GUI_PID" ] && kill -9 "$GUI_PID" 2>/dev/null || true
  [ -n "$VITE_PID" ] && kill -9 "$VITE_PID" 2>/dev/null || true
}
trap cleanup EXIT

case "$(uname -s)" in
  Darwin) PLATFORM="macOS" ;;
  Linux)  PLATFORM="Linux" ;;
  *) echo "error: unsupported OS $(uname -s)" >&2; exit 1 ;;
esac

echo "==> Platform: ${PLATFORM}"

# ---------------------------------------------------------------------------
# Focus: the overlay must be mapped and on top while never becoming key. On
# macOS `focusable(false)` compiles to `canBecomeKeyWindow: NO`, so `show()`'s
# `makeKeyAndOrderFront:` orders front without activating. That is the property
# under test - not the flag, but its observable effect on a real window server.
# ---------------------------------------------------------------------------
frontmost_app() {
  osascript -e 'tell application "System Events" to name of first process whose frontmost is true' 2>/dev/null
}

# `AXFocused` is useless here: it is false for every window of an app that is
# not active, so it reads clean even for an overlay that did become key. The
# properties that actually discriminate are `AXMain` on the pill and the app's
# own `AXFocusedWindow` - both flip when `focusable(false)` is dropped.
overlay_attrs() {
  osascript <<'APPLESCRIPT' 2>/dev/null
tell application "System Events"
  tell (first process whose name contains "erbatim")
    set w to first window whose name is "Verbatim Overlay"
    set overlayIsFocused to false
    try
      set overlayIsFocused to (name of (value of attribute "AXFocusedWindow") is "Verbatim Overlay")
    end try
    return (frontmost as text) & " " & (value of attribute "AXMain" of w as text) & " " & (overlayIsFocused as text)
  end tell
end tell
APPLESCRIPT
}

overlay_exists() {
  osascript -e 'tell application "System Events" to tell (first process whose name contains "erbatim") to exists (first window whose name is "Verbatim Overlay")' 2>/dev/null
}

# Give the dictation target a real window to own the focus, then confirm we can
# see it. A locked screen, or a terminal without Accessibility, answers every
# window query with an empty list - which would otherwise read as "the overlay
# never appeared" rather than "this check cannot run". Fail loudly on that.
open_target() {
  osascript >/dev/null 2>&1 <<'APPLESCRIPT'
tell application "TextEdit"
  activate
  if (count of documents) is 0 then make new document
end tell
APPLESCRIPT
  sleep 1
  if [ -z "$(osascript -e 'tell application "System Events" to get name of every window of process "TextEdit"' 2>/dev/null)" ]; then
    echo "SKIP: cannot enumerate windows. Unlock the screen and grant" >&2
    echo "      Accessibility to this terminal, then re-run." >&2
    exit 2
  fi
}

verify_focus_macos() {
  echo
  echo "==> Focus (real session, real window server)..."

  cargo build --locked -p verbatim-app --bin verbatim

  # The debug GUI loads the Vite dev server; start one if nothing is listening.
  if ! lsof -i ":${VITE_PORT}" >/dev/null 2>&1; then
    (cd ui && pnpm dev >/dev/null 2>&1) &
    VITE_PID=$!
    sleep 4
  fi

  # A focused dictation target, exactly as a user would have.
  open_target
  local target
  target="$(frontmost_app)"
  [ "$target" = "TextEdit" ] || { echo "FAIL: could not focus TextEdit (frontmost=${target})" >&2; return 1; }

  "$VERBATIM" gui >/dev/null 2>&1 &
  GUI_PID=$!

  # Wait for the daemon to own the trigger socket rather than guessing at a
  # startup delay; a `trigger start` sent too early is silently dropped.
  local waited=0
  until "$VERBATIM" status >/dev/null 2>&1; do
    [ "$waited" -ge 30 ] && { echo "FAIL: the GUI never began serving triggers" >&2; return 1; }
    sleep 1; waited=$((waited + 1))
  done

  [ "$(frontmost_app)" = "TextEdit" ] \
    || { echo "FAIL: launching the GUI stole focus from TextEdit" >&2; return 1; }

  "$VERBATIM" trigger start >/dev/null

  # The overlay is mapped by the webview a beat after ARMING; poll for it, so a
  # slow first paint reads as slow rather than as a passing focus check on a
  # window that was never on screen.
  waited=0
  until [ "$(overlay_exists)" = "true" ]; do
    [ "$waited" -ge 15 ] && { echo "FAIL: overlay window never appeared; the focus check would be vacuous" >&2; return 1; }
    sleep 1; waited=$((waited + 1))
  done

  local attrs app_frontmost overlay_main overlay_focused
  attrs="$(overlay_attrs)"
  read -r app_frontmost overlay_main overlay_focused <<<"$attrs"

  "$VERBATIM" trigger stop >/dev/null

  [ "$(frontmost_app)" = "TextEdit" ] \
    || { echo "FAIL: overlay pulled focus off the dictation target" >&2; return 1; }
  [ "$app_frontmost" = "false" ] \
    || { echo "FAIL: Verbatim became the frontmost app while the overlay showed" >&2; return 1; }
  [ "$overlay_main" = "false" ] \
    || { echo "FAIL: the overlay became the main (key) window" >&2; return 1; }
  [ "$overlay_focused" = "false" ] \
    || { echo "FAIL: the overlay became the app's focused window" >&2; return 1; }

  echo "PASS: overlay shown, never key, TextEdit kept focus throughout the session."
}

if [ "$PLATFORM" = "macOS" ]; then
  verify_focus_macos
else
  cat <<'EOF'

==> Focus: manual on Linux (record results in the M2 milestone).

  GNOME (Wayland + X11) and KDE Plasma 6:
  [ ] Focus a text editor, trigger dictation, and confirm the caret keeps
      blinking in the editor while the pill is up - the pill must never take
      the keyboard focus (spike-1: this is what broke Handy on KDE).
  [ ] The pill is click-through: clicking where it sits hits the window behind.

  If KDE steals focus the first suspect is focus-on-map, not accept-focus: tao
  sets `gtk_window_set_accept_focus(false)` for `focusable(false)` but leaves
  `gtk_window_set_focus_on_map` at its default, and KWin honours the latter when
  mapping a new window.
EOF
fi
echo
