// History list (UX.md 7): reverse-chron raw/polished pairs with copy-raw and
// clear-all. Refreshes live off the DictationRecorded bus event. No business
// logic - the Rust history store owns persistence and retention.
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import { UiEvent, VERBATIM_EVENT } from "../events";
import { clearHistory, HistoryEntry, listHistory } from "./commands";

function formatTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString();
}

export default function HistoryList() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [copied, setCopied] = useState<number | null>(null);

  const refresh = () => {
    listHistory().then(setEntries).catch(() => setEntries([]));
  };

  useEffect(() => {
    refresh();
    // A new dictation lands as DictationRecorded; pull the fresh list.
    const unlisten = listen<UiEvent>(VERBATIM_EVENT, ({ payload }) => {
      if (payload.type === "dictationRecorded") refresh();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const copyRaw = (entry: HistoryEntry) => {
    navigator.clipboard.writeText(entry.raw).then(
      () => {
        setCopied(entry.id);
        setTimeout(() => setCopied((id) => (id === entry.id ? null : id)), 1500);
      },
      () => {},
    );
  };

  const clear = () => {
    clearHistory().then(refresh).catch(() => {});
  };

  if (entries.length === 0) {
    return <p className="settings__hint">No dictations recorded yet.</p>;
  }

  return (
    <div className="history">
      <div className="history__header">
        <span className="settings__hint">{entries.length} recent</span>
        <button
          className="models__delete"
          aria-label="Clear all dictation history"
          onClick={clear}
        >
          Clear all
        </button>
      </div>
      <ul className="history__list">
        {entries.map((entry) => (
          <li key={entry.id} className="history__row">
            <div className="history__meta">
              <span className="models__sub">
                {entry.appId} · {formatTime(entry.createdAt)}
              </span>
              <button
                aria-label={`Copy raw transcript from ${entry.appId} at ${formatTime(entry.createdAt)}`}
                onClick={() => copyRaw(entry)}
              >
                {copied === entry.id ? "Copied" : "Copy raw"}
              </button>
            </div>
            <p className="history__raw">{entry.raw}</p>
            {entry.polished && entry.polished !== entry.raw && (
              <p className="history__polished">{entry.polished}</p>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
