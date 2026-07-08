# Spike 5 - Parakeet via sherpa-onnx: Rust binding go/no-go

Status: decided (desk research + API review). Verdict: **GO with `sherpa-rs`.**
Date: 2026-07-08.
Scope: pick the Rust binding for the spec'd `SherpaOnnx` transcription engine
(ARCHITECTURE.md 4.x) and confirm a Parakeet model that satisfies the PRD 136
attribution requirement. No linking/inference run here - that lands with the
feature-gated impl and its env-gated integration test (mirroring whisper).

## Options considered

| Binding | What it is | Fit for `SherpaOnnx` engine |
|---|---|---|
| **`sherpa-rs`** (v0.6.8, ~62k dl) | Rust bindings over k2-fsa's `sherpa-onnx` C++ (`OfflineRecognizer`) | **Direct match.** Mirrors how `whisper-rs` wraps whisper.cpp: one FFI surface hosting Parakeet now and other NeMo/sherpa models later, with backend declaration + CPU fallback - exactly the engine abstraction the architecture already names. |
| `parakeet-rs` (v0.3.5, active) | Parakeet-only, ONNX via `ort` (onnxruntime), fast on CPU, streaming | Lighter, more actively maintained, but Parakeet-only and does not model the multi-model `SherpaOnnx` engine. The fallback if `sherpa-rs` build friction bites. |
| `transcribe-rs` (cjpais/Handy) | Multi-engine wrapper (Parakeet, Whisper, ...) | Too high-level - it owns engine selection, which is our registry's job. Reference, not a dependency. |

## Verdict

**GO with `sherpa-rs`** for the `SherpaOnnx` engine. It is the direct binding to
the framework the architecture already spec'd, and it reuses the proven
whisper-rs shape (vendored C++ lib + optional Cargo feature + backend fallback),
so it drops into the engine registry with one entry and no new patterns.

Risks and mitigations:
- **Staleness** (last `sherpa-rs` release ~8 months old). Acceptable - sherpa-onnx
  itself is active and the C++ API `sherpa-rs` wraps is stable. If it lags a
  needed Parakeet export, the fallback is `parakeet-rs`/`ort` behind the same
  trait (no engine-boundary types leak, so swapping the impl is local).
- **Build weight** (links a C++ lib + onnxruntime). Same class of cost as
  whisper-rs already carries; gated behind the `sherpa-onnx` Cargo feature so
  fake-only/default CI builds pull none of it.
- **FFI safety**: `unsafe` stays inside the binding; our impl holds only the
  safe `sherpa-rs` handle. No sherpa types cross the `TranscriptionEngine`
  boundary (anti-pattern guard from the plan).

## Model

`nvidia/parakeet-tdt-0.6b` exported to ONNX for sherpa-onnx (k2-fsa hosts the
export). **License: CC-BY-4.0** - PRD 136's attribution requirement: the model
manager + About surface must credit NVIDIA. That attribution field + surfaces
land in this same phase, ahead of the engine wiring, so the credit exists before
the weights are downloadable.

## Follow-up (the FFI impl)

Deliberately deferred to a build environment where `sherpa-rs` compiles (it
links a C++ lib + onnxruntime), so the impl is written against the crate's real
API rather than guessed - writing FFI blind risks feature code that only fails
in the special CI job. This phase lands everything the impl does not gate on:
the Parakeet catalog entry, the attribution surfaces (PRD 136), and the
recommendation label. The impl then adds:

1. `SherpaOnnxEngine` `impl TranscriptionEngine` behind the `sherpa-onnx`
   feature (add `sherpa-rs` as its optional dep), mirroring `WhisperCppEngine`:
   resident model, GPU-then-CPU backend fallback, 16 kHz mono f32 in. No sherpa
   types cross the trait boundary.
2. Register it in `EngineRegistry` behind the feature flag (one entry) and add
   an app `parakeet` feature that pulls `verbatim-engines/sherpa-onnx`, plus a
   daemon selection branch (`VERBATIM_PARAKEET_MODEL`) mirroring the whisper one.
3. Env-gated integration test (`VERBATIM_PARAKEET_MODEL`) transcribing the JFK
   fixture, skipped when unset - same contract as the whisper test.
4. A feature-gated CI job building `--features sherpa-onnx` (mirrors the
   whisper-cpp/llama-cpp CI matrix) so the FFI stays green.
