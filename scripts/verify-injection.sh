#!/usr/bin/env bash
# Portable injection verification for M1 acceptance criterion (docs/ROADMAP.md:25, issue #18).
#
# Runs the real-state seam E2E for the host platform (mutates the real clipboard /
# reads frontmost focus, so it stays gated behind an env var in the default suite),
# then prints the manual real-keypress checklist that only a live desktop session
# can complete. macOS and Linux only; use verify-injection.ps1 on Windows.
#
# Usage:
#   scripts/verify-injection.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

case "$(uname -s)" in
  Darwin)
    PLATFORM="macOS"; FEATURE="mac-inject"; TEST="macos_seams"; E2E_VAR="VERBATIM_MAC_E2E" ;;
  Linux)
    PLATFORM="Linux"; FEATURE="linux-inject"; TEST="linux_seams"; E2E_VAR="VERBATIM_LINUX_E2E" ;;
  *)
    echo "error: unsupported OS $(uname -s); use verify-injection.ps1 on Windows." >&2
    exit 1 ;;
esac

# Machine-check the macOS "text landed in the editor" boxes by injecting into a
# real TextEdit document and reading it back over Apple Events, instead of
# asking the operator to confirm by eye. The expectation is derived from the
# receipt the injector reports, so the same run ticks box 2 when Accessibility
# is granted and box 3 when it is not - the two are the same experiment, and
# only the honest receipt distinguishes them.
textedit_readback() {
  # ASCII + digits + CJK, so a truncating or encoding-broken backend (and the
  # 20-UTF-16-unit CGEvent chunking) shows itself in the read-back.
  local sentinel="Verbatim inject self-test OK 1234 你好"
  local prior="PRIOR-CLIPBOARD-$$-${RANDOM}"

  cargo build --locked --features real-injection -p verbatim-app --bin verbatim

  printf '%s' "$prior" | pbcopy
  osascript >/dev/null <<'APPLESCRIPT'
tell application "TextEdit"
  activate
  close every document saving no
  make new document
end tell
APPLESCRIPT

  # A freshly launched TextEdit is not immediately key: a Cmd-V posted while it
  # is still coming up goes nowhere, and the check reports a phantom failure.
  # Wait for it to actually own the front window before injecting.
  local waited=0
  until [ "$(osascript -e 'tell application "System Events" to name of first process whose frontmost is true' 2>/dev/null)" = "TextEdit" ] \
     && [ "$(osascript -e 'tell application "TextEdit" to count documents' 2>/dev/null)" -ge 1 ]; do
    [ "$waited" -ge 10 ] && { echo "FAIL: TextEdit never became frontmost with a document" >&2; return 1; }
    sleep 1; waited=$((waited + 1))
  done

  local output
  output="$(./target/debug/verbatim inject-selftest "$sentinel" 2>&1)"
  echo "$output" | sed 's/^/  [selftest] /'

  sleep 1
  local landed clipboard backend
  landed="$(osascript -e 'tell application "TextEdit" to get text of front document')"
  clipboard="$(pbpaste)"
  backend="$(printf '%s' "$output" | sed -n 's/.*receipt: backend=\([A-Za-z]*\).*/\1/p')"
  osascript -e 'tell application "TextEdit" to close front document saving no' >/dev/null 2>&1

  echo
  case "$backend" in
    TransientPasteboardPaste|CgEventTyping)
      # Accessibility granted: the text must reach the editor, and the user's
      # clipboard must survive the paste.
      [ "$landed" = "$sentinel" ] || { echo "FAIL: backend=$backend but the editor holds: '${landed}'" >&2; return 1; }
      [ "$clipboard" = "$prior" ] || { echo "FAIL: prior clipboard not restored; holds: '${clipboard}'" >&2; return 1; }
      echo "PASS (box 2): text landed in TextEdit via ${backend}; prior clipboard restored."
      ;;
    ClipboardOnly)
      # Accessibility absent: nothing may be typed, and the text must be staged
      # rather than silently dropped (E4).
      [ -z "$landed" ] || { echo "FAIL: clipboard-only fallback still typed into the editor: '${landed}'" >&2; return 1; }
      [ "$clipboard" = "$sentinel" ] || { echo "FAIL: text was neither injected nor staged on the clipboard" >&2; return 1; }
      echo "PASS (box 3): honest degrade to clipboard-only; nothing typed, text staged (E4)."
      ;;
    *)
      echo "FAIL: no receipt parsed from inject-selftest (real-injection build?)" >&2
      return 1 ;;
  esac
}

echo "==> Platform: ${PLATFORM}"
echo "==> Running seam E2E (${E2E_VAR}=1, real clipboard/focus)..."
echo

env "${E2E_VAR}=1" \
  cargo test --locked -p verbatim-platform --features "$FEATURE" --test "$TEST"

echo
echo "======================================================================"
echo " Automated seam E2E passed. Manual real-keypress checklist for ${PLATFORM}"
echo " (docs/M1_INJECTION_VERIFICATION.md) - record results in issue #18:"
echo "======================================================================"
echo
echo " For every check: open a plain text editor, keep the caret focused there,"
echo " trigger dictation, speak a known phrase, confirm the EXACT text lands"
echo " IN THE EDITOR (not merely on the clipboard)."
echo

if [ "$PLATFORM" = "macOS" ]; then
  cat <<'EOF'
  [ ] Grant Accessibility to the app (or the terminal running it).
  [ ] Dictate into TextEdit; text lands via TransientPasteboardPaste and the
      prior clipboard is restored.
  [ ] Revoke Accessibility; honest degrade to clipboard-only (E4 notice, text
      on clipboard, no silent drop).

  Boxes 2 and 3 are machine-checkable: re-run with VERBATIM_TEXTEDIT_E2E=1 to
  drive a real TextEdit document through `verbatim inject-selftest` and read the
  result back over Apple Events (needs no permission the injector lacks). The
  check takes over TextEdit for a few seconds; whichever grant state you are in
  is the box it ticks.
EOF
  if [ "${VERBATIM_TEXTEDIT_E2E:-0}" = "1" ]; then
    echo
    echo "==> VERBATIM_TEXTEDIT_E2E=1: driving TextEdit..."
    textedit_readback
  fi
else
  cat <<'EOF'
  GNOME Wayland:
  [ ] First dictation pops the RemoteDesktop portal consent dialog; approve it.
  [ ] Text lands via LibeiPortal into a focused GNOME app (gedit / Text Editor).
  [ ] Restart the daemon; it reconnects SILENTLY via the persisted restore_token
      (no second consent dialog).
  [ ] Deny/revoke the portal; fall-through to uinput (if /dev/uinput writable)
      or clipboard-only (E4).

  KDE / Plasma 6:
  [ ] Repeat the GNOME Wayland checks (Kate / KWrite as target).
  [ ] The overlay never steals focus (the spike-1 KDE regression case).
EOF
fi
echo
