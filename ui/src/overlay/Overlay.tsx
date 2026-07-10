// The overlay pill (UX.md 2 overlay column, UX.md 7 surface spec).
//
// Pure presentation: state arrives on the overlay event channel from the Rust
// driver; there is no command invocation on this surface. Accessibility
// (UX.md 8): every state pairs a distinct icon/shape with its label - hue is
// never the only signal - and OS reduced-motion swaps the live waveform for a
// static level meter and disables animation.

import { useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

import {
  OVERLAY_EVENT,
  type ErrorPresentation,
  type OverlayEvent,
  type OverlayPhase,
  type PrimaryAction,
} from "./events";

/** Bars in the live waveform / static meter. */
const WAVEFORM_BARS = 24;

/**
 * The bus carries an honest linear RMS, which is the wrong scale to draw with:
 * ordinary speech sits near 0.03 and would lift a bar by 3% of its height,
 * reading as a flat line. Hearing is logarithmic, so the pill draws decibels.
 *
 * Floor is under a quiet room's noise (measured -51 to -58 dBFS on an Apple M5
 * internal mic); ceiling is where speech peaks before clipping.
 */
const LEVEL_FLOOR_DBFS = -60;
const LEVEL_CEIL_DBFS = -10;

/** Linear RMS in [0, 1] -> drawable level in [0, 1] on a decibel scale. */
export function normalizeLevel(rms: number): number {
  if (!(rms > 0)) return 0;
  const dbfs = 20 * Math.log10(rms);
  const span = LEVEL_CEIL_DBFS - LEVEL_FLOOR_DBFS;
  return Math.min(1, Math.max(0, (dbfs - LEVEL_FLOOR_DBFS) / span));
}

const PHASE_LABEL: Record<OverlayPhase, string> = {
  arming: "starting",
  recording: "listening",
  finalizing: "finishing",
  processing: "transcribing",
  success: "done",
  nothingHeard: "didn't catch anything",
  error: "something went wrong",
};

/** Button label per primary action - mirrors `PrimaryAction::label`
 * (crates/verbatim-app/src/error_catalog.rs). */
const ACTION_LABEL: Record<PrimaryAction["kind"], string> = {
  openMicPermission: "Open microphone settings",
  openModelManager: "Download model",
  retryTranscription: "Retry",
  pasteHint: "Paste anyway",
  openInputDevicePicker: "Choose microphone",
  resumeDownload: "Resume download",
  setUpTyping: "Set up typing",
  openPolishSettings: "Polish settings",
};

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (event: MediaQueryListEvent) => setReduced(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

function Waveform({
  levels,
  frozen,
  reducedMotion,
}: {
  levels: number[];
  frozen: boolean;
  reducedMotion: boolean;
}) {
  if (reducedMotion) {
    // Static level meter instead of a moving waveform (UX.md 8).
    const current = levels[levels.length - 1] ?? 0;
    return (
      <div className="meter" role="presentation">
        <div
          className="meter-fill"
          style={{ width: `${normalizeLevel(current) * 100}%` }}
        />
      </div>
    );
  }
  return (
    <div className={`waveform${frozen ? " frozen" : ""}`} role="presentation">
      {levels.map((rms, i) => (
        <div
          key={i}
          className="bar"
          style={{ transform: `scaleY(${0.08 + normalizeLevel(rms) * 0.92})` }}
        />
      ))}
    </div>
  );
}

/** Distinct icon/shape per state - never hue alone (UX.md 8). */
function PhaseIcon({ phase }: { phase: OverlayPhase }) {
  switch (phase) {
    case "arming":
      return (
        <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path
            d="M5 11a7 7 0 0 0 14 0M12 18v3"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>
      );
    case "recording":
      return (
        <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="7" />
        </svg>
      );
    case "finalizing":
    case "processing":
      return (
        <svg className="icon spinner" viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M12 3a9 9 0 1 1-9 9"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
          />
        </svg>
      );
    case "success":
      return (
        <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M4 12.5 10 18 20 6"
            fill="none"
            stroke="currentColor"
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      );
    case "nothingHeard":
      return (
        <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M4 12h3M10.5 12h3M17 12h3"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
          />
        </svg>
      );
    case "error":
      return (
        <svg className="icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M12 3 22 20H2Z" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
          <path d="M12 9v5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
          <circle cx="12" cy="17" r="1.2" />
        </svg>
      );
  }
}

export default function Overlay() {
  const [phase, setPhase] = useState<OverlayPhase>("arming");
  const [error, setError] = useState<ErrorPresentation | null>(null);
  const [levels, setLevels] = useState<number[]>(() =>
    Array<number>(WAVEFORM_BARS).fill(0),
  );
  const reducedMotion = usePrefersReducedMotion();
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  useEffect(() => {
    // The driver targets this window with `emit_to`; a window-scoped listener
    // is required to receive targeted events (a bare `listen` misses them).
    const unlisten = getCurrentWebviewWindow().listen<OverlayEvent>(OVERLAY_EVENT, ({ payload }) => {
      if (payload.kind === "phase") {
        setPhase(payload.phase);
        setError(payload.error);
        if (payload.phase === "arming") {
          setLevels(Array<number>(WAVEFORM_BARS).fill(0));
        }
      } else if (phaseRef.current === "recording") {
        setLevels((prev) => [...prev.slice(1), payload.rms]);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const showWaveform = phase === "recording" || phase === "finalizing";
  // In the error phase the pill speaks the catalog's plain copy (UX.md 4), not
  // a raw code; the primary action is offered as an explicit affordance.
  const isError = phase === "error" && error !== null;
  const label = isError ? error.copy : PHASE_LABEL[phase];

  return (
    <div
      className={`pill phase-${phase}`}
      role="status"
      aria-live="polite"
    >
      <PhaseIcon phase={phase} />
      {showWaveform ? (
        <Waveform
          levels={levels}
          frozen={phase === "finalizing"}
          reducedMotion={reducedMotion}
        />
      ) : (
        <span className="label">{label}</span>
      )}
      {isError && error.action !== null && (
        <span className="action">{ACTION_LABEL[error.action.kind]}</span>
      )}
    </div>
  );
}
