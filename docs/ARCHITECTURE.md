# Verbatim - System Architecture

Status: draft for sign-off.
Companion documents: [PRD.md](PRD.md), [UX.md](UX.md), [ENGINEERING.md](ENGINEERING.md); spike findings in [spikes/](../spikes/).

## 1. Overview

Verbatim is a Tauri 2 application: a Rust core owns everything real (audio, engines, injection, state), and a TypeScript/React webview renders the thin UI surfaces (settings, onboarding, history, model manager).
The overlay is a separate native-configured Tauri window driven by core events.

Four layers with strict dependency direction (each layer may only call downward):

```
UI Layer          settings/onboarding/history webviews, overlay window, tray menu
   │  (Tauri commands + events only)
Core Layer        session state machine, audio pipeline, polish pipeline,
                  model manager, config, history, event bus, logging
   │  (Rust traits)
Platform Layer    hotkeys, audio capture, text injection, clipboard,
                  permissions, focus tracking, autostart          (per-OS impls)
Engine Layer      TranscriptionEngine impls (whisper.cpp, sherpa-onnx/Parakeet),
                  PolishEngine impl (llama.cpp)
```

Rules:

- The UI never touches engines or platform APIs; it calls Tauri commands and subscribes to core events. Everything the UI can do, a headless CLI could do too (this is also how the GNOME hotkey workaround works, see spike 1).
- The Core layer is pure cross-platform Rust; anything with `#[cfg(target_os)]` lives in the Platform layer behind a trait.
- Engines are dynamically selectable at runtime and hidden behind traits; adding an engine touches only the Engine layer plus a registry entry.

## 2. Crate layout

Cargo workspace (details of the full repo tree in ENGINEERING.md):

| Crate | Layer | Contents |
|---|---|---|
| `verbatim-core` | Core | session state machine, audio pipeline, VAD, polish pipeline, model manager, config, history, event bus; depends on trait definitions only |
| `verbatim-platform` | Platform | trait definitions (`HotkeyManager`, `AudioCapture`, `TextInjector`, `ClipboardGuard`, `PermissionProbe`, `FocusTracker`, `Autostart`) and the per-OS implementations (`macos/`, `windows/`, `linux/` modules) |
| `verbatim-engines` | Engine | `TranscriptionEngine` + `PolishEngine` traits, `whisper-cpp`, `sherpa-onnx`, `llama-cpp` implementations behind feature flags, engine registry |
| `verbatim-app` | UI | Tauri shell: command handlers, event bridge, tray, overlay window management, CLI entry (`verbatim trigger`) |

## 3. The session state machine

The core owns one `DictationSession` state machine, exactly mirroring UX.md section 2: `Idle -> Arming -> Recording -> Finalizing -> Transcribing -> Polishing -> Injecting -> Idle`, plus `Cancelled` and per-state error edges (UX error catalog E1-E10).

- Implemented as an explicit enum + transition table; illegal transitions are compile-time-visible and log-fatal in debug builds.
- The pipeline tail (Transcribing onward) is asynchronous: a new session may start Arming while the previous session's tail is still running (UX hotkey-chaining rule). Concurrency is bounded to one recording + N pipeline tails (N=2, older tails complete first to preserve injection order).
- Every state transition is published on the event bus; the overlay, tray, and logs are pure consumers. No surface queries state; they replay events.

## 4. Subsystems

### 4.1 Audio pipeline (core)

