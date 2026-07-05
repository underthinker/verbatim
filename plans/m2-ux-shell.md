# M2 UX Shell - Scoping Plan

Status: draft.
Goal: close the four M2 acceptance criteria (docs/ROADMAP.md:34-39) - the real UI surfaces on top of the M1 headless walking skeleton.
Each phase is a branch off `main`, PR-gated by CI, executable in a fresh chat context.

## Acceptance criteria (the gate)

1. A non-technical tester completes install -> first dictation in < 5 min without help, on each platform. (onboarding, UX.md 6)
2. Every UX error state (E1-E10) is reachable in a test harness and shows its designed response; zero dead ends. (UX.md 4)
3. Overlay never takes focus (verified on KDE, the spike-1 regression case) and respects reduced-motion. (UX.md 2, 7)
4. Accessibility pass: keyboard-only nav, screen-reader labels, no color-only signaling. (UX.md 8)

## Current state (verified 2026-07-05)

- **Core is UI-ready.** `verbatim-core` publishes a typed event bus (`event.rs`: `SessionTransition`, `InputLevel`, `DownloadProgress`, `PermissionChanged`, `ErrorRaised`) that every surface consumes without querying state (ARCHITECTURE.md 4.9). Session state machine (`session.rs`) is exhaustive incl. `Failed(ErrorId)` for E1-E10. Hotkey hold/toggle/tap-lock semantics (`hotkey.rs`) already implemented and time-injected for testing.
- **No Tauri shell exists yet.** Despite ARCHITECTURE.md 1 naming Verbatim "a Tauri 2 application", `verbatim-app` today is a pure CLI daemon: `main.rs` (clap `daemon`/`trigger`/`status`), `daemon.rs` (`build_deps()` feature-swapped real/fake backends), `ipc.rs`/`transport.rs`/`client.rs` (closed-verb IPC, Unix socket + Windows named pipe). There is no webview, no React frontend, no overlay window, no Tauri command/event bridge.
- **Tray is macOS-only** (`verbatim-platform/src/macos/tray.rs`), a stopgap; ARCHITECTURE.md 4 and UX.md 7 want a cross-platform tray reflecting state.
- **No frontend toolchain.** No `package.json`, no Vite/React, no `src-tauri`. CI is Rust-only (four runners: clippy/test/build + package + latency bench).
- **Error catalog is typed in core but not rendered.** `ErrorId` E1-E10 exist and the runner raises them; nothing maps them to overlay copy + primary action (UX.md 4).

## Sources of truth (do not re-decide)

- UX contract: `docs/UX.md` - states (2), hotkeys (3), error catalog + copy + primary actions (4), polish UX (5), onboarding steps (6), surfaces (7), accessibility (8), open questions (9).
- Architecture: `docs/ARCHITECTURE.md` - Tauri command/event bridge (1, 4.9 line 138: "forwards the bus to the webview event system 1:1; overlay and tray are direct Rust consumers, no webview round-trip on the hot path"), crate roles (2), subsystem contracts (4), happy-path budget (5).
- Overlay latency budget: ARCHITECTURE.md 5 line 143 - overlay visible < 50 ms via direct event (not a webview round-trip).
- Event bus: `crates/verbatim-core/src/event.rs` - the only thing the UI subscribes to. New surfaces consume events; they never poke runner internals (the `walking_skeleton.rs` E2E rule).
- Spike-1 regression: overlay focus-steal broke Handy on KDE (UX.md 2 line 58) - the single highest-risk constraint of this milestone.

## Cross-cutting decisions to make in-branch (surface before coding)

- **Frontend stack**: ARCHITECTURE.md commits TypeScript/React webview. Pin the build tool (Vite), package manager (pnpm), and where `src-tauri`/frontend live in the workspace. New CI toolchain (Node + pnpm) lands with Phase A.
- **Overlay implementation**: a separate Tauri window configured native-side as non-activating + click-through on all three OSes, OR a per-platform native overlay driven by the bus. ARCHITECTURE.md 1 line 9 says "separate native-configured Tauri window". Prove non-activating on KDE first (criterion 3) before building visuals.
- **Hot-path rule**: overlay and tray subscribe to the Rust bus directly; only the settings/onboarding/history/model-manager webviews go through the Tauri command+event bridge. Do not route the overlay through the webview (blows the < 50 ms budget).

