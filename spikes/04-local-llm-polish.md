# Spike 4 - Local LLM text polish feasibility

Status: measured.
Machine: Apple M5, 16 GB, macOS 26.5, llama.cpp via Homebrew (Metal).
Model: Qwen2.5-1.5B-Instruct, Q4_K_M GGUF (986 MB).
Date: 2026-07-02.

## Setup

System prompt: dictation-cleanup rules (remove fillers/false starts/repeats, fix grammar/punctuation/capitalization, expand casual forms, never add information, never answer questions) plus **one few-shot example pair**.
Temperature 0.
The few-shot example mattered: the zero-shot variant of the same rules left "um so," intact and failed to capitalize; with one example the output was clean on every test.

## Results

| Test | Input (dictated, filler-heavy) | Output | Verdict |
|---|---|---|---|
| Casual request | "um so hey can you uh can you send over the the latest draft of the architecture doc when you get a chance i wanna like review the section on on text injection before our meeting tomorrow morning at ten" | "Hey, can you send over the latest draft of the architecture doc when you get a chance? I want to review the section on text injection before our meeting tomorrow morning at ten." | Perfect: fillers and repeats gone, wanna -> want to, correct capitalization, meaning intact |
| Question trap | "hey um quick question uh what time does the the standup start tomorrow and and should i prepare anything" | "Hey, quick question. What time does the standup start tomorrow, and should I prepare anything?" | Passed: the question was cleaned, **not answered** |
| Technical jargon | "okay so the uh the transcription engine trait needs a a transcribe method that takes um pcm samples at sixteen kilohertz and returns like a result with segments and uh confidence scores" | "The transcription engine trait should include a transcribe method that takes PCM samples at sixteen kilohertz and returns a result with segments and confidence scores." | Good: pcm -> PCM unprompted; one soft rewording ("needs" -> "should include") - acceptable but shows drift pressure; "sixteen kilohertz" not normalized to "16 kHz" (personal-dictionary / normalization rules needed) |

## Latency (model resident; one-shot load adds ~225 ms once)

| Metric | Value |
|---|---|
| Prompt eval | ~2,500-2,700 tok/s (system prompt + input, 130-200 tokens, ~50-70 ms) |
| Generation | ~100 tok/s (~10 ms/token) |
| Short dictation end-to-end | 270-410 ms |

**Scaling law to design around: polish generation costs ~10 ms per output token on this hardware, so latency grows linearly with dictation length.**
A 10 s utterance (~45 output tokens) polishes in ~450 ms; a 60 s ramble (~250 tokens) takes ~2.5 s.
The UX deadline model (inject raw on deadline miss) plus the PRD's 1.5 s p50-with-polish target for 10 s utterances are both satisfied; long dictations need either a longer advertised budget, a faster/smaller model tier, or chunked polish.

## Conclusions for ARCHITECTURE.md

1. Local polish is **feasible with good quality at ~1 GB of model**, validating the PRD's headline differentiator. Combined spike 3 + 4 pipeline estimate for a 10 s utterance on Apple Silicon: ~0.6-1.1 s, inside the 1.5 s budget.
2. Polish model must be **kept resident** alongside the ASR model (~1.5 GB combined for small.en + Qwen 1.5B Q4; fine on 8 GB+ machines, tiered defaults needed below that).
3. Prompt engineering is part of the product: the few-shot example is load-bearing, and the per-app profiles from the UX spec map to swappable system prompts. Version prompts like code, with a regression benchmark set.
4. Guard against rewording drift: temperature 0, an output-similarity check (reject polish when edit distance from raw exceeds a length-scaled threshold), and the UX raw/diff history view as the trust backstop.
5. Number/unit normalization ("sixteen kilohertz" -> "16 kHz") should be explicit prompt rules per profile, not assumed.
6. Re-benchmark on Windows CPU-only laptop in M1; if generation drops well below ~40 tok/s, the default there may need a smaller model (Qwen 0.5B / Llama 3.2 1B) or polish default-off.
