# Verification scripts

Turnkey scripts for the acceptance criteria that need a real desktop session.
They wrap the same checks CI runs (bench, seam E2E) plus the manual checklist
that only a live desktop can complete, so validating on real hardware is one
command per platform.

| Script | Criterion | Platforms |
| --- | --- | --- |
| `verify-latency.{sh,ps1}` | p50 raw latency < 800 ms, resident model ([ROADMAP.md:27], #16) | macOS / Linux (`.sh`), Windows (`.ps1`) |
| `verify-injection.{sh,ps1}` | dictation lands text in a foreign app ([ROADMAP.md:25], #18) | macOS / Linux (`.sh`), Windows (`.ps1`) |
| `verify-overlay.sh` | overlay never takes focus (M2) | macOS (machine-checked), Linux (checklist) |

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

On macOS the two "text lands in the editor" boxes are machine-checkable:

```sh
VERBATIM_TEXTEDIT_E2E=1 scripts/verify-injection.sh
```

This drives a real TextEdit document through `verbatim inject-selftest` and reads
the result back over Apple Events, instead of asking the operator to confirm by
eye. It derives its expectation from the receipt the injector reports, so the same
run ticks the granted box (text lands via `TransientPasteboardPaste`, prior
clipboard restored) or the revoked one (nothing typed, text staged on the
clipboard, E4). It takes over TextEdit for a few seconds and refuses to inject
unless TextEdit is genuinely frontmost, so leave the machine alone while it runs.

This check is what caught the paste/restore race that made a cold target receive
the user's previous clipboard content instead of the dictation.

See [docs/M1_INJECTION_VERIFICATION.md](../docs/M1_INJECTION_VERIFICATION.md) for
the full two-layer verification strategy.

## Overlay focus (M2)

```sh
scripts/verify-overlay.sh
```

Drives a real dictation session on the default fake capture/engine backends - no
microphone, model, or permission grant needed - and asserts the pill maps on
screen while the editor keeps the focus.

On macOS every box is machine-checked: the overlay must appear (a hidden window
would make the focus assertion vacuous), Verbatim must never become the frontmost
app, and the pill must never become the main window. `AXFocused` is deliberately
not one of the assertions: it reads `false` for every window of an inactive app,
so it stays green even for an overlay that did become key. `AXMain` and the app's
own `AXFocusedWindow` are the properties that actually flip, and the check is
calibrated against a deliberately `focusable(true)` build to prove it can fail.

The script needs an unlocked screen and Accessibility for the terminal; without
either, window enumeration returns nothing, and it exits 2 rather than reporting
a focus failure it never observed.

On Linux the same criterion stays a manual checklist (GNOME and KDE Plasma 6),
since KDE focus-steal is the spike-1 regression this criterion exists to guard.

The reduced-motion half of the criterion has no script: `overlay.css` stills the
pill under one `prefers-reduced-motion: reduce` media query covering every
descendant, so a newly added animation cannot escape it.

[ROADMAP.md:25]: ../docs/ROADMAP.md
[ROADMAP.md:27]: ../docs/ROADMAP.md
