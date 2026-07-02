# Verbatim

A cross-platform, privacy-first, fully local dictation application.
Press a hotkey, speak, and get polished text in any app - with zero cloud dependency.

**Status: design phase.**
No production code exists yet.
The project is currently specified in the documents below; implementation follows the roadmap.

## Design documents

| Document | Purpose |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Product requirements: vision, goals, non-goals, competitive analysis, success criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture: layers, subsystems, interfaces, data flow |
| [docs/UX.md](docs/UX.md) | UX specification: state machine, error handling, onboarding, accessibility |
| [docs/ENGINEERING.md](docs/ENGINEERING.md) | Engineering specification: repo layout, testing, CI/CD, packaging, releases |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Implementation roadmap: milestones, acceptance criteria |

## Risk spikes

`spikes/` contains throwaway prototypes and findings notes for the highest-risk subsystems (Wayland text injection, macOS permissions, transcription latency, local LLM text polish).
Spike code is not production code and carries no quality bar.

## License

MIT, including the Verbatim name and branding: forks may ship as-is.
See [LICENSE](LICENSE).
