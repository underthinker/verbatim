# Verbatim - UX Specification

Status: signed off with the PRD (2026-07-02); maintained as the living spec - implementation follows this doc, not the reverse.
Companion documents: [PRD.md](PRD.md) for scope, [ARCHITECTURE.md](ARCHITECTURE.md) for the subsystems referenced here.

## 1. Experience principles

1. **Invisible until summoned.** Verbatim lives in the menu bar/tray; it never opens windows uninvited, never interrupts, never nags.
2. **Never lose the user's words.** Audio that was recorded is always transcribed or recoverable; failures degrade (clipboard, history) rather than discard.
3. **No dead ends.** Every error state names the problem in plain language and offers exactly one primary action to fix it.
4. **Fast feels correct.** Perceived latency matters more than measured latency: instant overlay feedback on hotkey press, visible processing state, text arriving in one chunk.
5. **The polish is trustworthy.** Cleaned text must never change meaning; when confidence is low, prefer raw. Raw mode is always one modifier away.

## 2. The core loop

The dictation loop is a strict state machine.
Every state has: overlay presentation, tray icon variant, allowed transitions, and timeout behavior.

```
                     hotkey down
        IDLE ──────────────────────► ARMING
          ▲                             │ mic stream open (< 100 ms)
          │                             ▼
          │                         RECORDING ◄────────┐
          │                             │              │ (audio continues)
          │            hotkey up /      │              │
          │            toggle-stop      │              │
          │                             ▼              │
          │                         FINALIZING ────────┘ (VAD tail capture, ≤ 300 ms)
          │                             │
          │                             ▼
          │                        TRANSCRIBING
          │                             │
          │                             ▼
          │                         POLISHING   (skipped in raw mode; hard deadline)
          │                             │
          │                             ▼
          │                         INJECTING
          │        success             │
          └─────────────────────────────┘
```

State-by-state specification:

| State | Overlay | Exit conditions | Failure handling |
|---|---|---|---|
| IDLE | hidden | hotkey down -> ARMING | - |
| ARMING | appears immediately with mic icon, "starting" shimmer | stream open -> RECORDING; stream fails -> error E1 | mic unavailable -> E1 |
| RECORDING | live waveform driven by input level; elapsed time after 10 s; ESC hint | hotkey up (hold mode) or hotkey press (toggle mode) -> FINALIZING; ESC -> cancel to IDLE (discard); 5 min cap -> auto-FINALIZING with notice | device disconnect mid-recording -> E6 |
| FINALIZING | waveform freezes, short sweep animation | VAD tail flushed -> TRANSCRIBING | - |
| TRANSCRIBING | indeterminate progress ("transcribing") | text ready -> POLISHING or INJECTING (raw mode) | engine error -> E3 |
| POLISHING | same progress state, no separate visual phase | polished text or deadline (see 5.2) -> INJECTING | polish failure/timeout -> silently inject raw text; count it, never block |
| INJECTING | brief success tick, then overlay fades (200 ms) | injected -> IDLE | injection fails -> E4 |

Global rules:

- Empty/silent recordings (VAD saw no speech) return to IDLE with a soft "didn't catch anything" overlay flash, no error dialog.
- The overlay is click-through and never takes keyboard focus (spike 1: focus-steal broke Handy on KDE; the same rule applies everywhere).
- Every transcription (raw and polished) is written to local history before injection is attempted, satisfying principle 2.

## 3. Hotkey semantics

- **Hold-to-talk** (default): key down starts, key up stops. Presses shorter than 250 ms are treated as accidental: show a 1.5 s hint ("hold to dictate, double-tap to lock") and discard.
- **Toggle**: press starts, press again stops. Recording cap and ESC-cancel still apply.
- **Double-tap lock** (hold mode): double-press locks recording as if toggled; next press stops.
- Hotkey pressed while TRANSCRIBING/POLISHING/INJECTING: queue a new ARMING immediately (the pipeline is asynchronous; a user may chain dictations). Two concurrent recordings are impossible by construction.
- Hotkey during an error overlay: dismiss the error and start recording, unless the error blocks recording itself (E1, E2).
- Default binding: a chord that is free on all three OSes (proposal: `Ctrl+Alt+Space` / `⌥ Space` on macOS, conflict-checked during onboarding; Globe key offered as optional macOS trigger per spike 2).

## 4. Error catalog

Every error has an ID, detection point, overlay copy pattern, and one primary action.
Copy tone: plain, no jargon, no blame, never "Error 0x80004005."

