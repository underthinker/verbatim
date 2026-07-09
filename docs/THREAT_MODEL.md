# Verbatim - Threat Model

Status: v1.0 deliverable (M4 Phase D).
Scope: the security-sensitive surfaces of a fully-local dictation daemon that can type into any focused application.
This document is authoritative for security decisions; it complements ENGINEERING.md section 8 (posture) and the M4 acceptance criterion "security review of the injection IPC surface done".

## 1. What we are protecting and against whom

Verbatim's defining capability is also its defining risk: it can synthesize keystrokes and paste into whatever window has focus, using OS input-injection privileges (macOS Accessibility, Linux `uinput`/libei, Windows `SendInput`).
The one invariant everything else serves:

> **No process other than Verbatim's own hotkey/CLI path can cause text to be injected into the user's session.**

If that invariant holds, a compromised or malicious peer can at worst start and stop recordings; it can never turn Verbatim into a keystroke-injection gadget for arbitrary text.

### Assets

| Asset | Why it matters |
|---|---|
| Injection capability | The daemon holds standing OS permission to type into any app; abused, it is arbitrary input injection into the user's session. |
| Microphone capture | Recordings contain whatever the user says; a peer that can `start` recording at will is a privacy problem even without reading the audio. |
| Transcript / history data | Everything ever dictated, stored locally; discloses content and usage patterns. |
| Model files | Executed indirectly by the inference engines (FFI); a swapped model is code the engine trusts. |
| The Verbatim binary + update channel | A tampered update runs with the user's injection privileges on next launch. |

### Adversaries (in scope)

- **A same-machine, same-UID process** the user did not intend to grant control (another app they run, a compromised dependency in some unrelated program).
  This is the primary adversary the IPC surface defends against.
- **A same-machine, different-UID local user** (shared workstation).
- **A network attacker** who can tamper with model downloads or update artifacts in transit.

### Out of scope

- **Root / an Administrator / another process already running as the user with a debugger.**
  A same-UID process with `ptrace`/task-port/debug rights, or root, is inside the trusted computing base: it can read the daemon's memory, drive the OS input APIs directly, or replace the binary.
  Verbatim cannot defend the user against code that already has the user's full authority; the IPC hardening below raises the bar for *ordinary* same-UID processes, not for a debugger-equipped one.
