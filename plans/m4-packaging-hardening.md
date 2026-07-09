# M4 - Packaging, hardening, v1.0

The milestone that turns a working product into a shippable one: signed installers on every channel, the second engine, a threat model, and a real dogfood that re-gates all of PRD section 7.

## Acceptance criteria (the gate)

From ROADMAP.md M4:

- [ ] All PRD section 7 success criteria pass, measured and recorded.
- [ ] Signed installers on all channels; clean-machine installs verified.
- [ ] Crash-free rate > 99.5% over a 2-week dogfood with >= 5 external testers across the three OSes.
- [ ] Security review of the injection IPC surface (trigger verbs only) done.

## Current state (verified 2026-07-08)

- M3 closed except the >= 80% polish-preference half of criterion 1, explicitly deferred to this milestone's dogfood.
- Release pipeline is spec'd (ENGINEERING.md 6: tag `v*` -> matrix builds -> sign + notarize -> package -> draft release with checksums + SBOM -> channel PRs) but not implemented.
- Engine registry supports multiple transcription engines; only whisper.cpp is real (`SherpaOnnx`/Parakeet is spec'd in ARCHITECTURE.md 4.x, unimplemented).
- `assets/models.json` schema already carries license + attribution string per model (ENGINEERING.md 5.1); no attribution UI renders it yet.
- IPC surface already matches the security posture (owner-only `0o600` socket, closed verb set `start`/`stop`/`toggle`/`status`, no `Cancel`); the formal review + threat-model doc do not exist.
- Latency-regression CI gates raw transcription and polish (per-runner p50 baselines, 20% limit).
- Carried-over M1 blockers: #18 real-keypress manual checklist (macOS foreground, Windows UIPI, GNOME Wayland portal + `restore_token`, KDE Plasma 6) and #16 reference-Windows-laptop latency measurement.

## Sources of truth (do not re-decide)

- ENGINEERING.md 6 (release pipeline shape) and 7 (per-OS signing: Developer ID + hardened runtime + notarytool, no App Sandbox ever; Authenticode OV-first; AppImage primary + Flathub + `.deb`, udev-rule helper ships in both Linux formats).
- ENGINEERING.md 5.1 (`models.json`: RAM/GPU recommendation tier + license/attribution fields; Parakeet CC-BY-4.0 renders in About and model manager).
- ENGINEERING.md security section (threat model is a v1.0 deliverable; injection misuse is the sensitive surface; `cargo deny` bans http clients outside the downloader).
- ARCHITECTURE.md 4.x engine registry (`SherpaOnnx` declares supported backends, automatic CPU fallback on GPU init failure - spike 3).
- PRD section 7 (the six success criteria this milestone measures for real) and PRD 136 (Parakeet attribution requirement).
- docs/M1_INJECTION_VERIFICATION.md (manual checklist the dogfood's cross-platform gate depends on).

## Cross-cutting decisions to surface before coding

- **Certificates are a human dependency**: Apple Developer ID and a Windows Authenticode OV cert must be purchased/issued by Tristen before Phase A can produce signed artifacts; CI wiring can land earlier against ad-hoc signing. Flag at kickoff, not mid-phase.
- **Dogfood telemetry**: crash-free rate needs a measurement mechanism that respects the no-network posture - local crash/session counters surfaced for manual tester report, not remote telemetry. Decide the exact shape in Phase E design, keep it opt-in-visible.
- **Parakeet scope**: sherpa-onnx is the second FFI surface and the biggest unknown; Phase C starts with a time-boxed spike (`spikes/`) before committing the trait impl.
- **Preference-benchmark corpus**: criterion 1's >= 80% blind preference needs real dictation samples collected during the dogfood, not synthetic fixtures; consent + storage stay local (testers submit transcript pairs manually).

## Phase 0 - unblock (no branch, existing issues)

Close the M1 carry-overs the dogfood gate depends on.

1. #18: run the real-keypress manual checklist (docs/M1_INJECTION_VERIFICATION.md) on Windows incl. UIPI, GNOME Wayland portal + `restore_token` silent reconnect, KDE Plasma 6.
2. #16: measure raw p50 on the reference Windows laptop; record in ROADMAP.md.

Verification: both issues closed with evidence recorded; M1 milestone closes.

## Phase A - branch `m4-phaseA-release-pipeline`

Release pipeline + signing + notarization (ENGINEERING.md 6/7).

1. Tag-triggered (`v*`) release workflow: matrix release builds for the five targets (macOS ARM + Intel, Windows x64, Linux AppImage + Flatpak).
2. macOS: Developer ID Application signing, hardened runtime, `notarytool` notarize + staple on the `.dmg`; stable signing identity from the first public build (TCC grants key to it).
3. Windows: Authenticode-signed `.msi` via WiX through tauri-bundler.
4. Draft GitHub release assembling artifacts + SHA-256 checksums + SBOM.
5. Cert material lives in CI secrets only; the workflow degrades to unsigned artifacts with a loud warning when secrets are absent (fork/PR safety).

Verification: a `v0.9.x` pre-release tag produces signed, notarized, stapled artifacts end to end; `spctl --assess` and Windows SmartScreen metadata verified on the outputs.

Anti-patterns: no cert material in the repo; no signing steps that silently skip.

## Phase B - branch `m4-phaseB-channels`

Distribution channels + clean-machine install verification.

1. Homebrew cask (tap repo) fed by the release workflow's channel PR step.
2. winget manifest PR automation.
3. Flathub manifest (libei portal path, Flatpak-clean per spike 1) + AppImage publishing; both ship the udev-rule helper script onboarding E9 references.
4. Clean-machine install matrix: fresh macOS/Windows/Ubuntu VMs, install from each channel, first dictation succeeds - recorded as the acceptance-criterion-2 evidence.

Verification: acceptance criterion 2 ticks with per-channel evidence.

## Phase C - branch `m4-phaseC-parakeet`

Parakeet engine (sherpa-onnx) + attribution surfaces + model recommendations.

1. Time-boxed spike: sherpa-onnx Rust bindings, Parakeet GGUF/ONNX load, transcribe the JFK fixture; go/no-go on binding choice.
2. `SherpaOnnx` `TranscriptionEngine` impl behind a feature flag, mirroring the whisper pattern (backend declaration, automatic CPU fallback on GPU init failure).
3. Attribution surfaces: About dialog + model manager render the license/attribution string from `models.json` (Parakeet CC-BY-4.0, PRD 136).
4. Model recommendations by hardware: model manager sorts/labels by the existing RAM/GPU tier fields plus the calibration ms/token probe already measured at onboarding.

Verification: feature build transcribes the fixture with Parakeet; default build stays fake-only and green; attribution text visible in both surfaces; recommendation ordering unit-tested against synthetic tier inputs.

Anti-patterns: no sherpa types across the trait boundary; `unsafe` only in FFI with `// SAFETY:` justifications.

## Phase D - branch `m4-phaseD-threat-model`

Threat-model doc + injection IPC security review (acceptance criterion 4).

1. `docs/THREAT_MODEL.md`: assets, trust boundaries, injection-capability misuse analysis (uinput/Accessibility), the no-other-process-can-inject invariant, model-download integrity, update-channel integrity.
2. Adversarial review of `ipc.rs` + socket setup: verb closure, pre-interpretation rejection, socket permissions, symlink/race hardening, fuzz the wire protocol with malformed payloads.
3. Fix anything found; add regression tests for each finding.

Verification: criterion 4 ticks; fuzz corpus committed; every finding has a test.

## Phase E - branch `m4-phaseE-dogfood` (LONG POLE - start once B lands)

Two-week external dogfood re-gating all of PRD section 7.

1. Recruit >= 5 external testers covering macOS, Windows 11, Ubuntu 24.04 (GNOME Wayland + KDE among them).
2. Local session/crash counters (no network) + a tester report template; crash-free session rate computed from submitted reports (criterion: > 99.5%).
3. Polish-preference blind panel: testers submit real dictation transcript pairs (raw vs polished, consented, locally collected); blind comparison scores the >= 80% preference half of M3/PRD criterion 1.
4. Onboarding timing: each tester's installer-download-to-first-dictation time recorded (< 5 min, no docs).
5. Latency spot-checks on tester hardware recorded against the PRD budgets (raw < 800 ms p50, polished < 1.5 s p50).
6. Triage cadence: dogfood bugs land as issues on this milestone; fixes ship to testers via pre-release tags through the Phase A pipeline.

Verification: acceptance criteria 1 and 3 tick with recorded evidence; PRD section 7 table filled in and committed to ROADMAP.md.

## Phase F - branch `m4-phaseF-docs`

End-user docs + README.

1. End-user docs site (static, versioned with the repo) covering install per channel, permissions per OS, dictionary/profiles, troubleshooting the E1-E10 catalog.
2. README rewrite for end users (current one is contributor-facing); screenshots from the M2 shell.

Verification: a dogfood tester can resolve a permission error from the docs alone; docs link ships in the About dialog.

(2026-07-08: content-first slice landed. `docs/site/` is an Astro Starlight site (config + package.json + content collection) with five pages - what-is / install / permissions / using / troubleshooting; the E1-E10 troubleshooting copy is kept in sync with `error_catalog.rs`. README rewritten end-user-first with a For-developers section below. About tab shows the docs address. Follow-ups: wire the Starlight build into CI + publish to Pages; one-click docs open in About needs the tauri opener plugin; screenshots pending the M2 shell captures.)

## Final phase - v1.0 sign-off

1. Re-check all four M4 criteria + the PRD section 7 table with evidence; tick in a closing PR.
2. Tag `v1.0.0`; the Phase A pipeline publishes all channels.
3. ROADMAP.md updated; post-v1 backlog becomes the planning surface.

Dependency order: Phase 0 and A first (0 unblocks E's cross-platform gate, A unblocks everything downstream). B after A. C and D parallel with B. E starts the moment B produces installable channel builds and runs >= 2 weeks. F parallel with E, done before E's testers need troubleshooting docs. Sign-off last.

## Kickoff decisions (resolved 2026-07-08)

- Certs: neither purchased yet.
  Phase A lands the full pipeline against ad-hoc/unsigned artifacts with a loud unsigned warning; cert secrets get wired when issued.
  Cert purchase (Apple Developer program + Authenticode OV) is Tristen's action item, tracked as a Phase A follow-up.
- Testers: recruit from friends/colleagues plus online communities (dictation/accessibility subreddits, HN, Discord); >= 5 total covering macOS, Windows 11, Ubuntu 24.04.
- Blind panel: testers cross-score each other's anonymized raw/polished pairs; target minimum ~50 pairs total for the >= 80% claim.
- Docs site: Astro Starlight, versioned with the repo (node toolchain already present via `ui/`).
