// Model manager (UX.md 7): list catalog models with installed state + size,
// total disk usage, download / delete / set-default. Download progress rides
// the same core bus event the overlay and onboarding use. No business logic -
// the Rust ModelManager owns filesystem + config.
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

import { UiEvent, VERBATIM_EVENT } from "../events";
import {
  deleteModel,
  downloadModel,
  formatBytes,
  listModels,
  ManagedModel,
  modelsDiskUsage,
  setDefaultModel,
} from "./commands";

interface Progress {
  received: number;
  total: number | null;
}

export default function ModelsTab() {
  const [models, setModels] = useState<ManagedModel[]>([]);
  const [diskUsage, setDiskUsage] = useState(0);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listModels().then(setModels).catch(() => setModels([]));
    modelsDiskUsage().then(setDiskUsage).catch(() => setDiskUsage(0));
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<UiEvent>(VERBATIM_EVENT, ({ payload }) => {
      if (payload.type === "downloadProgress") {
        setProgress((prev) => ({
          ...prev,
          [payload.modelId]: {
            received: payload.receivedBytes,
            total: payload.totalBytes,
          },
        }));
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [refresh]);

  const download = (id: string) => {
    setError(null);
    downloadModel(id)
      .then(() => {
        setProgress((prev) => {
          const next = { ...prev };
          delete next[id];
          return next;
        });
        refresh();
      })
      .catch((reason: string) => setError(`Download failed: ${reason}`));
  };

  const remove = (id: string) => {
    setError(null);
    deleteModel(id)
      .then(refresh)
      .catch((reason: string) => setError(String(reason)));
  };

  const makeDefault = (id: string) => {
    setError(null);
    setDefaultModel(id)
      .then(refresh)
      .catch((reason: string) => setError(String(reason)));
  };

  return (
    <div className="models">
      <p className="settings__row">
        <span>Disk used by models</span>
        <strong>{formatBytes(diskUsage)}</strong>
      </p>
      {error && (
        <p className="settings__error" role="alert">
          {error}
        </p>
      )}
      <ul className="models__list">
        {models.map((model) => {
          const p = progress[model.id];
          const downloading = p !== undefined;
          return (
            <li key={model.id} className="models__row">
              <div className="models__meta">
                <strong>{model.name}</strong>
                <span className="models__sub">
                  {model.kind} · {formatBytes(model.sizeBytes)}
                  {model.installed && " · installed"}
                  {model.isDefault && (
                    <span className="models__badge"> Default</span>
                  )}
                </span>
                {downloading && (
                  <progress
                    className="models__progress"
                    value={p.total ? p.received : undefined}
                    max={p.total ?? undefined}
                    aria-label={`Downloading ${model.name}`}
                  />
                )}
              </div>
              <div className="models__actions">
                {model.installed ? (
                  <>
                    {!model.isDefault && (
                      <button onClick={() => makeDefault(model.id)}>
                        Set default
                      </button>
                    )}
                    <button
                      className="models__delete"
                      onClick={() => remove(model.id)}
                    >
                      Delete
                    </button>
                  </>
                ) : (
                  <button
                    onClick={() => download(model.id)}
                    disabled={downloading}
                  >
                    {downloading ? "Downloading..." : "Download"}
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
