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

const STEP_META: Record<Step, { label: string; detail: string }> = {
  welcome: { label: "Welcome", detail: "Meet Verbatim" },
  microphone: { label: "Microphone", detail: "Hear your voice" },
  typing: { label: "Typing", detail: "Write in any app" },
  model: { label: "Speech model", detail: "Choose local AI" },
  try: { label: "Try it", detail: "First dictation" },
  polish: { label: "Writing style", detail: "Make it yours" },
};

/** A permission counts as satisfied for advancing when it is granted or the OS
 * does not require it (UX.md 6: permission steps are the only hard gates). */
function isSatisfied(state: PermissionState): boolean {
  return state === "Granted" || state === "NotNeeded";
}

function ProgressRail({ index }: { index: number }) {
  return (
    <ol className="step-rail" aria-label={`Step ${index + 1} of ${STEPS.length}`}>
      {STEPS.map((step, i) => (
        <li key={step} className={`${i === index ? "active" : ""}${i < index ? " done" : ""}`}>
          <span className="step-rail__number" aria-hidden="true">{i < index ? "✓" : i + 1}</span>
          <span>
            <strong>{STEP_META[step].label}</strong>
            <small>{STEP_META[step].detail}</small>
          </span>
        </li>
      ))}
    </ol>
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
      <div className="screen__eyebrow">Permission</div>
      <h1>{title}</h1>
      <p>{body}</p>
      <div className="permission-card">
        <span className="permission-card__icon" aria-hidden="true">
          {capability === "microphone" ? "◉" : "⌨"}
        </span>
        <span>
          <strong>{capability === "microphone" ? "Microphone access" : "Accessibility access"}</strong>
          <small>{satisfied ? "Ready to use" : "Required to continue"}</small>
        </span>
        <span className={`permission-card__status${satisfied ? " ready" : ""}`}>
          {satisfied ? "Granted" : "Not granted"}
        </span>
      </div>
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
      <div className="screen__eyebrow">Local intelligence</div>
      <h1>Download a speech model</h1>
      {model === null ? (
        <p>Checking your hardware...</p>
      ) : (
        <>
          <p>We picked the best balance of speed and accuracy for this Mac.</p>
          <div className="model-choice">
            <span className="model-choice__icon" aria-hidden="true">V</span>
            <span>
              <strong>{model.name}</strong>
              <small>{formatBytes(model.sizeBytes)} · Runs entirely on-device</small>
            </span>
            <span className="model-choice__badge">Recommended</span>
          </div>
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
      <div className="screen__eyebrow">Test drive</div>
      <h1>Try it</h1>
      <p>Hold your hotkey and say anything. This is a safe place to get the feel of it.</p>
      <textarea
        className="tryfield"
        placeholder="Your first dictation will appear here…"
        aria-label="Dictation practice field"
      />
      <div className="actions">
        <button className="primary" onClick={() => invoke("trigger", { verb: "toggle" })}>
          Start a test recording
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
      <div className="screen__eyebrow">Optional</div>
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
      <header className="onboarding__brand">
        <span className="onboarding__brand-mark" aria-hidden="true">V</span>
        <span><strong>Verbatim</strong><small>Private dictation for your Mac</small></span>
      </header>
      <div className="onboarding__body">
        <aside><ProgressRail index={index} /></aside>
        <section className="onboarding__content">
          {step === "welcome" && (
            <div className="screen screen--welcome">
              <div className="welcome-orb" aria-hidden="true"><span>V</span></div>
              <div className="screen__eyebrow">Welcome</div>
              <h1>Your voice, instantly in writing.</h1>
              <p>Press a hotkey, speak naturally, and Verbatim types polished text into any app.</p>
              <div className="privacy-card">
                <span aria-hidden="true">⌁</span>
                <span><strong>Private by design</strong><small>Audio and text never leave this Mac.</small></span>
              </div>
            </div>
          )}
          {step === "microphone" && (
            <PermissionStep
              capability="microphone"
              title="Let Verbatim hear you"
              body="Microphone access is used only while you are actively dictating."
              onSatisfied={setMicOk}
            />
          )}
          {step === "typing" && (
            <PermissionStep
              capability="textInjection"
              title="Type into any application"
              body="Accessibility lets Verbatim place your finished transcript exactly where your cursor is."
              onSatisfied={setTypingOk}
            />
          )}
          {step === "model" && <ModelStep onReady={() => undefined} />}
          {step === "try" && <TryItStep />}
          {step === "polish" && <PolishStep onFinish={finish} />}
        </section>
      </div>

      <nav className="nav">
        <span className="nav__count">{index + 1} of {STEPS.length}</span>
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
              {step === "welcome" ? "Set up Verbatim" : "Continue"}
            </button>
          </>
        )}
      </nav>
    </main>
  );
}
