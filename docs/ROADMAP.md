# Verbatim - Implementation Roadmap

Status: draft for sign-off.
Milestones are sequential; each has acceptance criteria that gate the next.
"All platforms" always means macOS (ARM + Intel), Windows 11 x64, Ubuntu 24.04 (X11 + Wayland GNOME/KDE).

## M0 - Spikes and design sign-off (done except hardware validation)

Deliverables: the five docs in `docs/`, spike findings 1-4 in `spikes/`.

Remaining acceptance criteria:

- [ ] Spike 1 re-run as running code on real Linux (GNOME Wayland, Plasma 6, Hyprland): inject into GTK/Qt/Electron/terminal targets via libei and uinput; confirm `restore_token` persistence.
- [ ] Spike 2 validated with a signed dev build: paste + typing injection into Terminal, iTerm2, VS Code, Safari, Slack, and a password field.
- [ ] Spike 3/4 re-measured on a mid-range Windows x64 laptop (CPU-only); polish default per hardware tier decided.
- [x] Tristen signs off on PRD (differentiation thesis, success criteria), name, and license. (2026-07-02: polish headline, Tauri 2 + Rust, all 3 platforms day one, hardware validation during M1, name Verbatim, MIT incl. branding.)

## M1 - Walking skeleton (ugly but real, all platforms)

Scope: hotkey -> record -> whisper.cpp -> inject raw text, tray icon with quit, `verbatim trigger` CLI, fake-capture test seam, CI matrix from the first commit.

Acceptance criteria:

- [ ] Fresh checkout builds and packages unsigned dev artifacts on all platforms in CI.
- [ ] Dictation lands text in a foreign app on all platforms, including GNOME Wayland (portal or uinput) and KDE.
- [ ] Injection failure is detected honestly (no silent success) and falls back to clipboard.
- [ ] p50 raw latency < 800 ms for a 10 s utterance on Apple Silicon and the reference Windows laptop (resident model).
- [ ] State machine has exhaustive unit tests; E2E smoke test green on all platforms.

## M2 - UX shell

Scope: overlay (all states from UX 2), onboarding (UX 6), settings, model manager UI, history, permissions subsystem with live re-checks, error catalog E1-E9 wired end-to-end, hold/toggle/double-tap-lock semantics.

Acceptance criteria:

- [ ] A non-technical tester completes install -> first dictation in < 5 min without help, on each platform.
- [ ] Every UX error state is reachable in a test harness and shows its designed response; zero dead ends.
- [ ] Overlay never takes focus (verified on KDE, the spike 1 regression case) and respects reduced-motion.
- [ ] Accessibility pass: keyboard-only navigation, screen-reader labels, no color-only signaling.

## M3 - Text polish (the differentiator)

Scope: llama.cpp polish engine, prompt/profile system with versioned assets, personal dictionary (UI + deterministic post-pass), per-app profiles, raw-mode modifier, similarity guard, deadline racing, polish benchmark suite in CI, E10.

Acceptance criteria:

- [ ] Blind comparison on the benchmark set: polished preferred >= 80%, zero meaning-altering edits in the accepted set (PRD 7).
- [ ] Deadline misses inject raw with no user-visible failure; measured miss rate < 5% for 10 s utterances on reference hardware.
- [ ] Polish adds <= 700 ms p50 for 10 s utterances on Apple Silicon; hardware-tier defaults applied elsewhere.
- [ ] Prompt changes are benchmark-gated in CI.

## M4 - Packaging, hardening, v1.0

Scope: signing + notarization, Homebrew/winget/Flathub/AppImage channels, Parakeet engine + attribution surfaces, model recommendations by hardware, threat-model doc, docs site/README for end users, latency regression CI.

Acceptance criteria:

- [ ] All PRD section 7 success criteria pass, measured and recorded.
- [ ] Signed installers on all channels; clean-machine installs verified.
- [ ] Crash-free rate > 99.5% over a 2-week dogfood with >= 5 external testers across the three OSes.
- [ ] Security review of the injection IPC surface (trigger verbs only) done.

## Post-v1 (ordered backlog, not scheduled)

1. Streaming transcription (`StreamingTranscriptionEngine` extension trait).
2. Voice commands ("new line", "scratch that", "send it").
3. Plugin system on top of the four traits (design doc first).
4. Additional engines: MLX (Apple), Faster-Whisper sidecar, cloud-optional never.
5. Auto-updater evaluation (Tauri updater) for direct-download installs.
6. Custom vocabulary fine-tuning / boosted decoding.
7. Localization of the UI.
