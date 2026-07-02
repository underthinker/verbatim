# Verbatim - Engineering Specification

Status: draft for sign-off.
Companion documents: [PRD.md](PRD.md), [ARCHITECTURE.md](ARCHITECTURE.md), [UX.md](UX.md), [ROADMAP.md](ROADMAP.md).

## 1. Repository layout

```
verbatim/
├── Cargo.toml                  # workspace
├── crates/
│   ├── verbatim-core/
│   ├── verbatim-platform/      # traits + macos/ windows/ linux/ impls
│   ├── verbatim-engines/       # whisper-cpp, sherpa-onnx, llama-cpp (feature-gated)
│   └── verbatim-app/           # tauri shell + CLI entry
├── ui/                         # React + TypeScript + Vite (webview sources)
│   ├── src/
│   │   ├── surfaces/           # settings, onboarding, history, models, about
│   │   ├── overlay/            # overlay window app (separate entry)
│   │   └── shared/             # event bridge, design tokens, components
├── assets/
│   ├── prompts/                # versioned polish prompt templates (see 5.3)
│   └── models.json             # model registry: URLs, hashes, hw recommendations
├── docs/                       # these documents
├── spikes/                     # throwaway prototypes + findings (no quality bar)
├── benches/                    # latency + polish-quality benchmark harnesses
├── tests/                      # cross-crate integration tests
└── .github/workflows/
```

## 2. Technology choices (pinned)

| Concern | Choice | Notes |
|---|---|---|
| Shell | Tauri 2 | overlay via non-activating always-on-top window; tray via tauri tray API |
| Core language | Rust (stable, edition 2024) | MSRV pinned in workspace |
| UI | React 18 + TypeScript strict + Vite + Tailwind | no runtime CSS-in-JS; design tokens shared with overlay |
| Audio | cpal | behind `AudioCapture` trait |
| VAD | Silero via sherpa-onnx (single ONNX runtime dependency) | evaluate `voice_activity_detector` crate as lighter alternative in M1 |
| ASR | whisper-rs (whisper.cpp), sherpa-onnx (Parakeet) | backend features: metal, cuda, vulkan, cpu |
| Polish | llama-cpp-2 | resident context; model per hardware tier |
| Linux input | reis (libei), evdev/uinput crates | no external binary dependencies (spike 1) |
| DB | rusqlite (bundled SQLite) | history only |
| Config | toml + serde, versioned schema | |
| Logging | tracing + tracing-appender | file rotation, no network sinks ever |

## 3. Coding standards

- `cargo fmt` + `clippy -D warnings` + `cargo deny` (licenses/advisories) gate every PR; TypeScript: `tsc --noEmit`, ESLint, Prettier.
- No `unsafe` outside `verbatim-platform` and engine FFI; every `unsafe` block carries a `// SAFETY:` justification.
- Platform trait implementations may not leak OS types across the trait boundary.
- Error handling: `thiserror` per crate, one top-level error taxonomy mapping to UX error IDs (E1-E10); no `unwrap`/`expect` outside tests and startup.
- Markdown docs: one sentence per line.
- Commits: Conventional Commits; PRs small and single-purpose.

## 4. Testing strategy

| Level | Scope | How |
|---|---|---|
| Unit | state machine transition table, VAD gating, similarity guard, dictionary post-pass, config migration | pure Rust, no OS deps; state machine gets exhaustive illegal-transition tests |
| Engine integration | each engine transcribes fixture WAVs within accuracy + latency envelopes | tiny models cached in CI; fixtures are real recorded speech, not synthesized (spike 3 caveat) |
| Polish quality benchmark | fixed benchmark set of dictation transcripts -> polished output scored (exact-expectation + similarity-guard assertions) | runs in CI on prompt or model changes; prompts are versioned, a prompt change must ship with benchmark deltas |
| Platform integration | injection receipt honesty, hotkey up/down semantics, permission probe states | per-OS CI runners where possible; Linux Wayland matrix (GNOME, KDE, Hyprland) on a self-hosted or nested-compositor (weston headless) runner |
| E2E smoke | launch app, simulate hotkey, fixture audio through a fake `AudioCapture`, assert text injected into a test window | tauri-driver/WebDriver per platform; the fake-capture seam makes this deterministic |
| Latency regression | benches/ harness replays spike 3/4 measurements | fails CI on >20% regression against per-runner baselines |

