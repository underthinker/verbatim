// First-run onboarding flow (UX.md 6). Six screens, progress dots, all
// skippable-but-discouraged except the two permission steps. The load-bearing
// logic lives in the Rust onboarding service (commands.ts wraps it); this file
// is sequencing and presentation only.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

import { stateLabel, UiEvent, VERBATIM_EVENT } from "../events";
import {
  Capability,
  ModelInfo,
  PermissionState,
  complete,
  downloadModel,
  formatBytes,
  openSettings,
  permission,
  recommendedModel,
  requestPermission,
} from "./commands";

const STEPS = [
  "welcome",
  "microphone",
  "typing",
  "model",
  "try",
  "polish",
] as const;
type Step = (typeof STEPS)[number];

/** A permission counts as satisfied for advancing when it is granted or the OS
 * does not require it (UX.md 6: permission steps are the only hard gates). */
function isSatisfied(state: PermissionState): boolean {
  return state === "Granted" || state === "NotNeeded";
}

function ProgressDots({ index }: { index: number }) {
  return (
    <div
      className="dots"
      role="img"
      aria-label={`Step ${index + 1} of ${STEPS.length}`}
    >
      {STEPS.map((step, i) => (
        <span
          key={step}
          className={`dot${i === index ? " active" : ""}${i < index ? " done" : ""}`}
        />
      ))}
    </div>
  );
}

function PermissionStep({
  capability,
  title,
  body,
  onSatisfied,
}: {
  capability: Capability;
  title: string;
  body: string;
  onSatisfied: (satisfied: boolean) => void;
}) {
  const [state, setState] = useState<PermissionState | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    permission(capability)
      .then((s) => {
        setState(s);
        onSatisfied(isSatisfied(s));
      })
      .catch(() => undefined);
  }, [capability, onSatisfied]);

  const request = useCallback(async () => {
    setBusy(true);
    try {
      const next = await requestPermission(capability);
      setState(next);
      onSatisfied(isSatisfied(next));
    } finally {
      setBusy(false);
    }
  }, [capability, onSatisfied]);

  const satisfied = state !== null && isSatisfied(state);

  return (
    <div className="screen">
      <h1>{title}</h1>
      <p>{body}</p>
      {satisfied ? (
        <p className="ok" role="status">
          <span className="tick" aria-hidden="true">
            &#10003;
          </span>
          {state === "NotNeeded" ? "Nothing needed here." : "Granted."}
        </p>
      ) : (
        <div className="actions">
          <button className="primary" onClick={request} disabled={busy}>
            {busy ? "Waiting..." : "Grant access"}
          </button>
          <button className="link" onClick={() => openSettings(capability)}>
            Open settings
          </button>
        </div>
      )}
    </div>
  );
}

