# M3 - Text polish (the differentiator)

The milestone that earns the product its headline: raw dictation becomes clean,
meaning-preserving text, fully local, without ever surprising the user.

## Acceptance criteria (the gate)

From ROADMAP.md M3:

- [ ] Blind comparison on the benchmark set: polished preferred >= 80%, zero meaning-altering edits in the accepted set (PRD 7).
- [ ] Deadline misses inject raw with no user-visible failure; measured miss rate < 5% for 10 s utterances on reference hardware.
- [ ] Polish adds <= 700 ms p50 for 10 s utterances on Apple Silicon; hardware-tier defaults applied elsewhere.
- [ ] Prompt changes are benchmark-gated in CI.

## Current state (verified 2026-07-07)

- `PolishEngine` trait (`crates/verbatim-engines/src/polish.rs`): `load`/`unload`/`is_loaded`/`polish(raw, profile, deadline)`. Only a fake impl.
- `llama-cpp = []` in `verbatim-engines/Cargo.toml` is an empty stub feature - no real backend.
- `SessionRunner::run_polish` (`runner.rs:316`) already races the deadline and degrades to raw: `Polished` -> inject polished, `Rejected` -> inject raw + `PolishSkipped`, `Err` -> `E10` fault + raw. State machine + walking-skeleton E2E already cover the degrade paths.
- Gap: `run_polish` calls `PolishProfile::default()` hardcoded. No similarity guard, no personal dictionary, no per-app profile selection, no prompt assets, no benchmark, no calibration.

## Sources of truth (do not re-decide)

- ARCHITECTURE.md 4.3 (polish pipeline: similarity guard is caller-side, deadline is engine-side self-reject, dictionary applied via prompt AND deterministic post-pass, per-app profile from FocusTracker).
- UX.md 5 (polish UX: user-visible dictionary, per-app profiles, terminals default raw, raw always one modifier away), UX.md 5.3 (versioned prompts, one-click confirm per learned term), E10 (silent raw fallback + one-time tray notice).
- ENGINEERING.md: prompts live in `assets/prompts/<profile>@<version>.txt` with inline few-shot; prompt changes are benchmark-gated code changes; polish-quality + latency-regression benches in CI.
- Spike 4 findings in `spikes/`.

## Cross-cutting decisions to surface before coding

- **llama backend crate**: `llama-cpp-2` per ARCHITECTURE.md 4.3. Confirm Metal/Vulkan/CPU feature-gating mirrors the whisper.cpp split already in Cargo.toml. Default build stays fake-only (no OS/model deps) so CI stays green.
- **Similarity metric**: length-scaled edit-distance threshold. Pick the exact function + threshold curve in Phase B, back it with the benchmark set (not vibes).
- **Profile config shape**: per-app profile map + personal dictionary live in existing config (ARCHITECTURE.md 4.8). Extend the config struct the M2 Settings tab already reads/writes; do not fork a new store.

## Phase A - branch `m3-phaseA-llama-engine`

Real llama.cpp `PolishEngine` behind a `llama-cpp` feature, mirroring the whisper.cpp pattern.

1. Wire `llama-cpp-2` as an optional dep, feature-gated (`llama-cpp`, `llama-metal`, `llama-vulkan`) matching the whisper target-cfg split. Default build stays fake.
2. Implement `LlamaPolishEngine`: resident context, temperature 0, load/unload/is_loaded, `polish` generates under the profile prompt and self-rejects (`PolishOutcome::Rejected`) on deadline miss.
3. Prompt assembly from `PolishProfile` (system template + inline few-shot + dictionary terms).

Verification: feature build loads a small GGUF, polishes a fixture transcript at temp 0 deterministically; default (fake) build + tests untouched and green. Self-check: deadline-miss returns `Rejected`, not a block.

Anti-patterns: no leaking llama types across the trait boundary; no non-zero temperature; no unwrap/expect outside tests.

## Phase B - branch `m3-phaseB-similarity-guard`

Caller-side similarity guard in the core polish pipeline (ARCHITECTURE.md 4.3: guard belongs to the caller, not the engine).

1. Length-scaled edit-distance guard applied to `Polished` output in `run_polish`: over threshold -> treat as rejected, inject raw, record raw-only.
2. Threshold curve chosen against the benchmark fixture set; unit tests pin the boundary (just-under accepts, just-over rejects).

Verification: guard unit tests + a walking-skeleton case where an over-edited polish degrades to raw. Self-check: `assert` boundary test.

## Phase C - branch `m3-phaseC-dictionary`

Personal dictionary: prompt injection + deterministic post-pass (exact-match casing replacement), user-visible in Settings (UX.md 5.3).

1. Dictionary entries in config; applied both in prompt (Phase A assembly) and as a deterministic post-pass exact-match replacement (e.g. "pcm"->"PCM") after polish, so critical terms never depend on the LLM alone.
2. Settings UI: view/add/remove terms; every auto-learned term requires one-click confirm before it applies (no black box).

Verification: post-pass unit tests (casing map applied to raw and polished); Settings round-trips terms via existing config commands. Self-check: post-pass replaces known term regardless of LLM output.

## Phase D - branch `m3-phaseD-app-profiles-rawmode`

Per-app profiles + raw-mode modifier (UX.md 5.1, 5).

1. Profile map in config: FocusTracker frontmost app id at target time selects the profile in `run_polish` (replace the hardcoded `PolishProfile::default()`). Terminals default to raw.
2. Raw-mode modifier: one modifier forces raw injection for the current dictation regardless of profile.
3. Settings UI for per-app profile assignment.

Verification: profile-selection unit test (terminal app id -> raw, mapped app -> its profile); raw-modifier E2E injects raw. Self-check: assert terminal bundle id resolves to raw.

## Phase E - branch `m3-phaseE-benchmark-calibration`

Prompt versioning, polish-quality benchmark in CI, deadline calibration.

1. `assets/prompts/<profile>@<version>.txt` with inline few-shot; loader reads versioned asset.
2. Polish-quality benchmark harness in `benches/`: fixed transcript set -> polished output scored (exact-expectation + similarity-guard assertions). CI runs it on prompt/engine changes; a prompt change must ship benchmark deltas.
3. Deadline calibration micro-benchmark at onboarding (~10 ms/output-token, measured per machine) feeding `polish_deadline`.
4. Latency-regression gate for polish (>20% fails CI), matching the whisper harness.

Verification: benchmark runs green in CI on a prompt change; calibration produces a per-machine deadline. Acceptance criteria 1, 3, 4 close here.

## Final phase - acceptance sign-off

1. Re-check ROADMAP.md M3 criteria with evidence (blind-comparison run, deadline-miss-rate measurement, Apple Silicon p50, CI benchmark gate); tick in a closing PR.
2. Guards: no non-zero temperature; similarity guard covered; E10 path still degrades silently + one-time tray notice.
3. Blind-comparison result recorded in the PR.

Dependency order: A first (engine before pipeline features consume it). B after A. C and D after A, parallel with B (both extend config + `run_polish`). E last (benchmarks + calibration audit the whole pipeline).

## Open questions to resolve at kickoff

- GGUF polish model choice + default per hardware tier (ties to M0 spike 3/4 mid-range laptop measurement still open).
- Similarity threshold curve: exact function + constants (decide in Phase B against fixtures).
- Auto-learn source for dictionary terms (what proposes a term for one-click confirm) - may defer past v1 if UX.md doesn't pin it.