## Phase A - branch `m2-phaseA-tauri-shell`

Stand up the Tauri 2 shell around the existing CLI without losing the headless path.

What to implement:

1. Introduce `src-tauri` (or fold into `verbatim-app`) + a frontend workspace (Vite + React + TS, pnpm). Decide layout; keep `verbatim daemon`/`trigger`/`status` subcommands working (GNOME hotkey workaround and E2E depend on the CLI).
2. Tauri command layer: thin handlers that call into `verbatim-core`/runner - every command a headless CLI could also invoke (ARCHITECTURE.md 1 line 27). No engine/platform calls from the webview.
3. Event bridge: forward `verbatim-core` `EventBus` to the Tauri webview event system 1:1 (ARCHITECTURE.md 4.9 line 138). One adapter, typed, tested.
4. Minimal "hello" webview proving the bridge: subscribe to `SessionTransition`, render current state. Throwaway UI, real plumbing.
5. CI: add a Node+pnpm job (frontend build + typecheck + lint); keep the Rust matrix green. Package job builds the Tauri bundle per platform (unsigned) alongside the existing CLI artifact.

Verification: `pnpm build` + `cargo build` green on all runners; hello-webview shows live state transitions driven by `verbatim trigger` over IPC; CLI subcommands unchanged.

Anti-patterns: no business logic in TS; no webview access to engines/platform; do not break the headless daemon path.

## Phase B - branch `m2-phaseB-overlay`

The overlay window - criterion 3, the KDE focus-steal risk. Do this second, before the heavier webviews, because it de-risks the milestone.

What to implement:

1. Overlay as a separate native-configured window: always-on-top, click-through, **non-activating / never takes keyboard focus** on macOS, Windows, and Linux (Wayland GNOME + KDE). Prove the non-activating property on KDE Plasma 6 first (spike-1 regression).
2. Drive it directly from the Rust bus (not the webview command path): `SessionTransition` -> overlay presentation per UX.md 2 table (ARMING shimmer, RECORDING waveform from `InputLevel`, FINALIZING sweep, TRANSCRIBING/POLISHING progress, INJECTING success tick + 200 ms fade). Hit the < 50 ms visible budget.
3. Reduced-motion: respect OS setting - static level meter instead of waveform, no fades (UX.md 8 line 132).
4. State differ by icon/shape, not hue alone (criterion 4 / UX.md 8 line 131).
5. Empty/silent recording -> "didn't catch anything" flash, no dialog (UX.md 2 line 57).

Verification: automated - overlay window property assertions (non-activating, click-through) per platform, gated like the seam tests where a display server is needed. Manual - KDE focus-steal check recorded in the issue (this is also spike-1 sign-off). Reduced-motion toggle honored.

Anti-patterns: overlay must never activate/focus; never route overlay updates through the webview; no color-only state signaling.

## Phase C - branch `m2-phaseC-error-catalog`

Wire E1-E10 end to end with a reachability harness - criterion 2 ("zero dead ends").

What to implement:

1. Map every `ErrorId` (E1-E10) to its overlay/notification presentation: copy pattern + single primary action per UX.md 4 table. One exhaustive mapping, compile-checked against the `ErrorId` enum (no default arm - a new error must force a mapping).
2. Wire primary actions: E1 open mic permission pane (deep link) + live re-check; E2 open model manager preselected; E3 retry-from-saved-audio then backend switch; E4/E7 clipboard fallback + paste hint; E5 secure-field refuse; E6 device picker; E8 resumable download retry; E9 Linux guided typing setup; E10 silent raw fallback + one-time tray notice.
3. Reachability harness: a test surface that can drive the runner into every `Failed(ErrorId)` state and assert the designed response renders with its primary action wired (criterion 2 is literally this harness).

Verification: harness reaches all 10 error states and asserts copy + primary action; no state renders a dead end. E4/E5 already have E2E coverage from M1 (`walking_skeleton.rs`) - extend, don't duplicate.

Anti-patterns: no raw error codes/jargon in copy (UX.md 4 line 73); no error state without exactly one primary action; no unhandled `ErrorId` (exhaustive match).

## Phase D - branch `m2-phaseD-onboarding`