- **Physical attacks, evil-maid, firmware, side channels.**
- **The correctness of the OS injection permission itself** (we rely on the platform's Accessibility/`uinput` gate being sound).

## 2. Trust boundaries

```
  malicious/unaware peer process        network (model host, update channel)
            │                                        │
            ▼  IPC (Unix socket / named pipe)        ▼  HTTPS + pinned hash / signature
   ┌─────────────────────────────────────────────────────────┐
   │  Verbatim daemon (user's UID)                            │
   │   - trigger endpoint: CLOSED verb set, no text frame     │
   │   - SessionRunner: hotkey/CLI -> record -> ASR -> inject │
   │   - model store (hash-verified), history (0600), logs    │
   └─────────────────────────────────────────────────────────┘
            │
            ▼  OS input injection (Accessibility / uinput / SendInput)
       the user's focused application
```

Two boundaries carry the weight:

1. **The IPC boundary** between a connecting peer and the daemon (section 3).
2. **The network boundary** between the daemon and the model/update hosts (section 5).

The injection boundary below the daemon is a *capability we hold*, not an attack surface a peer reaches directly - the only way to reach it is through the SessionRunner, which is only ever driven by the local hotkey, the local CLI trigger, or the closed IPC verb set.

## 3. The injection / IPC surface (primary review target)

### 3.1 Design invariant: the protocol cannot carry text

The trigger protocol (`crates/verbatim-app/src/ipc.rs`) is a closed set of four tokens - `start`, `stop`, `toggle`, `status` - each a bare newline-terminated line.
There is deliberately **no frame anywhere in the protocol that carries a text payload.**
This is the structural reason a peer cannot inject text through us: even a peer that speaks the protocol perfectly can only nudge the state machine, and the text that gets injected is always and only what Verbatim itself transcribed from the microphone.
`Request::parse` rejects anything outside the four tokens, returning the offending token as an *error to log*, never as data to act on (`daemon.rs` `handle_connection`).
`Cancel` is intentionally absent: discarding a dictation is a local ESC action, never a remotely triggerable one.

### 3.2 Access control on the endpoint

- **Unix (macOS, Linux):** the socket is a Unix domain socket created under the per-user runtime directory and `chmod`ed to `0o600` (`transport.rs` `restrict_to_owner`).
  Only the owning UID can connect.
  A leftover socket from a previous run is removed before bind; the `remove_file` targets the path entry itself (a symlink is unlinked, not followed), and the containing directory is created under the user-private runtime dir (`XDG_RUNTIME_DIR`, mode `0700`, or the macOS Application Support dir).
- **Windows:** a named pipe `\\.\pipe\verbatim-<username>`.
  Named pipes live in a flat namespace, so per-user scoping comes from the name plus the pipe's security descriptor.
  tokio creates the pipe with NULL security attributes, which yields the process token's **default DACL** - full control to the creating user and `LocalSystem`, and access to `Administrators`.
  Crucially the default DACL does **not** grant `Everyone`; a different, non-admin local user therefore cannot open the pipe.
  See finding F3 for the residual (admin-only) exposure and why it is accepted.

### 3.3 Findings from the adversarial review

Reviewed: `ipc.rs` (parser + wire format), `transport.rs` (socket/pipe bind + permissions), `daemon.rs` `handle_connection` (read loop + dispatch).

| ID | Finding | Severity | Disposition |
|---|---|---|---|
| F1 | `read_line` was unbounded: a same-UID peer holding a connection open and streaming bytes with no newline could grow the daemon's read buffer without limit (memory-exhaustion DoS). | Low (same-UID) | **Fixed.** Read is capped at `MAX_REQUEST_BYTES` (64) via a `Take` adapter; the truncated line is not a verb, so it is rejected. Regression: `oversized_payload_is_bounded_and_rejected`. |
| F2 | No read timeout: a peer that connected and never sent a request line pinned a spawned task indefinitely (half-open-connection / task-leak DoS). | Low (same-UID) | **Fixed.** The request read is wrapped in a 5 s timeout; a silent peer is dropped with no reply. Regression: `silent_client_times_out_without_hanging`. |
| F3 | Windows named pipe uses the default DACL, which additionally grants `Administrators`. A local admin could send trigger verbs (start/stop/toggle/status). | Low | **Accepted, documented.** An admin is already in the TCB (can drive `SendInput` directly, replace the binary, read process memory); an explicit owner-only DACL would exclude admins but buys nothing against an actor who already has the user's authority. It would also require `unsafe` security-descriptor FFI, which the coding standard bars from `verbatim-app` (allowed only in `verbatim-platform`/engine FFI). Revisit only if the pipe ever needs to be reachable by a lower-privileged helper. |
| F4 | Parser robustness against malformed / injection-shaped payloads. | - | **No defect; test added.** Binary/non-UTF-8 input cannot even reach the parser - `read_line` errors on invalid UTF-8 first. A committed corpus (`parser_never_panics_and_only_the_closed_set_parses`) asserts the parser never panics and accepts nothing outside the four exact verbs, including injection-shaped strings, embedded control characters, and overlong inputs. |

### 3.4 Residual risks on this surface (accepted)

- A same-UID process can start/stop/toggle recordings and read status.
  This is inherent: a same-UID process has the user's authority and could drive the microphone or the OS injection APIs directly regardless of Verbatim.
  The closed verb set guarantees the *worst* such a peer can do through Verbatim is toggle recording state - never inject chosen text - which is strictly less than it can already do on its own.
- Denial of service by a same-UID peer (spawning many connections) is possible but uninteresting: the same peer can `kill` the daemon outright.
  The F1/F2 fixes exist for robustness against buggy/wedged clients and defense-in-depth, not because the DoS meaningfully raises the attacker's power.

## 4. Injection-capability misuse analysis

The capability (Accessibility / `uinput` / `SendInput`) is reachable only through `SessionRunner`, and `SessionRunner` is driven by exactly three sources, all local:

1. the OS global hotkey (user's physical key),
2. the local `verbatim trigger` CLI (which itself speaks the closed IPC protocol), and
3. the closed IPC verb set.

None of these can specify the injected text; the text is always the transcript of live microphone audio.
There is no code path from "bytes arriving on the socket" to "those bytes are typed" - the bytes are parsed into one of four verbs and discarded otherwise.
This is the concrete mechanization of the section-1 invariant, and it is enforced structurally (by the absence of a text frame) rather than by validation that could be bypassed.

On Linux specifically, the `uinput` fallback path requires a udev rule granting the user access to `/dev/uinput`; that grant widens the user's own ability to synthesize input but does not expose Verbatim's socket to other users, and is documented in onboarding (E9) as a deliberate, user-consented step.

## 5. Model-download and update-channel integrity

### 5.1 Model downloads

Model acquisition is the only sanctioned outbound network code (ENGINEERING.md 8; `cargo deny` bans HTTP-client crates elsewhere in the tree).
Every catalog entry pins a lowercase-hex SHA-256 (`crates/verbatim-engines/src/model.rs`, `ModelSpec::sha256`), and the `ModelDownloader` trait contract is that a model is hash-verified against that pin **before** it is activated for use.
A tampered or truncated download fails the hash check and is never handed to an engine.
Downloads use fixed HTTPS hosts; partial files resume via Range but are still verified whole before activation.

Current state (honest): the real network downloader is not yet implemented - the daemon runs on `FakeModelDownloader`, and several catalog `sha256` fields are still empty placeholders pending the real artifact hashes.
The invariant is a *contract on the downloader seam*; it becomes load-bearing when the real downloader lands, at which point the pinned hashes must be populated and a "reject on hash mismatch" test must gate it.
Until then, no un-verified model is fetched because no model is fetched over the network at all.

### 5.2 Update channels

Distribution rides platform-native, signed channels rather than a bespoke updater (ENGINEERING.md 6/7):
Developer ID + notarization on macOS, Authenticode on Windows, and the OS package managers / Flathub / signed AppImage on Linux.
Update integrity therefore inherits each platform's signature verification; Verbatim ships no self-update code of its own that could be tricked into installing an unsigned artifact.
The release pipeline publishes SHA-256 checksums and an SBOM alongside each artifact (M4 Phase A) so a manually-downloaded artifact can be verified out of band.
Residual: the platform trust roots and the signing keys themselves; key custody is a release-process concern (CI secrets only, never in the repo).

## 6. Data-at-rest

All user data stays in the platform data directory; there is no cloud sync.
History and config are owner-readable only.
Temporary audio is deleted after transcription and swept on startup (ENGINEERING.md 8).
Compromise of the user account discloses this data, but that is the same TCB boundary as everywhere else in this document - not a separately defensible layer.

## 7. Review checklist status (M4 criterion 4)

- [x] Injection-capability misuse analyzed; the no-other-process-can-inject invariant stated and traced to its structural enforcement (section 3.1, 4).
- [x] IPC wire protocol adversarially reviewed; verb closure and pre-interpretation rejection confirmed (section 3.1).
- [x] Socket / pipe access control reviewed; Unix `0o600` confirmed, Windows default-DACL exposure analyzed (section 3.2, F3).
- [x] Malformed-payload handling fuzzed with a committed corpus; parser proven panic-free and closed-set (F4).
- [x] Findings fixed with regression tests (F1, F2) or explicitly accepted with rationale (F3).
- [x] Model-download and update-channel integrity documented, including the current fake-downloader caveat (section 5).
</content>
</invoke>
