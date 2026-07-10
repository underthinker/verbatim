# M1 Completion Plan - Remaining Phase Branches

Status: **all phases shipped; the milestone is code-complete.**
Goal was to close the five M1 acceptance criteria (docs/ROADMAP.md, M1 section).
Each phase was a branch off `main`, PR-gated by CI.
The plan below is kept as the record of what was built and why; ROADMAP.md is the live status.

Outcome (updated 2026-07-09):

- Phases 6-10 all merged: Linux injection, Windows injection + IPC, CI packaging, latency bench, E2E smoke.
- Three of the five criteria are ticked outright: unsigned dev artifacts on all four runners, honest failure detection with clipboard fallback, exhaustive state-machine tests plus green cross-platform E2E smoke.
- Two criteria stay open on hardware and desktop access rather than on code:
  - Real-keypress injection is verified and machine-checked on macOS (2026-07-09, Apple M5); Windows UIPI, GNOME Wayland portal + `restore_token`, and KDE Plasma 6 remain. Checklist and evidence: `docs/M1_INJECTION_VERIFICATION.md`.
  - Raw p50 latency is 105.1 ms on Apple M5, far inside the 800 ms budget; the reference Windows laptop measurement is still unmade (desktop-class Windows figures are recorded in ROADMAP.md M0/M1).

Current state at authoring time (verified 2026-07-03, now historical):

- macOS vertical slice done: injection (`mac-inject`), hotkey/tray (`global-hotkey`), whisper.cpp, clipboard fallback with honest E4 receipts.
- Linux backend is a 7-line doc stub (`crates/verbatim-platform/src/linux/mod.rs`).
- Windows backend is a 6-line doc stub (`crates/verbatim-platform/src/windows.rs`); IPC is `#[cfg(unix)]` only.
- CI builds and tests on all four runners but uploads no artifacts, runs no E2E, no benchmarks.
- State machine already has an exhaustive transition-table test (`session.rs:224`); no new work needed there.

## Allowed APIs / sources of truth

- Platform traits: `crates/verbatim-platform/src/traits.rs` - `HotkeyManager::register/unregister`, `AudioCapture::start/stop/abort/is_capturing`, `TextInjector::probe/inject`, `ClipboardGuard::snapshot/set_transient_text/restore_if_unchanged`, `PermissionProbe::probe`, `FocusTracker::focused_app`.
  New backends implement these traits exactly; do not add methods.
- Reference implementation to copy structure from: `crates/verbatim-platform/src/macos/` (module-per-concern, feature-gated in `mod.rs`, probe-ordered backend list, honest `InjectionReceipt { verified }`).
- Feature-flag pattern to copy: `verbatim-platform/Cargo.toml` (`mac-inject`, `global-hotkey`) and `verbatim-app/Cargo.toml` (`real-injection`, `global-hotkey`), plus cfg-gating pattern in `verbatim-platform/src/lib.rs:11-25`.
- Daemon wiring pattern to copy: `crates/verbatim-app/src/daemon.rs` `build_deps()` (feature-swapped real backends, graceful degrade to fake).
- Linux mechanism decisions: `spikes/01-wayland-injection-hotkeys.md` (chain at line 36, crates at line 37, hotkeys at lines 30-38).
- Windows mechanism decisions: `crates/verbatim-platform/src/windows.rs` doc stub (RegisterHotKey + SendInput unicode -> clipboard+Ctrl-V, UIPI-aware E4).
- Latency methodology: `spikes/03-latency-budget.md`; CI expectations: `docs/ENGINEERING.md:62-92`.
- Anti-patterns (global): never shell out to ydotool/wtype; never trust exit codes as injection success; no `unwrap`/`expect` (workspace clippy denies); no network-capable crates outside the model downloader (`cargo deny` ban-list); keep all real backends feature-gated so default build stays fake-only.

## Phase 6 - branch `m1-phase6-linux-injection`

