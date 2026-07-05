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
EOF
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
