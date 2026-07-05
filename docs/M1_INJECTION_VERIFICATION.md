# M1 injection verification (#18)

The M1 acceptance criterion for injection is:

> Dictation lands text in a foreign app on all platforms, including GNOME Wayland (portal or uinput) and KDE.

Two layers back this: automated seam tests that run in CI, and manual real-keypress checks that need a desktop session, a permission grant, or specific hardware and so cannot run headless.

## Automated coverage (CI)

The per-platform seam tests assert the injection contract without a GUI, and run on their matching CI runner under the real-injection feature (`cargo test --workspace --all-features`):

- `crates/verbatim-platform/tests/macos_seams.rs` (macOS, `mac-inject`)
- `crates/verbatim-platform/tests/windows_seams.rs` (Windows, `win-inject`)
- `crates/verbatim-platform/tests/linux_seams.rs` (Linux, `linux-inject`)

Each asserts:

- the probe always offers `ClipboardOnly` as the single last resort, and event-capable backends are tried before it (never surrender to a manual paste while a real backend could deliver);
- the permission probe answers every capability without prompting or blocking;
- clipboard and focus behaviours that mutate real state or need a window server are gated behind a `VERBATIM_<PLATFORM>_E2E=1` env var so the default suite never touches the developer's clipboard.

The cross-crate E2E smoke (`crates/verbatim-app/tests/walking_skeleton.rs`) additionally proves, over the fake seam, that a blocked primary backend falls back to the clipboard with the text preserved (E4) and that a secure field refuses honestly (E5).

## Manual real-keypress checks

These are the "verified end-to-end (real keypress -> text in target)" bullet. Run each on a real desktop session and record the result in the issue.

Run `scripts/verify-injection.sh` (macOS/Linux) or `pwsh scripts/verify-injection.ps1` (Windows) to execute the host platform's seam E2E and print its checklist in one command - see [scripts/README.md](../scripts/README.md).

For every platform: open a plain text editor, put the caret in it, keep it focused, trigger dictation, speak a known phrase, and confirm the exact text appears **in the editor** (not just on the clipboard).

### macOS
- [ ] Grant Accessibility to the app (or the terminal running it).
- [ ] Dictate into TextEdit; confirm text lands via `TransientPasteboardPaste` and the prior clipboard is restored.
- [ ] Revoke Accessibility; confirm honest degrade to clipboard-only (E4 notice, text on clipboard).
- Local seam E2E verified 2026-07-04 on Apple M5 (real NSPasteboard transient-write/restore + frontmost-app focus): `VERBATIM_MAC_E2E=1 cargo test -p verbatim-platform --features mac-inject --test macos_seams`.

### Windows
- [ ] Dictate into Notepad; confirm text lands via `SendInputUnicode`.
- [ ] Dictate into an **elevated** window (e.g. an admin console); confirm UIPI blocks SendInput, the failure is detected (short insert), and it falls through to clipboard (E4) rather than silently dropping text.
- [ ] Confirm the user's prior clipboard is restored after a paste-backed injection.

### Linux - GNOME Wayland
- [ ] First dictation pops the RemoteDesktop portal consent dialog; approve it.
- [ ] Confirm text lands via `LibeiPortal` into a focused GNOME app (e.g. gedit/GNOME Text Editor).
- [ ] Restart the daemon; confirm it reconnects **silently** via the persisted `restore_token` (no second consent dialog).
- [ ] Deny/revoke the portal; confirm fall-through to uinput (if `/dev/uinput` is writable) or clipboard-only (E4).

### Linux - KDE / Plasma 6
- [ ] Repeat the GNOME Wayland checks on KDE Plasma 6 (Kate/KWrite as the target).
- [ ] Confirm the overlay never steals focus (the spike-1 KDE regression case).

## Status

- macOS: seam E2E verified on real hardware; foreground-app real-keypress check pending a controlled desktop run.
- Windows: automated seams added; real-keypress + UIPI check pending a Windows machine.
- Linux GNOME Wayland / KDE: automated seams present; portal/uinput real-keypress checks pending real desktop sessions.