Linux TextInjector + ClipboardGuard + FocusTracker + hotkey backends.

What to implement:

1. Add feature `linux-inject` to `verbatim-platform` pulling `reis` (libei), `evdev` (uinput), the XDG portal client crate (evaluate `ashpd` - spike names the portal abstractly; confirm crate choice against portal docs before adding), and a Wayland clipboard crate.
2. Implement injection chain in probe order per spike 1 line 36: RemoteDesktop portal + libei -> uinput -> wlroots `virtual-keyboard-unstable-v1` -> clipboard-assisted paste -> clipboard-only.
   Copy the probe/fallback-loop/`last_error`/`AllBackendsFailed` structure from `macos/inject.rs:155-205`.
3. Persist and reuse portal `restore_token` for silent reconnect (spike 1 lines 15-18).
4. Clipboard-only path returns `verified: false` so runner raises E4, same as `macos/clipboard.rs:43`.
5. Hotkeys: GlobalShortcuts portal backend; keep `verbatim trigger` IPC as documented GNOME fallback; opt-in evdev listener for hold-to-talk (spike 1 lines 30-38).
6. Wire into `build_deps()` in `daemon.rs` behind `real-injection` on `target_os = "linux"`, mirroring the macOS arm.
7. Extend `verbatim-app` non-macOS hotkey path so the daemon can serve with the portal hotkey backend.

Verification:

- `cargo clippy --workspace --all-targets --all-features -D warnings` on ubuntu-24.04.
- Integration tests gated on an E2E env var, copying the pattern in `crates/verbatim-platform/tests/macos_seams.rs` (graceful skip when portals absent, e.g. headless CI).
- Manual acceptance on real desktops: GNOME Wayland and Plasma 6 - dictate into GTK, Qt, Electron, and a terminal target; confirm `restore_token` persistence across daemon restarts (this is also the open M0 spike-1 criterion).

Anti-pattern guards: no ydotool/wtype subprocesses; no X11-only paths sold as Wayland support; do not report `verified: true` from a backend that cannot observe delivery.

## Phase 7 - branch `m1-phase7-windows`

Windows TextInjector + ClipboardGuard + FocusTracker + hotkey + IPC.

What to implement:

1. Add feature `win-inject` to `verbatim-platform` with the `windows` crate (official Microsoft bindings).
2. Injection per the `windows.rs` stub: `SendInput` `KEYEVENTF_UNICODE` typing -> clipboard + Ctrl-V paste -> clipboard-only; map UIPI denial (elevated target window) to honest failure -> E4.
3. Hotkey: `RegisterHotKey` backend plus low-level keyboard hook for bare-modifier hold, mirroring the macOS split between `hotkey.rs` and `modifier_tap.rs`.
4. Tray: extend `tray.rs` gating - `tray-icon` crate already supports Windows; ungate from macOS-only where safe.
5. Port IPC: `client`/`daemon`/`ipc` modules in `verbatim-app` are `#[cfg(unix)]`; add a Windows transport (named pipe, or Unix-domain socket which Windows 10+ supports via `tokio::net` - decide in-branch, prefer whichever keeps the closed-verb protocol byte-identical).
6. Wire into `build_deps()` on `target_os = "windows"`.

Verification:

- CI already runs windows-latest with `--all-features`; must stay green (remember the phase-5 lesson: gate platform-specific raw-pointer types so cross-platform `--all-features` builds compile).
- Daemon round-trip tests (`trigger_round_trip_drives_the_session`) run on Windows once IPC lands.
- Manual acceptance on Windows 11 x64: dictate into Notepad, VS Code, a browser, a terminal; verify elevated-window injection fails honestly to clipboard.

Anti-pattern guards: no `winapi` crate (use `windows`/`windows-sys`); no simulated Ctrl-V without clipboard snapshot/restore via `ClipboardGuard`.

## Phase 8 - branch `m1-phase8-ci-packaging`

Unsigned dev artifacts from CI (acceptance criterion 1).

