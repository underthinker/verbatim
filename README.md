# Verbatim

A cross-platform, privacy-first, fully local dictation application.
Press a hotkey, speak, and get polished text in any app - with zero cloud dependency.

**Status: early implementation (milestone M1).**
The walking-skeleton workspace has a working `SessionRunner` actor that drives the dictation pipeline through deterministic fakes, a Unix-only daemon with IPC trigger protocol, and the session state machine + platform/engine layer traits.
Real capture, transcription, injection, and the Tauri shell land during later M1 phases; see [docs/ROADMAP.md](docs/ROADMAP.md) for milestone status.
The design documents below remain the source of truth.

## Design documents

| Document | Purpose |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Product requirements: vision, goals, non-goals, competitive analysis, success criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture: layers, subsystems, interfaces, data flow |
| [docs/UX.md](docs/UX.md) | UX specification: state machine, error handling, onboarding, accessibility |
| [docs/ENGINEERING.md](docs/ENGINEERING.md) | Engineering specification: repo layout, testing, CI/CD, packaging, releases |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Implementation roadmap: milestones, acceptance criteria |

## Workspace

Cargo workspace (Rust stable, edition 2024, MSRV 1.88 pinned via `rust-toolchain.toml`):

| Crate | Layer | Contents |
|---|---|---|
| `verbatim-core` | Core | session state machine, event bus, error taxonomy, session runner actor |
| `verbatim-platform` | Platform | hotkey/audio/injection/clipboard/permission/focus/autostart traits, deterministic fakes, per-OS stubs, optional real cpal mic capture (`cpal-audio` feature) |
| `verbatim-engines` | Engine | transcription + polish traits, engine registry, fakes |
| `verbatim-app` | App | the `verbatim` binary (`daemon`/`trigger`/`status` CLI; Tauri shell joins later in M1) |

```
cargo build --locked      # build all crates
cargo test  --locked      # unit + walking-skeleton integration tests
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the layer model and [docs/ENGINEERING.md](docs/ENGINEERING.md) for the full repo layout.

## Risk spikes

`spikes/` contains throwaway prototypes and findings notes for the highest-risk subsystems (Wayland text injection, macOS permissions, transcription latency, local LLM text polish).
Spike code is not production code and carries no quality bar.

## License

MIT, including the Verbatim name and branding: forks may ship as-is.
See [LICENSE](LICENSE).
