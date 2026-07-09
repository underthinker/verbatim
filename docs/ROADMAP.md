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
- [ ] Dictation lands text in a foreign app on all platforms, including GNOME Wayland (portal or uinput) and KDE. (2026-07-04: #18 - per-platform seam tests (`macos_seams`/`windows_seams`/`linux_seams`) assert the probe chain always tries a real backend before the single `ClipboardOnly` last resort and never prompts, running on each CI runner; macOS seam E2E (real NSPasteboard transient/restore + frontmost focus) verified on Apple M5. The real-keypress checks (macOS foreground app, Windows incl. UIPI, GNOME Wayland portal + `restore_token` silent reconnect, KDE Plasma 6) are desktop/permission-bound and tracked as a manual checklist in [docs/M1_INJECTION_VERIFICATION.md](M1_INJECTION_VERIFICATION.md).
  2026-07-09: **macOS closed.** All three manual checks verified on Apple M5 with Accessibility granted: the sentinel lands in TextEdit via `TransientPasteboardPaste` with the prior clipboard restored, revoking Accessibility degrades honestly to clipboard-only (E4, `verified=false`, nothing typed), and a real push-to-talk dictation through `ggml-base.en` reaches `Idle`, which the runner allows only on a verified receipt. Boxes 2 and 3 are now machine-checked by `VERBATIM_TEXTEDIT_E2E=1 scripts/verify-injection.sh`, which reads the target document back rather than trusting the receipt - and which caught a paste/restore race that fed a cold target the user's previous clipboard content under a `verified=true` receipt. Windows and Linux real-keypress checks remain open.)
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
  (2026-07-09: markup pass done. Tablist takes Home/End and its panel is focusable, so the control-free Dictation panel is reachable; onboarding moves focus to the new step's heading; history rows name raw vs polished off-screen rather than by rule-and-hue alone; the copy confirmation, the try-it state word, and the interrupted download reach a live region. Forced-colors restores the step dots, the download bar, and the selected tab, all of which lost their only signal when the accent background flattened. `eslint-plugin-jsx-a11y` (strict) now runs in the `pnpm lint` CI job as the standing guard.
  2026-07-09: `AccessibilityAnnouncer` now has all three backends, so overlay state reaches Narrator and Orca as well as VoiceOver - the overlay window is non-activating, so its `aria-live` region alone could never carry it.
  Windows detects a screen reader through `SPI_GETSCREENREADER` and speaks by raising a UIA notification against the host provider of the overlay's `HWND`.
  Linux watches `org.a11y.Status.ScreenReaderEnabled` on a worker thread, so toggling Orca mid-session is picked up without a restart, and speaks through a transient desktop notification: an AT-SPI `Announcement` is the right shape but only routes from a source registered with the a11y registry, which Verbatim has no accessible tree to provide.
  Registering a minimal AT-SPI application root is the recorded upgrade path.
  One gap keeps this open: the pass has not been driven by hand under VoiceOver, Narrator, or Orca, and the announcement leg of each backend can only be confirmed with the screen reader actually running.)

## M3 - Text polish (the differentiator)

Scope: llama.cpp polish engine, prompt/profile system with versioned assets, personal dictionary (UI + deterministic post-pass), per-app profiles, raw-mode modifier, similarity guard, deadline racing, polish benchmark suite in CI, E10.

Acceptance criteria:

- [ ] Blind comparison on the benchmark set: polished preferred >= 80%, zero meaning-altering edits in the accepted set (PRD 7).
  Meaning-preservation half closed: 10/10 within the similarity guard, 0 drift (polish-quality bench, Apple M5, 2026-07-08).
  The >= 80% preference half needs a real dictation corpus + blind panel; it rides the M4 dogfood, which re-gates all of PRD 7 with >= 5 external testers.
- [x] Deadline misses inject raw with no user-visible failure; measured miss rate < 5% for 10 s utterances on reference hardware.
  0.0% (0/50) at the calibrated 378 ms deadline (Apple M5); calibration scales the deadline off per-machine ms/token so the rate holds across tiers. E10 degrades silently to raw (runner.rs `run_polish`), one-time tray notice only on true engine error.
- [x] Polish adds <= 700 ms p50 for 10 s utterances on Apple Silicon; hardware-tier defaults applied elsewhere.
  p50 148 ms, p95 238 ms (polish-quality bench, Apple M5, Qwen2.5-0.5B q4_k_m); hardware-tier deadline via `calibration::deadline_from_ms_per_token`.
- [x] Prompt changes are benchmark-gated in CI.
  Polish-quality bench runs on every push (`.github/workflows/ci.yml` polish-bench job), failing on a similarity-guard breach, > 20% latency regression, or > 5% deadline-miss rate.

## M4 - Packaging, hardening, v1.0

Scope: the signing + notarization pipeline (wired, certificates deferred), Homebrew/winget/Flathub/AppImage channels, Parakeet engine + attribution surfaces, model recommendations by hardware, threat-model doc, docs site/README for end users, latency regression CI.

Acceptance criteria:

- [ ] All PRD section 7 success criteria pass, measured and recorded.
- [ ] Unsigned installers on all channels, with the one-time OS bypass documented; clean-machine install verified on each OS.
  (2026-07-09: code signing deferred, not dropped. Verbatim ships to a small known audience, where an Apple Developer ID and a Windows Authenticode OV certificate buy only dialog suppression. `release.yml` keeps both signing paths wired behind absent secrets and degrades loud, so this reverses by adding secrets, never by editing code. Accepted cost: macOS keys TCC grants to the ad-hoc code-signing hash, so Microphone and Accessibility must be re-granted on every update - see [ENGINEERING.md](ENGINEERING.md) section 7.)
- [ ] Crash-free rate > 99.5% over a 2-week dogfood with >= 5 external testers across the three OSes.
- [x] Security review of the injection IPC surface (trigger verbs only) done.
  (2026-07-08, M4 Phase D: [docs/THREAT_MODEL.md](THREAT_MODEL.md) plus IPC hardening - F1/F2 fixed with regression tests, F3/F4 dispositioned, wire-protocol fuzz corpus committed.)

## Post-v1 (ordered backlog, not scheduled)

1. Streaming transcription (`StreamingTranscriptionEngine` extension trait).
2. Voice commands ("new line", "scratch that", "send it").
3. Plugin system on top of the four traits (design doc first).
4. Additional engines: MLX (Apple), Faster-Whisper sidecar, cloud-optional never.
5. Auto-updater evaluation (Tauri updater) for direct-download installs.
6. Custom vocabulary fine-tuning / boosted decoding.
7. Localization of the UI.
