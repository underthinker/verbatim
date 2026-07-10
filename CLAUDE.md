# Verbatim - agent guide

Cross-platform, privacy-first, fully local dictation app.
Press a hotkey, speak, get polished text injected into any app, with zero cloud dependency.
Status: M4 (packaging, hardening, v1.0) is the active milestone; phases A-F have all landed.
M1-M3 are code-complete: what remains of them is hardware- and desktop-bound verification, not implementation.
The open criteria across every milestone live in `docs/ROADMAP.md` and reduce to three things: real-keypress injection checks on Windows and Linux, a mid-range-laptop latency measurement, and the two-week external dogfood that re-gates PRD section 7.

## Model selection

Suggest a model and effort level at the start of each task, for cost efficiency.
When you suggest one, also ask the user whether they want to swap to that model/effort level (they drive the switch; do not assume).
Two-tier only (do not use Sonnet):

- **Haiku 4.5** - mechanical, bounded work: renames, typo fixes, formatting, single-file edits, status/CI checks, PR babysitting.
- **Opus 4.8** - anything with design judgment: cross-crate work, debugging, security-sensitive code, feature implementation.

Pick effort (low/med/high) based on ambiguity and blast radius.

### Pre-merge gate (lean, no LLM review pass)

Before shipping, run the local checks: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo deny check`. Auto-fix any issues found. Squash all style/fmt/clippy fixups into the feature commit — no standalone style fixup commits on main.

### Ship flow delegation

Once code is clean, delegate **only** to Haiku: push branch, open PR, stop. No CI babysitting, no merge. Haiku returns the PR URL and nothing else. Opus does not push/PR. You review CI and squash-merge manually.

## Source of truth

Design docs in `docs/` are authoritative. Read them before making design decisions.

| Doc | Purpose |
|---|---|
| `docs/PRD.md` | Product requirements: vision, goals, non-goals, success criteria |
| `docs/ARCHITECTURE.md` | Layers, subsystems, interfaces, data flow |
| `docs/UX.md` | State machine, error handling (E1-E10), onboarding, accessibility |
| `docs/ENGINEERING.md` | Repo layout, tech choices, testing, CI/CD, packaging, security |
| `docs/ROADMAP.md` | Milestones, acceptance criteria, current status |
| `docs/THREAT_MODEL.md` | Assets, trust boundaries, injection-misuse analysis, IPC review |
| `docs/M1_INJECTION_VERIFICATION.md` | Per-platform injection checklist: what CI proves, what a human must |
| `docs/DOGFOOD_REPORT_TEMPLATE.md` | What an external tester submits (M4 crash-free + preference gates) |

End-user docs are an Astro Starlight site in `docs/site/`, deployed to GitHub Pages by `.github/workflows/docs.yml`.
Milestone plans live in `plans/` (`m1-completion.md`, `m2-ux-shell.md`, `m3-polish.md`, `m4-packaging-hardening.md`), each carrying its own status header.
Hardware-bound verification runs from `scripts/` (`verify-injection`, `verify-latency`, `verify-overlay`) - see `scripts/README.md`.

## Workspace

Cargo workspace, Rust stable, edition 2024, MSRV 1.88 (`rust-toolchain.toml`).

| Crate | Layer | Contents |
|---|---|---|
| `verbatim-core` | Core | session state machine, event bus, error taxonomy, `SessionRunner` actor |
| `verbatim-platform` | Platform | hotkey/audio/injection/clipboard/permission/focus/autostart traits, deterministic fakes, per-OS backends |
| `verbatim-engines` | Engine | transcription (whisper.cpp, sherpa-onnx/Parakeet) + polish (llama.cpp) traits, engine registry, fakes |
| `verbatim-app` | App | `verbatim` binary (`daemon`/`gui`/`trigger`/`status`/`stats`/`inject-selftest` CLI), Tauri 2 shell (`gui.rs`), webview event bridge (`bridge.rs`), `tauri.conf.json` |

Frontend lives in `ui/` (Vite + React + TS, pnpm): the webview surfaces.
It subscribes to the bridged event bus (`ui/src/events.ts` mirrors `bridge.rs`); no business logic in TS.

## Build & test

```
cargo build --locked      # all crates
cargo test  --locked      # unit + walking-skeleton integration tests
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo deny check          # licenses + advisories
```

Frontend (from `ui/`): `pnpm install`, `pnpm build` (typecheck + bundle), `pnpm lint`.
Debug `verbatim gui` loads the Vite dev server (`pnpm dev`, port 1420); release bundles embed `ui/dist` via `ui/node_modules/.bin/tauri build` run from the repo root.

Feature flags gate every real backend behind its trait, so the default build is fake-only and tests stay OS-free.
`verbatim-app` exposes the ones you build with: `real-audio` (cpal mic), `real-transcription` (whisper.cpp), `real-injection` (the `mac-inject`/`win-inject`/`linux-inject` seams for the target OS), and `global-hotkey`.
Underneath, `verbatim-engines` carries `whisper-cpp`, `sherpa-onnx`, and `llama-cpp`, each with a `-vulkan` GPU variant on non-macOS targets (Metal is compiled in on macOS).

## Coding standards (from ENGINEERING.md section 3)

- No `unwrap`/`expect` outside tests and startup. Enforced by workspace clippy lint (`unwrap_used`/`expect_used` = deny).
- No `unsafe` outside `verbatim-platform` and engine FFI; every `unsafe` block needs a `// SAFETY:` justification.
- Platform trait impls must not leak OS types across the trait boundary.
- Error handling: `thiserror` per crate; one top-level taxonomy mapping to UX error IDs E1-E10.
- Conventional Commits; PRs small and single-purpose.
- Markdown docs: one sentence per line.
- CHANGELOG is auto-generated - never hand-edit.

## Security posture (non-negotiable)

- No network code outside the hash-verified model downloader and channel update mechanics. `cargo deny` bans http-client crates elsewhere.
- The daemon Unix socket is owner-only (`0o600`). The IPC wire protocol (`ipc.rs`) accepts only a closed set of verbs: `start`/`stop`/`toggle` + `status`. Any other payload is rejected before interpretation, never treated as text to inject. `Cancel` is deliberately absent (ESC discard is local only).
- All user data stays in the platform data dir; temp audio is deleted after transcription and on startup sweep.

## Conventions

- Commits use `Underthinker` as author name (human and agent). Agent sets `GIT_AUTHOR_NAME=Underthinker GIT_COMMITTER_NAME=Underthinker` for every commit.
- Injection backends report honest receipts (real delivery ordered before fallback); seam tests assert ordering. See `docs/M1_INJECTION_VERIFICATION.md`.
- `spikes/` holds throwaway prototypes - no quality bar, not production code.
- Data locations differ per OS (config/models/history/logs) - see ENGINEERING.md section 5.2.
