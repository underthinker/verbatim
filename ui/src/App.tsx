// Phase A hello webview: throwaway UI, real plumbing. Proves the Rust event
// bridge by rendering live session state driven over IPC (`verbatim trigger`).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import { stateLabel, UiEvent, VERBATIM_EVENT } from "./events";

export default function App() {
  const [state, setState] = useState<string>("(waiting for daemon)");
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    // Initial state comes from a command; everything after replays the bus.
    invoke<string>("session_state")
      .then((s) => setState((prev) => (prev === "(waiting for daemon)" ? s : prev)))
      .catch(() => setState("(daemon unavailable)"));

    const unlisten = listen<UiEvent>(VERBATIM_EVENT, ({ payload }) => {
      if (payload.type === "sessionTransition") {
        setState(stateLabel(payload.to));
        setLog((entries) =>
          [
            `#${payload.session}: ${stateLabel(payload.from)} -> ${stateLabel(payload.to)}`,
            ...entries,
          ].slice(0, 20),
        );
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <main style={{ fontFamily: "system-ui", padding: "2rem" }}>
      <h1>Verbatim shell</h1>
      <p>
        Session state: <strong>{state}</strong>
      </p>
      <p>
        <button onClick={() => invoke("trigger", { verb: "toggle" })}>
          Toggle dictation
        </button>
      </p>
      <ol reversed>
        {log.map((line, i) => (
          <li key={log.length - i}>{line}</li>
        ))}
      </ol>
    </main>
  );
}