First-run onboarding - criterion 1 (< 5 min install -> first dictation).

What to implement (UX.md 6, one screen each, progress dots):

1. Welcome + inline privacy statement.
2. Microphone permission: OS prompt on click, live state check via `PermissionChanged`, success tick.
3. Typing permission: per-OS - macOS Accessibility deep link + re-check polling; Windows none; Linux guided uinput/portal + "test typing here" box (spike 1/2).
4. Model download: hardware-detected recommended model preselected (RAM/GPU), size/disk shown, background download with `DownloadProgress`, "keep going" once small model lands.
5. Try it: in-app text field, "hold [hotkey] and say anything", success shows transcribed text - validates the whole pipeline inside our window.
6. Polish opt-in: before/after example, optional polish-model download, "skip for now" equally prominent.
7. Re-entry: failed permission later (E1/E9) deep-links to the exact step, not the start.

Verification: scripted walkthrough completes install -> first dictation; manual timed run per platform recorded in the issue (< 5 min, no docs). Permission re-check loops work.

Anti-patterns: no step un-skippable except permissions; no dead-end permission screens; do not require documentation.

## Phase E - branch `m2-phaseE-settings-history-models`

The steady-state webviews (UX.md 7): Settings (tabs: General/Dictation/Polish/Models/History/About), Model manager (download/delete/default, disk usage, E8 resumable), History (raw/polished pairs, copy-raw, diff-on-hover, retention), cross-platform tray reflecting state with the UX.md 7 menu.

What to implement:

1. Settings tabs bound to config (ARCHITECTURE.md 4.8) via Tauri commands; hotkey rebind with conflict check (UX.md 3 line 68); raw/polished toggle; per-app profiles + personal dictionary (user-visible, one-click confirm per UX.md 5.3).
2. Model manager: list/download/delete/default, disk usage, E8 byte-progress resumable retry.
3. History window: reverse-chron raw/polished pairs, copy-raw per entry, hover diff, retention default 7 days incl. "off".
4. Cross-platform tray: state-reflecting icon (idle/recording/processing/error) + menu (pause, raw/polished, input device, last 5 dictations, Settings, Quit). Ungate `tray.rs` from macOS-only.

Verification: each surface drives real config/model/history via commands; tray state matches bus events on all platforms; personal-dictionary confirm flow works.

Anti-patterns: no black-box dictionary (UX.md 5.3); notifications only for background-download-done and E10 (UX.md 7 line 125), never routine success.

## Phase F - branch `m2-phaseF-accessibility`

Accessibility pass across all surfaces - criterion 4.

What to implement (UX.md 8): full keyboard nav + visible focus rings on every window; screen-reader labels on every control; overlay state changes announced via OS a11y notification API (default on when a screen reader is detected); no color-only signaling (verify across overlay + tray + error states); high-contrast honored; auto-dismiss timeouts extended when assistive tech is active.

Verification: keyboard-only walkthrough of every window; screen-reader label audit (VoiceOver / Narrator / Orca); color-only audit; reduced-motion + high-contrast honored.

Anti-patterns: no control without a label; no information by color alone; no focus trap / dead keyboard path.

## Final phase - acceptance sign-off

1. Re-check ROADMAP.md:34-39 with evidence (harness runs, timed onboarding, KDE focus check, a11y audit); tick in a closing PR.
2. Guards: exhaustive `ErrorId` match (no default arm); overlay non-activating asserted per platform; grep guards from M1 still clean.
3. Cross-platform manual pass (macOS, Windows 11, GNOME Wayland, KDE) recorded in the PR.

Dependency order: A first (everything needs the shell). B and C parallel after A (both consume the bus; B de-risks KDE early). D after C (onboarding reuses E1/E2/E9 wiring). E after A (independent of B/C/D, can run parallel). F last (audits all surfaces once they exist).

## Open questions to resolve at kickoff (UX.md 9)

- Near-cursor overlay placement: Wayland restricts global cursor position - ship display-edge first, defer near-cursor.
- History default on/off for the privacy-first audience: proposal on with 7-day retention (decide before Phase E).
- Frontend stack pinning (Vite/pnpm layout) and whether `src-tauri` is a new crate or folded into `verbatim-app` (decide in Phase A).
