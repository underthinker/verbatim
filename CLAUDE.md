# Verbatim - agent guide

Cross-platform, privacy-first, fully local dictation app.
Press a hotkey, speak, get polished text injected into any app, with zero cloud dependency.
Status: early implementation, milestone M1 (walking skeleton).

## Source of truth

Design docs in `docs/` are authoritative. Read them before making design decisions.

| Doc | Purpose |
|---|---|
| `docs/PRD.md` | Product requirements: vision, goals, non-goals, success criteria |
| `docs/ARCHITECTURE.md` | Layers, subsystems, interfaces, data flow |
| `docs/UX.md` | State machine, error handling (E1-E10), onboarding, accessibility |
| `docs/ENGINEERING.md` | Repo layout, tech choices, testing, CI/CD, packaging, security |
| `docs/ROADMAP.md` | Milestones, acceptance criteria, current status |

Milestone plans live in `plans/` (`m1-completion.md`, `m2-ux-shell.md`).

## Workspace

Cargo workspace, Rust stable, edition 2024, MSRV 1.88 (`rust-toolchain.toml`).

| Crate | Layer | Contents |
|---|---|---|
| `verbatim-core` | Core | session state machine, event bus, error taxonomy, `SessionRunner` actor |
| `verbatim-platform` | Platform | hotkey/audio/injection/clipboard/permission/focus/autostart traits, deterministic fakes, per-OS backends |
| `verbatim-engines` | Engine | transcription + polish traits, engine registry, fakes |
| `verbatim-app` | App | `verbatim` binary (`daemon`/`trigger`/`status` CLI; Tauri shell lands in M2) |

## Build & test

```
cargo build --locked      # all crates
cargo test  --locked      # unit + walking-skeleton integration tests
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo deny check          # licenses + advisories
```

Feature flags gate real backends behind traits: `cpal-audio` (mic capture), `real-injection`
(+ `win-inject` on Windows). Default build uses deterministic fakes so tests stay OS-free.

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

- Injection backends report honest receipts (real delivery ordered before fallback); seam tests assert ordering. See `docs/M1_INJECTION_VERIFICATION.md`.
- `spikes/` holds throwaway prototypes - no quality bar, not production code.
- Data locations differ per OS (config/models/history/logs) - see ENGINEERING.md section 5.2.
