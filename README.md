# Verbatim

**Local-first dictation with on-device polish.**
Press a hotkey, speak, and get clean, polished text in any app - with zero cloud dependency.

Verbatim transcribes your voice and types it into whatever app has focus - your editor, browser, or chat window.
Everything runs on your own machine: your speech is transcribed, polished, and typed locally, and nothing ever leaves your computer.
No telemetry, no account, no subscription.

## Install

| Platform | Recommended | Also available |
|---|---|---|
| macOS (Apple Silicon + Intel) | `brew install --cask verbatim` | `.dmg` (signed + notarized) |
| Windows 11 | `winget install Verbatim` | `.msi` (Authenticode-signed) |
| Linux | `flatpak install flathub app.verbatim.Verbatim` | AppImage, `.deb` |

Then grant the microphone (and, on macOS, Accessibility) permission when Verbatim asks.
Full per-channel and per-OS instructions are in the [documentation](docs/site/src/content/docs/).

## Using it

- **macOS:** hold **right Option** and speak (push-to-talk).
- **Windows / Linux:** press **Ctrl + Alt + Space** to start and stop (toggle).

The hotkey, push-to-talk vs toggle, the personal dictionary, per-app profiles, and raw mode are all configurable - see [Using Verbatim](docs/site/src/content/docs/using.md).
Hit a message like `E4`? The [troubleshooting guide](docs/site/src/content/docs/troubleshooting.md) explains every one.

## Documentation

End-user docs live in [`docs/site/`](docs/site/) (an Astro Starlight site, readable as plain Markdown):

- [What is Verbatim?](docs/site/src/content/docs/index.md)
- [Install](docs/site/src/content/docs/install.md)
- [Permissions](docs/site/src/content/docs/permissions.md)
- [Using Verbatim](docs/site/src/content/docs/using.md)
- [Troubleshooting (E1-E10)](docs/site/src/content/docs/troubleshooting.md)

## Privacy

- No network code except the hash-verified model downloader and the OS update channels.
- All audio, transcripts, and history stay in your platform data directory; temporary audio is deleted right after transcription.
- The daemon's control socket is owner-only and accepts only start/stop/toggle/status - nothing can push text through Verbatim into your apps. See [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

---

## For developers

Verbatim is a Cargo workspace (Rust stable, edition 2024, MSRV 1.88 pinned via `rust-toolchain.toml`).

| Crate | Layer | Contents |
|---|---|---|
| `verbatim-core` | Core | session state machine, event bus, error taxonomy, session runner actor |
| `verbatim-platform` | Platform | hotkey/audio/injection/clipboard/permission/focus/autostart traits, deterministic fakes, per-OS backends |
| `verbatim-engines` | Engine | transcription (whisper.cpp, sherpa-onnx) + polish (llama.cpp) traits, engine registry, fakes |
| `verbatim-app` | App | the `verbatim` binary (`daemon`/`gui`/`trigger`/`status`/`stats` CLI) + Tauri 2 shell |

```sh
cargo build --locked      # build all crates
cargo test  --locked      # unit + walking-skeleton integration tests
```

The design documents are the source of truth:

| Document | Purpose |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Product requirements: vision, goals, non-goals, success criteria |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture: layers, subsystems, interfaces, data flow |
| [docs/UX.md](docs/UX.md) | UX specification: state machine, error handling, onboarding, accessibility |
| [docs/ENGINEERING.md](docs/ENGINEERING.md) | Engineering specification: repo layout, testing, CI/CD, packaging, releases |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Implementation roadmap: milestones, acceptance criteria |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Security threat model + IPC surface review |

`spikes/` holds throwaway prototypes for the highest-risk subsystems (no quality bar).

## License

MIT, including the Verbatim name and branding: forks may ship as-is.
See [LICENSE](LICENSE).
