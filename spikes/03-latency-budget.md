# Spike 3 - Transcription latency budget

Status: measured.
Machine: Apple M5, 16 GB unified memory, macOS 26.5, whisper.cpp via Homebrew (ggml 0.15.3, Metal backend active).
Input: 10.9 s synthesized dictation sample (16 kHz mono WAV, two sentences, 33 words).
Date: 2026-07-02.

## Numbers

| Model | Load | Encode | Decode | Total (cold, incl. load) |
|---|---|---|---|---|
| base.en (142 MB) | 48 ms | 30 ms | 2 ms | **196 ms** |
| small.en (466 MB) | 126 ms | 106 ms | 7 ms | **533 ms** |

Both models produced a 100% accurate transcript of the sample, including correct punctuation and the "at ten" -> "at 10" normalization.

## Interpretation

- Even cold-start small.en beats the PRD's 800 ms raw-latency budget for a 10 s utterance on Apple Silicon; a resident model (the real app keeps it loaded) leaves roughly 650-700 ms of headroom for VAD tail, polish, and injection.
- Model load is cheap enough that lazy-load-on-first-dictation is acceptable, but keep-resident is still the design (load time recurs per dictation otherwise, and small models fit easily in memory).
- Transcription cost scales roughly with audio length; a 60 s ramble is the case to re-measure. Streaming/chunked decode remains post-v1, but the budget suggests batch is fine for typical dictation lengths.

## Caveats

- Synthesized (`say`) audio is cleaner than real microphone speech; real-world accuracy will be lower and should be benchmarked with recorded samples in M1.
- This is the strongest consumer chip Apple ships; the PRD budget must also be validated on a mid-range Windows x64 laptop (CPU-only or Vulkan) and a Linux machine in M1. Handy's issue tracker shows Whisper crashes on some Windows/Linux GPU configs, so backend fallback (Metal/Vulkan/CUDA -> CPU) needs to be automatic.
- Parakeet (via sherpa-onnx) is reported faster than Whisper at similar quality for English; measure it in M1 before choosing the default model per platform.

## Feed into ARCHITECTURE.md

- Keep engine resident between dictations; unload on memory pressure or idle timeout.
- Latency budget table (10 s utterance, Apple Silicon): capture stop -> VAD flush 50 ms, transcription 150-550 ms, polish (spike 4), injection 50 ms.
- Automatic backend fallback chain per platform is part of the engine spec, not an afterthought.
