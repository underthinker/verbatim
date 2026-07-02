# Spike 2 - macOS text injection and TCC permissions

Status: research findings plus a local permission probe (`axprobe.swift` in the session scratchpad).
Live injection tests require interactive TCC grants and are deferred to M1 on a real build.
Machine: Apple M5, macOS 26.5.
Date: 2026-07-02.

## Probe result

From an unprivileged CLI process:

```
AXIsProcessTrusted: false
CGPreflightPostEventAccess: false
CGPreflightListenEventAccess: false
```

Confirms: no injection or event-listening capability exists until the user grants permissions in System Settings; the preflight APIs let us check state without triggering prompts, which the onboarding flow should use.

## Injection mechanisms

| Mechanism | Permission | Verdict |
|---|---|---|
| Pasteboard + synthetic Cmd-V (`CGEventPost`) | Accessibility | **Primary for normal text.** Fast for any length. Must mark our pasteboard entry `org.nspasteboard.TransientType` so clipboard managers ignore it, and restore the prior clipboard only after the target app has consumed the paste (changeCount-aware delay, not a fixed sleep; OpenWhispr's fixed-delay restore race is a documented failure) |
| `CGEventPost` unicode typing (`CGEventKeyboardSetUnicodeString`, ~20-char chunks) | Accessibility | **Fallback for paste-hostile targets** (some terminals, vim modes, remote desktops) and short strings. Slower but never touches the clipboard |
| AX API (`AXUIElementSetAttributeValue` on focused element) | Accessibility | Not reliable for writing (many apps, especially Electron/web views, expose non-settable values). **Valuable for reading**: focused app/element identity powers per-app polish profiles and secure-field detection |
| Input Method Kit (IMK) | none | The architecturally "correct" text path but requires the user to install and switch input sources; wrong model for hotkey dictation. Rejected |

## Hotkeys

- Carbon `RegisterEventHotKey` requires **no TCC permission** and delivers both pressed and released events, so toggle and hold-to-talk both work without Input Monitoring. This keeps the permission ask to Microphone + Accessibility only.
- The Globe/Fn key (Wispr Flow's signature trigger) is not reachable via Carbon; it needs a `CGEventTap`/NSEvent global monitor, which drags in Input Monitoring or Accessibility-based listening. Offer Globe as an optional trigger, never the default, so baseline onboarding stays at two permissions.

## TCC behavior to design around

- Microphone: standard prompt on first capture via `NSMicrophoneUsageDescription`.
- Accessibility: cannot be granted via an in-app dialog; `AXIsProcessTrustedWithOptions(prompt: true)` deep-links the user to System Settings. Onboarding needs an explicit guided step with a live re-check (poll the preflight APIs) and a "test injection" confirmation.
- Grants are keyed to the code-signing identity: an unsigned or ad-hoc-signed dev build loses permissions on every rebuild, and changing the signing identity resets user grants. Sign with a stable Developer ID from the first public build.
- App Sandbox is incompatible with Accessibility-based injection: Verbatim ships notarized via direct download and Homebrew, not the Mac App Store. This is a hard distribution constraint, record it in ENGINEERING.md.

## Secure input

- Password fields enable Secure Event Input, which blocks event *listening* (taps), not `CGEventPost` delivery; Carbon hotkeys have historically misbehaved under it as well.
- Detect via `IsSecureEventInputEnabled()`; UX response: show "secure field, dictation paused" state rather than failing silently.

## Recommendation for ARCHITECTURE.md

1. macOS `TextInjector` chain: transient-pasteboard paste -> unicode-typing fallback, selected per target app (per-app profile can pin the method).
2. macOS `HotkeyManager`: Carbon hotkeys as default; event-tap path only when the user opts into Globe-key or advanced triggers.
3. Permission subsystem exposes preflight state machine (unknown / denied / granted per capability) consumed by onboarding and by the overlay's error states.
4. Follow-up in M1: verify paste vs typing behavior in Terminal, iTerm2, VS Code, Safari, Slack, and a password field, with a signed dev build.