function ModelStep({ onReady }: { onReady: () => void }) {
  const [model, setModel] = useState<ModelInfo | null>(null);
  const [received, setReceived] = useState<number | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    recommendedModel().then(setModel).catch(() => undefined);
    const unlisten = listen<UiEvent>(VERBATIM_EVENT, ({ payload }) => {
      if (payload.type === "downloadProgress") {
        setReceived(payload.receivedBytes);
        setTotal(payload.totalBytes);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const start = useCallback(async () => {
    if (model === null) return;
    setError(null);
    try {
      await downloadModel(model.id);
      setDone(true);
      onReady();
    } catch (err) {
      setError(String(err));
    }
  }, [model, onReady]);

  const pct =
    received !== null && total !== null && total > 0
      ? Math.round((received / total) * 100)
      : 0;

  return (
    <div className="screen">
      <h1>Download a speech model</h1>
      {model === null ? (
        <p>Checking your hardware...</p>
      ) : (
        <>
          <p>
            Recommended for this computer: <strong>{model.name}</strong> (
            {formatBytes(model.sizeBytes)}).
          </p>
          {done ? (
            <p className="ok" role="status">
              <span className="tick" aria-hidden="true">
                &#10003;
              </span>
              Ready - you can keep going.
            </p>
          ) : received === null ? (
            <div className="actions">
              <button className="primary" onClick={start}>
                Download
              </button>
            </div>
          ) : (
            <>
              <div
                className="bar"
                role="progressbar"
                aria-valuenow={pct}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div className="bar-fill" style={{ transform: `scaleX(${pct / 100})` }} />
              </div>
              <p className="muted">{pct}%</p>
            </>
          )}
          {error !== null && (
            <div className="actions">
              <p className="err" role="alert">
                Download interrupted.
              </p>
              <button className="primary" onClick={start}>
                Resume download
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function TryItStep() {
  const [state, setState] = useState<string>("idle");

  useEffect(() => {
    const unlisten = listen<UiEvent>(VERBATIM_EVENT, ({ payload }) => {
      if (payload.type === "sessionTransition") {
        setState(stateLabel(payload.to));
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <div className="screen">
      <h1>Try it</h1>
      <p>Hold your hotkey and say anything - the text appears below.</p>
      <textarea
        className="tryfield"
        placeholder="Your dictation lands here..."
        aria-label="Dictation practice field"
      />
      <div className="actions">
        <button className="primary" onClick={() => invoke("trigger", { verb: "toggle" })}>
          Start / stop dictation
        </button>
        {/* The only feedback this step gives is the state word; announce it. */}
        <span className="muted" role="status">
          state: {state}
        </span>
      </div>
    </div>
  );
}

function PolishStep({ onFinish }: { onFinish: (withPolish: boolean) => void }) {
  return (
    <div className="screen">
      <h1>Polish (optional)</h1>
      <p>Verbatim can tidy filler words and punctuation before it types.</p>
      <div className="example">
        <p className="muted">raw</p>
        <p>um so like the thing is basically done i think</p>
        <p className="muted">polished</p>
        <p>So, the thing is basically done, I think.</p>
      </div>
      <div className="actions">
        <button className="primary" onClick={() => onFinish(true)}>
          Enable polish
        </button>
        <button className="secondary" onClick={() => onFinish(false)}>
          Skip for now
        </button>
      </div>
    </div>
  );
}

export default function Onboarding() {
  const [index, setIndex] = useState(0);
  const [micOk, setMicOk] = useState(false);
  const [typingOk, setTypingOk] = useState(false);
  const step: Step = STEPS[index];
  const rootRef = useRef<HTMLElement>(null);

  // Advancing a step swaps the whole screen but leaves focus on the (now
  // re-labelled) Continue button, so a screen reader never announces the new
  // screen. Move focus to the step's heading instead (UX.md 8). The heading is
  // made focusable on the fly rather than every screen carrying a tabIndex it
  // only needs for this. Focus only ever moves when the step actually changes:
  // keying off "have I run before" would steal focus on mount, because
  // StrictMode invokes the effect twice.
  const focusedIndex = useRef(index);
  useEffect(() => {
    if (focusedIndex.current === index) return;
    focusedIndex.current = index;
    const heading = rootRef.current?.querySelector("h1");
    if (heading) {
      heading.tabIndex = -1;
      heading.focus();
    }
  }, [index]);

  const next = useCallback(() => setIndex((i) => Math.min(i + 1, STEPS.length - 1)), []);
  const back = useCallback(() => setIndex((i) => Math.max(i - 1, 0)), []);

  const finish = useCallback((withPolish: boolean) => {
    complete(null, withPolish ? "polish-qwen2.5-0.5b" : null).catch(() => undefined);
  }, []);

  // Permission steps are the only hard gates (UX.md 6).
  const canAdvance =
    step === "microphone" ? micOk : step === "typing" ? typingOk : true;

  return (
    <main className="onboarding" ref={rootRef}>
      {step === "welcome" && (
        <div className="screen">
          <h1>Welcome to Verbatim</h1>
          <p>Press a hotkey, speak, and your words are typed into any app.</p>
          <p className="privacy">
            Everything runs on this computer. Nothing is ever uploaded.
          </p>
        </div>
      )}
      {step === "microphone" && (
        <PermissionStep
          capability="microphone"
          title="Microphone"
          body="Verbatim needs your microphone to hear you. Nothing is recorded until you press your hotkey."
          onSatisfied={setMicOk}
        />
      )}
      {step === "typing" && (
        <PermissionStep
          capability="textInjection"
          title="Typing"
          body="Verbatim types the transcribed text into whatever app you're using."
          onSatisfied={setTypingOk}
        />
      )}
      {step === "model" && <ModelStep onReady={() => undefined} />}
      {step === "try" && <TryItStep />}
      {step === "polish" && <PolishStep onFinish={finish} />}

      <ProgressDots index={index} />

      <nav className="nav">
        {index > 0 && step !== "polish" && (
          <button className="link" onClick={back}>
            Back
          </button>
        )}
        {step !== "polish" && (
          <>
            {/* A disabled button is skipped by Tab, so the reason it is disabled
                has to be readable from the step itself. */}
            {!canAdvance && (
              <span id="gate-reason" className="sr-only">
                Grant this permission to continue.
              </span>
            )}
            <button
              className="primary"
              onClick={next}
              disabled={!canAdvance}
              aria-describedby={canAdvance ? undefined : "gate-reason"}
            >
              {step === "welcome" ? "Get started" : "Continue"}
            </button>
          </>
        )}
      </nav>
    </main>
  );
}
