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
- [x] Grant Accessibility to the app (or the terminal running it).
- [x] Dictate into TextEdit; confirm text lands via `TransientPasteboardPaste` and the prior clipboard is restored.
- [x] Revoke Accessibility; confirm honest degrade to clipboard-only (E4 notice, text on clipboard).
- Local seam E2E verified 2026-07-04 on Apple M5 (real NSPasteboard transient-write/restore + frontmost-app focus): `VERBATIM_MAC_E2E=1 cargo test -p verbatim-platform --features mac-inject --test macos_seams`.

Verified 2026-07-09 on Apple M5 (macOS, Darwin 25.5), Accessibility granted to the host terminal:

- Granted: `inject-selftest` probed `[TransientPasteboardPaste, CgEventTyping, ClipboardOnly]`, resolved the target as `com.apple.TextEdit`, and returned `backend=TransientPasteboardPaste verified=true`. The sentinel (ASCII + digits + CJK, exercising the 20-UTF-16-unit chunking) landed byte-for-byte in the TextEdit document, read back over Apple Events rather than by eye, and the prior clipboard was restored.
- Revoked: the same run probed `[ClipboardOnly]` and returned `verified=false`. TextEdit stayed empty and the text was staged on the clipboard - the honest E4 degrade, no silent drop.
- Full dictation: `verbatim daemon` built with `real-injection,real-audio,real-transcription,global-hotkey`, `ggml-base.en` resident, Right Option push-to-talk. Speech into TextEdit walked `Arming -> Recording -> Finalizing -> Transcribing -> Polishing -> Injecting -> Idle`. `Injecting -> Idle` is reachable only on a `verified` receipt (`runner.rs`, `SessionRunner::inject`), so reaching `Idle` is itself the proof that a real backend delivered.

Boxes 2 and 3 are machine-checked rather than confirmed by eye. `VERBATIM_TEXTEDIT_E2E=1 scripts/verify-injection.sh` drives a real TextEdit document through `verbatim inject-selftest` and reads the result back over Apple Events, which needs no permission the injector does not already hold. The expectation is derived from the receipt the injector reports, so one command ticks box 2 when Accessibility is granted and box 3 when it is not; the two are the same experiment and only the honest receipt tells them apart.

Two defects surfaced during this verification, both fixed:

- The paste backend restored the clipboard 40 ms after posting Cmd-V. A target reads the pasteboard only when it processes the key event, and a *reading* app never bumps `changeCount`, so the restore always fired blind. Against a cold TextEdit this lost the race 3 times out of 3: the app pasted the user's **previous clipboard content** while the receipt still reported `verified = true`. If that clipboard held a secret, dictation typed it into the focused app. The restore now runs on a detached thread after a generous settle, off the injection path so it costs no latency.
- An empty capture reached the transcription engine, whose empty-buffer inference error surfaced as `E3` with a "Retry" over no audio. `UX.md` 2 calls for a soft return to Idle; the state machine already defined `(Finalizing, SilenceOnly) -> Idle` and nothing emitted it.

Investigated and dismissed: the first push-to-talk press after launch spent ~2.2 s in `Arming`, which is `audio.start()` blocking on the one-time macOS Microphone permission prompt, not a cold-start defect. A fresh daemon arms in ~117 ms cold and ~105 ms warm.

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

- macOS: complete. Seam E2E plus all three manual real-keypress checks verified on Apple M5, 2026-07-09.
- Windows: automated seams added; real-keypress + UIPI check pending a Windows machine.
- Linux GNOME Wayland / KDE: automated seams present; portal/uinput real-keypress checks pending real desktop sessions.