| ID | Condition | Behavior | Primary action |
|---|---|---|---|
| E1 | Microphone missing or permission denied | Overlay: "Verbatim can't hear you yet - microphone access is off." | Open the OS permission pane (deep link), re-check live |
| E2 | No model downloaded (first run raced, or user deleted models) | Overlay: "One-time setup: download a speech model (about 150 MB)." | Open model manager with recommended model preselected |
| E3 | Transcription engine crash/failure | Inject nothing; keep audio; overlay: "Transcription hit a snag - your recording is saved." | "Retry" (re-runs from saved audio); second failure offers engine/backend switch |
| E4 | Text injection failed (no writable focus, injection rejected) | Text already in history; copy to clipboard; overlay: "Couldn't type into this app - your text is on the clipboard." | "Paste anyway" hint (Cmd/Ctrl+V) |
| E5 | Secure field focused (spike 2 detection) | Refuse to inject silently; overlay: "That looks like a password field, so Verbatim stayed out. Text is on the clipboard." | none needed |
| E6 | Input device disconnected mid-recording | Stop gracefully, transcribe what was captured, then overlay notice: "Your mic disconnected - transcribed what I heard." | Open input-device picker |
| E7 | Focused app changed between hotkey-up and injection | Inject into the newly focused app only if it equals the app at recording start; otherwise treat as E4 (clipboard fallback) | as E4 |
| E8 | Model download failure (network, disk) | In model manager, inline: resumable retry with byte progress | "Resume download" |
| E9 | Linux: no injection permission (uinput/portal not granted, spike 1) | Onboarding-grade guided fix with one-click script / portal consent, plus live "test injection" | "Set up typing" |
| E10 | Polish model missing/failed while polish enabled | Silent fallback to raw injection + one-time tray notice, not a per-dictation error | Settings link |

Mic switch while recording (device picker used mid-recording) is not an error: the recorder follows the default device seamlessly if possible, else E6.

## 5. Text polish UX

### 5.1 Modes

- **Polished** (default after onboarding opt-in): filler removal, punctuation, capitalization, personal dictionary, per-app profile.
- **Raw**: exact transcription. Toggled globally in tray menu, or per-dictation by holding a modifier (default `Shift`) with the hotkey.
- Per-app profile selects tone presets (e.g. "code comments: no capitalization changes," "email: full sentences") and can force raw for specific apps (terminals default to raw).

### 5.2 Trust rules

- Polish runs under a hard latency deadline (from ARCHITECTURE.md budget; target ≤ 700 ms beyond transcription). Deadline miss -> raw text injected, no user-visible failure.
- History stores raw and polished text side by side; the history window offers "copy raw" on every entry, and a diff view on hover.
- The personal dictionary is user-visible and editable (Settings), never a black box; every auto-learned term requires one-click confirmation before it starts being applied.

## 6. First-run onboarding

Target from PRD: installer download to first successful dictation in under 5 minutes, no documentation.

Steps (one screen each, progress dots, all skippable-but-discouraged except permissions):

1. **Welcome**: one sentence of promise, one "Get started" button. Privacy statement inline: "Everything runs on this computer. Nothing is ever uploaded."
2. **Microphone permission**: OS prompt triggered on user click, live state check, success tick.
3. **Typing permission**: per-OS flow (macOS Accessibility deep link with re-check polling; Windows none needed; Linux guided uinput/portal setup with "test typing here" input box - spike 1/2 findings).
4. **Model download**: one recommended model preselected per hardware (detected RAM/GPU), size and disk shown, background download with progress; "you can keep going" once the small model lands.
5. **Try it**: an in-app text field, prompt "hold [hotkey] and say anything." Success celebrates with the transcribed text appearing; this validates the whole pipeline inside our own window before the user trusts it elsewhere.
6. **Polish opt-in**: show one before/after example, offer to download the polish model (size shown), "skip for now" equally prominent.

Re-entry: any failed permission later (E1, E9) deep-links back to exactly the relevant onboarding step, not the start.

## 7. Surfaces

- **Overlay**: small pill, bottom-center of the active display, above all windows, click-through, non-activating; respects OS reduced-motion settings; auto-dark/light. Position configurable (bottom-center, top-center, near-cursor).
- **Tray/menu bar**: icon reflects state (idle / recording dot / processing spinner / error badge). Menu: pause Verbatim, raw/polished toggle, input device picker, recent dictations (last 5), Settings, Quit.
- **Settings window**: tabs - General (hotkeys, launch at login), Dictation (mode, language, VAD sensitivity), Polish (model, dictionary, per-app profiles), Models (manager: download/delete/default, disk usage), History (retention, clear-all), About (versions, licenses incl. Parakeet CC-BY attribution).
- **History window**: reverse-chronological list, raw/polished pairs, copy buttons, search; retention default 7 days, configurable including "off."
- **Notifications**: OS-native, used only for: model download completed in background, and E10-class silent degradations. Never for routine success.

## 8. Accessibility

- Full keyboard navigability of all windows; visible focus rings.
- Screen-reader labels on every control; overlay state changes announced via the OS accessibility notification API (configurable, default on when a screen reader is detected).
- No information conveyed by color alone; overlay states differ by icon and shape, not only hue.
- Respect OS reduced-motion (disable waveform animation, use static level meter) and high-contrast modes.
- All timeouts that auto-dismiss overlays are extended when assistive tech is active.

## 9. Open UX questions

- Near-cursor overlay placement needs a feasibility check per platform (global cursor position permission on Wayland is restricted); ship display-edge placement first.
- Whether history should be on or off by default for the privacy-first audience: proposal is on with 7-day retention, decided at PRD sign-off.