## 5. Assets and data

### 5.1 Model registry

`assets/models.json`: id, engine, URL, SHA-256, size, RAM/GPU recommendation tier, license + attribution string (Parakeet CC-BY-4.0 renders in About and model manager).
Downloads verify hash before activation; partial files resume via Range; storage in platform data dir under `models/<engine>/<id>/`.

### 5.2 Data locations (platform conventions)

| | macOS | Windows | Linux |
|---|---|---|---|
| Config | `~/Library/Application Support/Verbatim/config.toml` | `%APPDATA%\Verbatim\` | `$XDG_CONFIG_HOME/verbatim/` |
| Models, history, logs | `~/Library/Application Support/Verbatim/` | `%LOCALAPPDATA%\Verbatim\` | `$XDG_DATA_HOME/verbatim/` |

### 5.3 Prompts

Polish prompt templates live in `assets/prompts/<profile>@<version>.txt` with the few-shot examples inline (spike 4: few-shot is load-bearing).
Prompt changes are code changes: reviewed, versioned, benchmark-gated.

## 6. CI/CD

GitHub Actions:

- **PR pipeline** (every push): fmt/clippy/deny/tsc/eslint -> unit + engine tests on `macos-latest` (ARM), `windows-latest`, `ubuntu-24.04` -> debug build all three -> polish benchmark when prompts/engines changed.
- **Main pipeline**: PR pipeline + E2E smoke + latency regression.
- **Release pipeline** (tag `v*`): matrix release builds -> sign + notarize -> package -> draft GitHub release with checksums + SBOM -> publish channel PRs (Homebrew tap, winget manifest, Flathub repo).
- Caching: Rust + model fixtures cached; native engine builds (whisper.cpp etc.) in a prebuilt cache layer to keep CI under ~15 min.

## 7. Signing, packaging, distribution

- **macOS**: Developer ID Application certificate (stable identity from the first public build; TCC grants are keyed to it, spike 2), hardened runtime, notarization via `notarytool`. **No App Sandbox** (incompatible with Accessibility injection, spike 2) -> direct-download `.dmg` + Homebrew cask; Mac App Store is out of scope permanently.
- **Windows**: Authenticode (OV cert initially; EV/Azure Trusted Signing when SmartScreen reputation matters), `.msi` via WiX through tauri-bundler, winget manifest.
- **Linux**: AppImage (primary, bundles portals-client libs) + Flatpak on Flathub (libei portal path is Flatpak-clean, spike 1; uinput fallback documented as needing host permission) + `.deb`. AppImage and Flatpak both ship the udev-rule helper script referenced by onboarding (E9).
- Versioning: SemVer; `0.x` until v1.0 acceptance criteria (PRD 7) pass.
- Release cadence: tag-driven; release notes generated from Conventional Commits; CHANGELOG auto-generated (never hand-edited).

## 8. Security and privacy posture

- No network code outside the model downloader (fixed HTTPS hosts, hash-verified) and the distribution channels' own update mechanics; CI asserts no other outbound-capable dependencies (`cargo deny` ban-list on http client crates outside the downloader crate).
- All user data (audio buffers, transcripts, history, dictionary) stays in the platform data dir; temp audio files are deleted after successful transcription and on startup sweep.
- Threat model doc is a v1.0 deliverable: injection capability misuse (uinput/Accessibility) is the sensitive surface; the binary must never expose an IPC surface that lets *other* processes inject text through us (the `verbatim trigger` IPC accepts trigger verbs only, never text payloads).

## 9. Definition of done (any feature)

1. Unit + relevant integration tests pass on all three OS targets.
2. UX error catalog updated when a new failure mode was introduced.
3. Docs in `docs/` updated in the same PR (architecture drift is a review blocker).
4. No new clippy/eslint suppressions without justification comments.
5. Latency benchmarks within envelope.
