# Verbatim - Implementation Roadmap

Status: draft for sign-off.
Milestones are sequential; each has acceptance criteria that gate the next.
"All platforms" always means macOS (ARM + Intel), Windows 11 x64, Ubuntu 24.04 (X11 + Wayland GNOME/KDE).

## M0 - Spikes and design sign-off (done except hardware validation)

Deliverables: the five docs in `docs/`, spike findings 1-4 in `spikes/`.

Remaining acceptance criteria:

- [ ] Spike 1 re-run as running code on real Linux (GNOME Wayland, Plasma 6, Hyprland): inject into GTK/Qt/Electron/terminal targets via libei and uinput; confirm `restore_token` persistence.
- [ ] Spike 2 validated with a signed dev build: paste + typing injection into Terminal, iTerm2, VS Code, Safari, Slack, and a password field.
- [ ] Spike 3/4 re-measured on a mid-range Windows x64 laptop (CPU-only); polish default per hardware tier decided. (2026-07-06: desktop-class Windows figures recorded under M1 criterion 4 via PR #30 - Ryzen 7 5800X / RX 5700 XT: Vulkan p50 2088.6 ms, CPU 16-thread 7.1 s, CPU default 18.0 s; mid-range laptop tier still unmeasured.)
- [x] Tristen signs off on PRD (differentiation thesis, success criteria), name, and license. (2026-07-02: polish headline, Tauri 2 + Rust, all 3 platforms day one, hardware validation during M1, name Verbatim, MIT incl. branding.)

## M1 - Walking skeleton (ugly but real, all platforms)

Scope: hotkey -> record -> whisper.cpp -> inject raw text, tray icon with quit, `verbatim daemon`/`trigger`/`status` CLI, SessionRunner actor with fake-pipeline orchestration, trigger IPC protocol (closed verb set on a Unix domain socket), fake-capture test seam, CI matrix from the first commit.

Acceptance criteria (open items tracked in the [M1 - Walking skeleton milestone](https://github.com/underthinker/verbatim/milestone/1)):

- [x] Fresh checkout builds and packages unsigned dev artifacts on all platforms in CI. (2026-07-04: Phase 8, PR #10 - `Package unsigned dev artifact` jobs green on macos-latest, macos-15-intel, ubuntu-24.04, windows-latest.)
- [ ] Dictation lands text in a foreign app on all platforms, including GNOME Wayland (portal or uinput) and KDE. (2026-07-04: #18 - per-platform seam tests (`macos_seams`/`windows_seams`/`linux_seams`) assert the probe chain always tries a real backend before the single `ClipboardOnly` last resort and never prompts, running on each CI runner; macOS seam E2E (real NSPasteboard transient/restore + frontmost focus) verified on Apple M5. The real-keypress checks (macOS foreground app, Windows incl. UIPI, GNOME Wayland portal + `restore_token` silent reconnect, KDE Plasma 6) are desktop/permission-bound and tracked as a manual checklist in [docs/M1_INJECTION_VERIFICATION.md](M1_INJECTION_VERIFICATION.md).)
- [x] Injection failure is detected honestly (no silent success) and falls back to clipboard. (2026-07-04: #19 - every platform injector (macOS/Windows/Linux) walks its backend chain ending in `ClipboardOnly` with an honest `verified` receipt; unverified delivery surfaces E4 with the text staged on the clipboard, secure fields refuse with E5. E2E smoke covers the failure -> clipboard-fallback path (Windows UIPI / Wayland denial shape) and the secure-field refusal.)
- [ ] p50 raw latency < 800 ms for a 10 s utterance on Apple Silicon and the reference Windows laptop (resident model). (2026-07-04: raw-latency bench harness + per-runner regression gate landed in CI on `main` - PRs #11/#13/#14. CI runs on virtualised runners only, which cannot meet the 800 ms budget by design. Apple Silicon measured on real hardware (Apple M5, resident `ggml-base.en`, 11.0 s JFK fixture, 20 iterations): **p50 105.1 ms, p95 105.7 ms** - 7.6x inside the 800 ms budget. 2026-07-06: Windows desktop measured on real hardware (AMD Ryzen 7 5800X, RX 5700 XT, 32 GB RAM, resident `ggml-base.en`, 10 s fixture - PR #30 validation): Vulkan GPU **p50 2088.6 ms / p95 2289.0 ms**, CPU 16-thread 7.1 s, CPU default 18.0 s - all outside the 800 ms budget. Reference Windows measurement still open - tracked in #16.)
- [x] State machine has exhaustive unit tests; E2E smoke test green on all platforms. (2026-07-04: #20 - `session.rs` exhaustive transition table + terminal-Failed tests; `walking_skeleton.rs` E2E smoke (happy/injection-fail/cancel/polished/polish-degrades-to-raw/toggle) runs via `cargo test --workspace` on macos-latest, macos-15-intel, windows-latest, ubuntu-24.04.)

## M2 - UX shell

Scope: overlay (all states from UX 2), onboarding (UX 6), settings, model manager UI, history, permissions subsystem with live re-checks, error catalog E1-E9 wired end-to-end, hold/toggle/double-tap-lock semantics.

Acceptance criteria:

- [ ] A non-technical tester completes install -> first dictation in < 5 min without help, on each platform.
- [x] Every UX error state is reachable in a test harness and shows its designed response; zero dead ends.
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