What to implement:

1. Add a `package` job to `.github/workflows/ci.yml` after `test`: matrix over the same four runners, `cargo build --release --locked` with each platform's real-feature set, then `actions/upload-artifact` of the `verbatim` binary (zip/tarball per OS, named `verbatim-dev-{os}-{arch}`).
2. No signing/notarization - that is the M4-era release pipeline (ENGINEERING.md:92-98); M1 wants unsigned dev artifacts only.
3. Tauri bundling is out of scope until the Tauri shell exists; a packaged CLI binary satisfies "unsigned dev artifacts" for the walking skeleton.

Verification:

- Fresh-checkout CI run on a PR produces downloadable artifacts for macOS ARM, macOS Intel, Windows x64, Ubuntu.
- Downloaded macOS artifact runs `verbatim status` against a live daemon.

Anti-pattern guards: do not add signing secrets; do not build with `--all-features` for artifacts (feature sets are per-platform).

## Phase 9 - branch `m1-phase9-latency-bench`

p50 raw latency < 800 ms for a 10 s utterance (acceptance criterion 4).

What to implement:

1. Create `benches/` harness (dir already planned, ENGINEERING.md:26): feed a recorded (not synthesized - spike 3 line 25 caveat) 10 s 16 kHz mono WAV through `WhisperCppEngine` with a resident model; measure stop-to-text wall time; report p50/p95 over N>=20 iterations.
2. Add fixture audio: real recorded speech clip committed under `benches/fixtures/` (small, license-clean - record one).
3. CI job on main pipeline: run bench on macos-latest (Apple Silicon) and windows-latest; compare to per-runner baseline JSON; fail on >20% regression (ENGINEERING.md:65); assert p50 < 800 ms on Apple Silicon.
4. Reference-Windows-laptop measurement is hardware-bound: document a `cargo bench`-style one-shot command so Tristen can run it on the reference laptop; record the result in ROADMAP.md checkbox.

Verification:

- Bench runs locally on Apple Silicon and prints p50/p95; p50 < 800 ms with base.en or small.en resident.
- CI bench job green with baseline committed.

Anti-pattern guards: never include model download/load in the p50 timing (resident-model criterion); no synthesized `say` audio as the graded fixture.

## Phase 10 - branch `m1-phase10-e2e-smoke`

E2E smoke green on all platforms (acceptance criterion 5; exhaustive state-machine tests already exist - `session.rs:224` covers every state x input pair, no work needed).

What to implement:

1. E2E smoke per ENGINEERING.md:64: launch the daemon binary, drive it via `verbatim trigger` IPC, fixture audio through fake `AudioCapture`, real (or fake on headless CI) injector, assert injected text observed.
   No Tauri shell yet, so drive the CLI daemon directly instead of tauri-driver; note the deviation in the test header.
2. Injection target on CI: assert through the fake injector's recorded calls on headless runners; run real-injection variant behind the existing E2E env-var gate for local machines.
3. Wire the smoke test into CI for all four runners (Windows depends on phase 7 IPC).

Verification:

- `cargo test` E2E smoke green on all four CI runners.
- Local run with real features on macOS + Linux injects into a scratch window.

Anti-pattern guards: smoke test must go through the event bus and IPC like a real surface (see `walking_skeleton.rs` header), not poke runner internals.

## Final phase - acceptance sign-off

1. Re-check ROADMAP.md:22-28 boxes one by one with evidence (CI links, bench numbers, manual test notes); tick them in a closing PR.
2. Grep guards: `grep -rn "ydotool\|wtype" crates` empty; `cargo deny check` clean; clippy `--all-features` clean on all runners.
3. Manual cross-platform dictation pass (macOS, Windows 11, GNOME Wayland, KDE) recorded in the PR description.

Dependency order: 6 and 7 parallel; 8 anytime; 9 anytime (Windows leg after 7 if bench uses daemon path, else independent); 10 after 6+7 for full-matrix green.