- Capture via `cpal` (behind `AudioCapture` trait for testability), resampled to 16 kHz mono f32 ring buffer.
- Silero VAD (ONNX, via `sherpa-onnx`'s VAD or `voice_activity_detector` crate) runs on the live stream for: end-of-speech tail detection in Finalizing, silence-only detection (UX "didn't catch anything"), and input-level events for the overlay waveform.
- Recording cap 5 min (UX); buffer is written to a temp WAV on cap or on engine failure (E3 "recording is saved").
- Device handling: follow-default-device on disconnect where the OS allows, else emit `DeviceLost` (E6).

### 4.2 Engine interface (engine layer)

```rust
pub trait TranscriptionEngine: Send + Sync {
    fn id(&self) -> EngineId;
    fn load(&mut self, model: &ModelHandle, opts: &EngineOptions) -> Result<(), EngineError>;
    fn unload(&mut self);
    fn is_loaded(&self) -> bool;
    fn transcribe(&self, audio: &AudioBuffer, opts: &TranscribeOptions)
        -> Result<Transcript, EngineError>;   // 16 kHz mono f32
}

pub struct Transcript {
    pub segments: Vec<Segment>,   // text, t0, t1, confidence
    pub language: LanguageTag,
}
```

- v1 implementations: `WhisperCpp` (whisper-rs bindings) and `SherpaOnnx` (Parakeet); each declares supported backends (Metal/CUDA/Vulkan/CPU) and the registry picks the best available with **automatic fallback to CPU on backend init failure** (spike 3: GPU-config crashes are Handy's top Windows/Linux failure).
- Engines stay resident after first load; unload on 15 min idle or OS memory-pressure signal (spike 3: load is cheap but not free).
- Streaming is deliberately absent from the v1 trait; batch beat the latency budget (spike 3). A `StreamingTranscriptionEngine` extension trait is reserved post-v1.

### 4.3 Polish pipeline (core + engine)

- `PolishEngine` trait wraps llama.cpp (llama-cpp-2 crate) with a resident context; single implementation in v1, trait kept for future backends (MLX, candle).
- Input: raw transcript + `PolishProfile` (system prompt template, few-shot examples, personal dictionary entries, normalization rules). Output: polished text or a typed rejection.
- Spike 4 rules are architectural requirements:
  - temperature 0; prompts are versioned assets with a regression benchmark (see ENGINEERING.md testing);
  - few-shot example is part of every profile;
  - **similarity guard**: reject polish (use raw) when edit distance from raw exceeds a length-scaled threshold;
  - **deadline**: polishing races a deadline derived from utterance length (~10 ms/output-token on Apple Silicon; calibrated per machine at onboarding by a micro-benchmark); miss -> raw, count E10-class metric locally.
- Personal dictionary: user-confirmed term list applied via prompt and via deterministic post-pass (exact-match replacement for casing like "PCM", "Wispr Flow"), so critical terms never depend on the LLM alone.
- Per-app profile selection: `FocusTracker` supplies the frontmost app id at injection target time; profile map lives in config (terminals default to raw, per UX 5.1).

### 4.4 Text injection (platform)

```rust
pub trait TextInjector: Send + Sync {
    fn probe(&self) -> Vec<InjectionBackend>;          // capability-probed, ordered
    fn inject(&self, text: &str, target: &FocusedApp, strategy: InjectionStrategy)
        -> Result<InjectionReceipt, InjectError>;
}
```

Backend chains (from spikes 1 and 2), first healthy backend wins, per-app profile may pin one:

- **macOS**: transient-pasteboard paste (`org.nspasteboard.TransientType`, changeCount-aware clipboard restore) -> CGEventPost unicode typing. Secure-input check (`IsSecureEventInputEnabled`) before any attempt (E5).
- **Windows**: `SendInput` unicode (KEYEVENTF_UNICODE) -> clipboard+Ctrl-V with the same transient/restore discipline. UIPI: elevated targets fail; map to E4.
- **Linux**: libei via RemoteDesktop portal (GNOME >= 46, Plasma >= 6.1, `restore_token` persisted) -> /dev/uinput virtual keyboard (udev uaccess or input group; keymap handled in-process) -> wlroots `virtual-keyboard-v1` -> clipboard-assisted paste -> clipboard-only. **Never shell out to ydotool/wtype; never trust exit codes as success** (spike 1 silent-failure trap).
- Focus rule (E7): capture `FocusedApp` at recording start; re-check before injection; mismatch -> clipboard fallback path.
- Overlay is hidden before injection on Linux (spike 1 KDE focus-steal) and is globally non-activating.

### 4.5 Hotkeys (platform)

- **macOS**: Carbon `RegisterEventHotKey` (down+up, zero TCC permissions, spike 2); optional Globe-key trigger via event tap behind an explicit opt-in that explains the extra permission.
- **Windows**: `RegisterHotKey` + low-level keyboard hook for hold-to-talk release.
- **Linux**: GlobalShortcuts portal (KDE reliable; GNOME behind a flag due to the Mutter portal bug) + `verbatim trigger [start|stop|toggle]` CLI/IPC command for native-shortcut binding (default on GNOME), + opt-in evdev listener for hold-to-talk (spike 1).
- Semantics (hold/toggle/double-tap-lock, 250 ms accidental-press threshold) are implemented once in core on top of raw down/up events from the trait.

### 4.6 Permissions (platform)

- `PermissionProbe` exposes per-capability state (`Granted | Denied | Undetermined | NotNeeded`) without prompting (spike 2 preflight APIs; Linux checks uinput access + portal availability).
- One state machine per capability consumed by onboarding (guided steps, live re-check polling) and by error mapping (E1, E9).

### 4.7 Model manager (core)

- Registry of known models (ASR + polish) with per-hardware recommendations (RAM/GPU detection); user-added GGUF/ONNX paths supported.
- Downloads: HTTPS with SHA-256 verification, resumable (Range requests), into the platform data dir; disk-space preflight; E8 mapping.
- Storage layout and attribution metadata (Parakeet CC-BY-4.0 surfaces in About) in ENGINEERING.md.

### 4.8 Configuration, history, logging (core)

- Config: single versioned TOML (`config.toml`) in the platform config dir; schema-migrated on upgrade; watched for external edits. All defaults live in one annotated `Config::default()`.
- History: SQLite (rusqlite) storing raw+polished pairs, app id, timestamps; retention pruning (default 7 days, UX 7); "off" mode writes nothing; clear-all is a single delete + VACUUM.
- Logging: `tracing` with rotating file output in the data dir, level from config; **no network sinks exist in the codebase** (PRD: zero telemetry). Errors E1-E10 log structured events for local diagnosis.

### 4.9 Event bus (core)

- A `tokio::sync::broadcast` of a single typed `Event` enum: state transitions, input levels, download progress, permission changes, errors.
- The Tauri bridge forwards the bus to the webview event system 1:1; the overlay and tray are direct Rust consumers (no webview round-trip on the hot path).

## 5. Data flow (happy path, 10 s utterance, measured budget)

```
hotkey down ──► overlay visible (< 50 ms, direct event)
           ──► mic stream open (< 100 ms)  [ARMING -> RECORDING]
hotkey up  ──► VAD tail flush (≤ 300 ms)   [FINALIZING]
           ──► transcribe: 150-550 ms resident small.en/base.en (spike 3)
           ──► polish: ~10 ms/output token, 270-450 ms typical (spike 4), deadline-guarded
           ──► inject: < 100 ms paste path
Total: raw ≈ 0.4-1.0 s; polished ≈ 0.7-1.5 s   (PRD budgets: 800 ms raw / 1.5 s polished p50)
```

## 6. Cross-cutting decisions

- **Threading**: one dedicated real-time audio thread (no allocation on the callback), a tokio runtime for pipeline/IO, engines on a blocking pool. The state machine lives on a single actor task; all mutation flows through its mailbox.
- **No plugins in v1** (PRD non-goal enforcement): extension points are the four traits; the plugin system is a post-v1 design on top of them.
- **No auto-updater subsystem in v1**: updates ride distribution channels (Homebrew, winget, Flathub, GitHub releases); Tauri's updater may be revisited post-v1.
- **Headless parity**: `verbatim trigger`/`verbatim status` CLI exists from M1 (required for GNOME, useful for testing and scripting).

## 7. Spike-to-architecture traceability

| Spike finding | Architectural consequence |
|---|---|
| GNOME/KWin refuse virtual-keyboard-v1; libei portal + uinput are the viable paths (spike 1) | Linux injection chain order in 4.4; in-process backends; no ydotool/wtype dependency |
| Silent injection "success" is common (spike 1) | `InjectionReceipt` + capability probing; never trust exit codes |
| Carbon hotkeys need no TCC and deliver up/down (spike 2) | macOS default trigger avoids Input Monitoring entirely |
| TCC grants keyed to signing identity (spike 2) | Stable Developer ID from first public build (ENGINEERING.md) |
| Batch transcription well inside budget; GPU-config crashes common elsewhere (spike 3) | No streaming in v1 trait; mandatory backend fallback chain; engines resident |
| Polish ~10 ms/token, few-shot load-bearing, rewording drift exists (spike 4) | Deadline + similarity guard + versioned prompts + deterministic dictionary post-pass |
