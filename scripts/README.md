# Verification scripts

Turnkey scripts for the two hardware-bound M1 acceptance criteria.
They wrap the same checks CI runs (bench, seam E2E) plus the manual real-keypress
checklist that only a live desktop session can complete, so validating on real
hardware is one command per platform.

| Script | Criterion | Platforms |
| --- | --- | --- |
| `verify-latency.{sh,ps1}` | p50 raw latency < 800 ms, resident model ([ROADMAP.md:27], #16) | macOS / Linux (`.sh`), Windows (`.ps1`) |
| `verify-injection.{sh,ps1}` | dictation lands text in a foreign app ([ROADMAP.md:25], #18) | macOS / Linux (`.sh`), Windows (`.ps1`) |

## Latency (#16)

```sh
# macOS / Linux
scripts/verify-latency.sh [MODEL_PATH]
```
```powershell
# Windows (reference laptop)
pwsh scripts/verify-latency.ps1 [-ModelPath <path>]
```

Downloads `ggml-base.en` into `~/whisper-models` if no model is given, runs the
resident-model bench over the recorded 10 s fixture, and asserts p50 < 800 ms
(model load is excluded from the timing by design). Record the printed p50/p95 in
issue #16. Overrides: `VERBATIM_WHISPER_MODEL`, `VERBATIM_BENCH_ITERATIONS`,
`VERBATIM_BENCH_MAX_P50_MS`. Set `VERBATIM_BENCH_BASELINE=<runner>` to also arm the
20% regression gate against `benches/baselines/<runner>.json`.

## Injection (#18)

```sh
# macOS / Linux (auto-detects host)
scripts/verify-injection.sh
```
```powershell
# Windows
pwsh scripts/verify-injection.ps1
```

Runs the host platform's real-state seam E2E (gated behind `VERBATIM_<PLATFORM>_E2E`
because it touches the real clipboard/focus), then prints the manual real-keypress
checklist for that platform - including the GNOME/KDE portal `restore_token` and
Windows elevated-window UIPI cases. Record results in issue #18.

See [docs/M1_INJECTION_VERIFICATION.md](../docs/M1_INJECTION_VERIFICATION.md) for
the full two-layer verification strategy.

[ROADMAP.md:25]: ../docs/ROADMAP.md
[ROADMAP.md:27]: ../docs/ROADMAP.md
