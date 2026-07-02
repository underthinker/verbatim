# Verbatim - Product Requirements Document

Status: signed off by Tristen, 2026-07-02 (differentiation thesis, stack, platforms, process, name, license).
Owner: Tristen.
Last updated: 2026-07-02.

## 1. Vision

A cross-platform, privacy-first, fully local dictation application that turns speech into polished, ready-to-send text in any app, with a native-quality experience on macOS, Windows, and Linux.

The one-line pitch: **Wispr Flow quality, 100% local, open source, on every desktop OS.**

## 2. Why Verbatim exists (differentiation thesis)

Local dictation apps already exist.
[Handy](https://github.com/cjpais/Handy) (Tauri/Rust, MIT, ~22k stars) owns "press a key, get raw transcription" cross-platform.
[VoiceInk](https://github.com/Beingpax/VoiceInk) (Swift, GPLv3) is the most polished option but is macOS-only, paid-binary, and closed to contributions.
[OpenWhispr](https://github.com/OpenWhispr/openwhispr) (Electron) is cross-platform but heavyweight and increasingly a do-everything app (meetings, notes, agents).

None of them delivers what makes Wispr Flow feel magical: the text that lands in your app is not what you said, it is what you meant to write.
Filler words removed, punctuation and capitalization correct, formatting matched to the destination app, personal vocabulary respected.
Wispr Flow does this in the cloud; no open-source tool does it well locally.

Verbatim's differentiators, in priority order:

1. **Local text polish.**
   A local LLM stage between transcription and injection that removes filler, fixes grammar and punctuation, applies a personal dictionary, and adapts tone per destination app.
   Always with a raw-mode bypass, and never off-device.
2. **UX polish as a standing bar.**
   Non-technical users can install, grant permissions, download a model, and dictate within minutes, guided the whole way.
   Every error state has a designed, graceful response (see UX spec).
3. **Honest cross-platform citizenship.**
   Linux (including Wayland) is a first-class target, not a port.
   Handy's issue tracker shows this is where cross-platform dictation apps go to die; Verbatim treats it as a headline feature.
4. **A real open project.**
   MIT-licensed including branding usable by forks, contributions welcome, engine-agnostic architecture so the community can add engines without touching the core.

Non-differentiators (things we must match, not beat): raw transcription accuracy (everyone uses the same Whisper/Parakeet weights) and basic hotkey-to-paste mechanics.

## 3. Goals

- 100% local processing: audio and text never leave the device.
- Zero subscriptions, zero accounts, zero telemetry (none, not "opt-out").
- Cross-platform: macOS, Windows, Linux (X11 and Wayland), from the first release.
- Low latency: speech-to-injected-text fast enough to feel instant (budgets in section 7).
- Engine-agnostic: speech engines sit behind a stable interface; whisper.cpp and Parakeet ship in v1.
- Accessible to non-technical users: signed installers, guided onboarding, in-app model management, easy updates.
- Modular architecture that another engineer can extend without archaeology.

## 4. Non-goals

Verbatim is not:

- an AI chatbot or voice assistant (no "ask the AI anything" mode),
- a meeting recorder or transcription service for audio files at scale,
- a note-taking or knowledge-management app,
- a DAW or audio editor,
- a cloud service of any kind (no sync, no accounts, no BYOK cloud engines in v1),
- a mobile app (desktop only for v1; mobile is explicitly out of scope, not deferred-but-promised).

Feature requests that pull toward these become plugins or forks, not core.

## 5. Target users

1. **Privacy-required professionals** (lawyers, doctors, journalists, security folks): want Wispr Flow but contractually or ethically cannot send audio to a cloud.
2. **Developers and writers** who dictate into editors, terminals, chat, and email all day and care about latency and correctness of the injected text.
3. **Linux users** with no good dictation option at all today.
4. **Accessibility users** for whom typing is painful or impossible; for them reliability and error recovery matter more than speed.

## 6. Competitive analysis

Verified against the linked repos and their issue trackers, 2026-07.

| | Verbatim (target) | Handy | VoiceInk | OpenWhispr | OpenSuperWhisper | Wispr Flow |
|---|---|---|---|---|---|---|
| Platforms | macOS, Win, Linux | macOS, Win, Linux | macOS 14.4+ only | macOS, Win, Linux | macOS (ARM) only | macOS, Win, iOS, Android |
| 100% local | Yes | Yes | Yes (transcription) | Optional (BYOK cloud) | Yes | No (cloud) |
| Text polish (filler removal, formatting) | **Yes, local LLM** | No | Partial (AI modes, cloud/Ollama BYO) | Via cloud agents | No | Yes (cloud, its moat) |
| Personal dictionary | Yes | No | Yes | Partial | No | Yes |
| Per-app formatting profiles | Yes | No | Partial (context-aware) | No | No | Yes |
| Wayland support | **First-class target** | Weak (wtype/dotool workarounds, open issues) | n/a | Weak | n/a | n/a |
| Engines | whisper.cpp, Parakeet; trait for more | whisper.cpp, Parakeet | whisper.cpp, Parakeet | whisper.cpp, sherpa-onnx, cloud | whisper.cpp, Parakeet | proprietary cloud |
| Stack | Tauri 2 + Rust | Tauri + Rust | Swift | Electron | Swift | proprietary |
| License | MIT incl. branding | MIT, branding proprietary | GPLv3, paid binary, no PRs | MIT | MIT | proprietary, subscription |
| Footprint | Small (Tauri) | Small | Small | Large (Electron) | Small | n/a |

Reading of the field: Handy is the closest competitor and a good product; Verbatim wins only if the polish pipeline and the Linux story are genuinely better, and loses if it ships as "Handy minus maturity."
The polish pipeline is therefore not optional scope; it is the reason to build.

## 7. Success criteria (v1)

Measurable, checked before calling v1 done:

- **Latency:** p50 hotkey-release to injected raw text under 800 ms for a 10-second utterance on Apple Silicon and a mid-range Windows laptop; p50 under 1.5 s with polish enabled. Budgets validated by spike 3/4 measurements (see `spikes/`).
- **Onboarding:** a non-technical user goes from installer download to first successful dictation in under 5 minutes without reading documentation.
- **Reliability:** every error state in the UX spec catalog has a designed response; no dead-end states. Crash-free session rate above 99.5% in dogfooding.
- **Cross-platform:** all acceptance tests pass on macOS (ARM + Intel), Windows 11 x64, Ubuntu 24.04 X11 and Wayland (GNOME + KDE).
- **Polish quality:** on a fixed benchmark set of real dictation samples, polish output preferred over raw transcription in blind comparison at least 80% of the time, with zero meaning-altering edits in the accepted set.
- **Install:** signed dmg + Homebrew cask, signed msi + winget, AppImage + Flathub.

## 8. v1 feature cut

In v1:

- Global hotkey (hold-to-talk and toggle modes), recording overlay, menu bar/tray presence.
- Local transcription via whisper.cpp and Parakeet (sherpa-onnx), in-app model download and management.
- VAD-based end-of-speech handling (Silero).
- Local LLM text polish with personal dictionary, per-app profiles, and raw-mode bypass.
- Text injection into the focused app with clipboard fallback; graceful handling of secure fields.
- Settings UI, guided onboarding, permission flows.
- Multi-microphone selection, language auto-detection.
- History of recent dictations (local only, optional, clearable).

Post-v1 (roadmap, explicitly not v1):

- Plugin system, additional engines (MLX, Faster-Whisper sidecar), streaming transcription, auto-updater beyond store/package-manager updates, voice commands ("new line", "scratch that"), custom vocabulary training.

## 9. Risks

- **Handy ships polish first.** Mitigation: speed matters, but the deeper moat is the per-app profile system and Linux quality; also Handy's proprietary branding limits its fork ecosystem.
- **Local polish is too slow or too lossy on low-end hardware.** Mitigation: strict latency budget with automatic fallback to raw mode; polish quality gate in success criteria; spike 4 validates before architecture freezes.
- **Wayland fragmentation makes first-class Linux support unachievable.** Mitigation: spike 1 maps per-compositor reality before we promise anything publicly; the PRD claim is downgraded if the spike says so.
- **Scope creep toward OpenWhispr-style do-everything.** Mitigation: non-goals list is enforced in review; new scope requires a PRD amendment.

## 10. Decisions log

Confirmed 2026-07-02:

- Name: **Verbatim** (trademark search before wide distribution, tracked in M4).
- License: **MIT for everything, branding included**; forks may ship as-is (deliberate contrast with Handy's reserved branding).
- Differentiator: local text polish is the v1 headline; spike 4 validated feasibility.
- Stack: Tauri 2 + Rust; all three platforms from first commit; hardware validation of spikes runs during M1.

Still open:

- Parakeet weights are CC-BY-4.0: attribution surface design (model manager + About, per ENGINEERING.md 5.1).
- Default polish model per hardware tier: Qwen2.5-1.5B Q4 on Apple Silicon (spike 4); Windows CPU-only tier decided after M1 re-benchmark.
