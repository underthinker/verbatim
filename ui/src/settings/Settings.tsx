// The Settings surface (UX.md 7): the steady-state main window. Tabs bound to
// the persisted config via Tauri commands. No business logic - the Rust side
// validates the hotkey and owns every default.
import { useEffect, useRef, useState } from "react";

import {
  Config,
  getConfig,
  getInputMonitoringPermission,
  listModels,
  ManagedModel,
  openInputMonitoringSettings,
  openDocs,
  setConfig,
  validateHotkey,
} from "./commands";
import HistoryList from "./HistoryList";
import ModelsTab from "./ModelsTab";

const TABS = ["General", "Dictation", "Polish", "Models", "History", "About"] as const;
type Tab = (typeof TABS)[number];

const TAB_COPY: Record<Tab, { title: string; detail: string; glyph: string }> = {
  General: { title: "Shortcuts", detail: "Choose how you start and stop dictation.", glyph: "⌘" },
  Dictation: { title: "Dictation", detail: "Your active speech engine and language settings.", glyph: "◉" },
  Polish: { title: "Writing style", detail: "Shape the text Verbatim produces for you.", glyph: "✦" },
  Models: { title: "Models", detail: "Manage the local intelligence stored on this Mac.", glyph: "⬡" },
  History: { title: "History", detail: "Review and recover recent transcriptions.", glyph: "↺" },
  About: { title: "About Verbatim", detail: "Privacy, help, and open-source acknowledgements.", glyph: "i" },
};

const RIGHT_MODIFIER =
  /^(right(option|alt|command|cmd|control|ctrl|shift)|r(alt|opt|cmd|ctrl|shift))$/i;

export default function Settings() {
  const [config, setLocal] = useState<Config | null>(null);
  const [models, setModels] = useState<ManagedModel[]>([]);
  const [tab, setTab] = useState<Tab>("General");
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [inputMonitoring, setInputMonitoring] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [newTerm, setNewTerm] = useState("");
  const [newAppId, setNewAppId] = useState("");
  const [newProfile, setNewProfile] = useState("raw");
  const [docsFailed, setDocsFailed] = useState(false);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Roving arrow-key navigation between tabs (WAI-ARIA tablist, UX.md 8).
  // Home/End jump to the ends, as the pattern requires.
  const onTabKey = (e: React.KeyboardEvent, i: number) => {
    const next =
      e.key === "ArrowRight" ? (i + 1) % TABS.length
      : e.key === "ArrowLeft" ? (i - 1 + TABS.length) % TABS.length
      : e.key === "Home" ? 0
      : e.key === "End" ? TABS.length - 1
      : -1;
    if (next === -1) return;
    e.preventDefault();
    setTab(TABS[next]);
    tabRefs.current[next]?.focus();
  };

  useEffect(() => {
    getConfig().then(setLocal).catch(() => setLocal(null));
    listModels().then(setModels).catch(() => setModels([]));
  }, []);

  useEffect(() => {
    if (!config || !RIGHT_MODIFIER.test(config.hotkey.trim())) {
      setInputMonitoring(null);
      return;
    }
    getInputMonitoringPermission()
      .then(setInputMonitoring)
      .catch(() => setInputMonitoring(null));
  }, [config?.hotkey]);

  if (!config) {
    return (
      <main className="settings settings--loading" role="status" aria-busy="true">
        Loading settings...
      </main>
    );
  }

  // One updater for every field; edits are local until Save.
  const patch = (next: Partial<Config>) => {
    setLocal({ ...config, ...next });
    setSaved(false);
  };

  const addTerm = () => {
    const term = newTerm.trim();
    if (term === "" || config.dictionary.includes(term)) return;
    patch({ dictionary: [...config.dictionary, term] });
    setNewTerm("");
  };

  const removeTerm = (term: string) =>
    patch({ dictionary: config.dictionary.filter((t) => t !== term) });

  const addProfile = () => {
    const appId = newAppId.trim();
    if (appId === "") return;
    patch({ profiles: { ...config.profiles, [appId]: newProfile } });
    setNewAppId("");
    setNewProfile("raw");
  };

  const removeProfile = (appId: string) => {
    const next = { ...config.profiles };
    delete next[appId];
    patch({ profiles: next });
  };

  const onHotkey = (chord: string) => {
    patch({ hotkey: chord });
    validateHotkey(chord).then(
      () => setHotkeyError(null),
      (reason: string) => setHotkeyError(String(reason)),
    );
  };

  const save = () => {
    setConfig(config).then(
      () => {
        setSaved(true);
        setHotkeyError(null);
      },
      (reason: string) => setHotkeyError(String(reason)),
    );
  };

  return (
    <main className="settings">
      <div className="settings__chrome">
        <header className="settings__brand">
          <span className="settings__brand-mark" aria-hidden="true">V</span>
          <span>
            <strong>Verbatim</strong>
            <small>Private, local dictation</small>
          </span>
          <span className="settings__local"><i /> On-device</span>
        </header>
        <div className="settings__tabs" role="tablist" aria-label="Settings sections">
          {TABS.map((name, i) => (
            <button
              key={name}
              ref={(el) => {
                tabRefs.current[i] = el;
              }}
              role="tab"
              id={`tab-${name}`}
              aria-selected={tab === name}
              aria-controls={`panel-${name}`}
              tabIndex={tab === name ? 0 : -1}
              className={tab === name ? "settings__tab settings__tab--active" : "settings__tab"}
              onClick={() => setTab(name)}
              onKeyDown={(e) => onTabKey(e, i)}
            >
              <span aria-hidden="true">{TAB_COPY[name].glyph}</span>
              {name}
            </button>
          ))}
        </div>
      </div>

      <div className="settings__workspace">
        <header className="settings__page-header">
          <div>
            <h1>{TAB_COPY[tab].title}</h1>
            <p>{TAB_COPY[tab].detail}</p>
          </div>
        </header>
        {/* tabIndex 0: the Dictation panel holds no focusable control, so without
            it a keyboard user can neither reach nor scroll that panel. */}
        <section
          className="settings__panel"
          role="tabpanel"
          id={`panel-${tab}`}
          aria-labelledby={`tab-${tab}`}
          tabIndex={0}
        >
        {tab === "General" && (
          <div className="settings__card">
            <div className="settings__card-heading">
              <strong>Recording trigger</strong>
              <span>The shortcut works from any application.</span>
            </div>
            <label className="settings__field" htmlFor="hotkey">
              <span>Dictation hotkey</span>
              <input
                id="hotkey"
                type="text"
                value={config.hotkey}
                aria-invalid={hotkeyError !== null}
                aria-describedby={hotkeyError ? "hotkey-error" : undefined}
                onChange={(e) => onHotkey(e.target.value)}
              />
            </label>
            {hotkeyError && (
              <p id="hotkey-error" className="settings__error" role="alert">
                {hotkeyError}
              </p>
            )}
            {RIGHT_MODIFIER.test(config.hotkey.trim()) && (
              <p
                className={
                  inputMonitoring === "Granted"
                    ? "settings__hint"
                    : "settings__error"
                }
                role={inputMonitoring === "Granted" ? undefined : "alert"}
              >
                {inputMonitoring === "Granted"
                  ? "Input Monitoring is granted for this build."
                  : "macOS is denying Input Monitoring for this build. Remove the existing Verbatim row with the – button, add /Applications/Verbatim.app again, enable it, and restart Verbatim."}{" "}
                <button className="link" onClick={openInputMonitoringSettings}>
                  Open Input Monitoring
                </button>
              </p>
            )}
            <fieldset className="settings__field">
              <legend>Hotkey mode</legend>
              {(["hold", "toggle"] as const).map((mode) => (
                <label key={mode} className="settings__radio">
                  <input
                    type="radio"
                    name="hotkey_mode"
                    checked={config.hotkey_mode === mode}
                    onChange={() => patch({ hotkey_mode: mode })}
                  />
                  {mode === "hold" ? "Hold to talk" : "Press to toggle"}
                </label>
              ))}
            </fieldset>
          </div>
        )}

        {tab === "Dictation" && (
          <div className="settings__card">
            <div className="settings__card-heading">
              <strong>Speech recognition</strong>
              <span>Audio stays on this computer from capture to transcript.</span>
            </div>
            <p className="settings__row">
              <span>Transcription model</span>
              <strong>{config.transcription_model ?? "None selected"}</strong>
            </p>
            <p className="settings__hint">Manage models in the Models tab.</p>
          </div>
        )}

        {tab === "Polish" && (
          <>
            <label className="settings__toggle">
              <input
                type="checkbox"
                checked={config.polish}
                onChange={(e) => patch({ polish: e.target.checked })}
              />
              Inject polished text (uncheck for raw transcript)
            </label>
            <p className="settings__row">
              <span>Polish model</span>
              <strong>{config.polish_model ?? "None selected"}</strong>
            </p>
            <fieldset className="settings__field">
              <legend>Personal dictionary</legend>
              <p className="settings__hint">
                Terms forced to this exact casing in every dictation, whether polished
                or raw.
              </p>
              {config.dictionary.length === 0 ? (
                <p className="settings__hint">No terms yet.</p>
              ) : (
                <ul className="settings__dictionary">
                  {config.dictionary.map((term) => (
                    <li key={term} className="settings__dictionary-item">
                      <span>{term}</span>
                      <button
                        type="button"
                        className="settings__dictionary-remove"
                        onClick={() => removeTerm(term)}
                        aria-label={`Remove ${term}`}
                      >
                        Remove
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <div className="settings__dictionary-add">
                <input
                  type="text"
                  value={newTerm}
                  placeholder="e.g. PCM"
                  aria-label="New dictionary term"
                  onChange={(e) => setNewTerm(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addTerm();
                    }
                  }}
                />
                <button
                  type="button"
                  onClick={addTerm}
                  disabled={newTerm.trim() === ""}
                >
                  Add
                </button>
              </div>
            </fieldset>
            <fieldset className="settings__field">
              <legend>Per-app profiles</legend>
              <p className="settings__hint">
                Choose how each app is handled. Terminals default to raw; assign
                Raw to force the exact transcript, or Polished to override.
              </p>
              {Object.keys(config.profiles).length === 0 ? (
                <p className="settings__hint">No per-app overrides yet.</p>
              ) : (
                <ul className="settings__dictionary">
                  {Object.entries(config.profiles).map(([appId, profile]) => (
                    <li key={appId} className="settings__dictionary-item">
                      <span>
                        {appId} &rarr; {profile === "raw" ? "Raw" : "Polished"}
                      </span>
                      <button
                        type="button"
                        className="settings__dictionary-remove"
                        onClick={() => removeProfile(appId)}
                        aria-label={`Remove override for ${appId}`}
                      >
                        Remove
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <div className="settings__dictionary-add settings__profile-add">
                <input
                  type="text"
                  value={newAppId}
                  placeholder="e.g. com.apple.Terminal"
                  aria-label="App id"
                  onChange={(e) => setNewAppId(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addProfile();
                    }
                  }}
                />
                <select
                  value={newProfile}
                  aria-label="Profile for app"
                  onChange={(e) => setNewProfile(e.target.value)}
                >
                  <option value="raw">Raw</option>
                  <option value="default">Polished</option>
                </select>
                <button
                  type="button"
                  onClick={addProfile}
                  disabled={newAppId.trim() === ""}
                >
                  Add
                </button>
              </div>
            </fieldset>
          </>
        )}

        {tab === "Models" && <ModelsTab />}

        {tab === "History" && (
          <>
            <label className="settings__field" htmlFor="retention">
              <span>Keep dictation history for</span>
              <select
                id="retention"
                value={config.history_retention_days}
                onChange={(e) => patch({ history_retention_days: Number(e.target.value) })}
              >
                <option value={0}>Off (store nothing)</option>
                <option value={1}>1 day</option>
                <option value={7}>7 days</option>
                <option value={30}>30 days</option>
                <option value={365}>1 year</option>
              </select>
            </label>
            <HistoryList />
          </>
        )}

        {tab === "About" && (
          <>
            <p className="settings__row">
              <span>Config schema version</span>
              <strong>{config.version}</strong>
            </p>
            <p className="settings__hint">
              Verbatim runs fully on your machine. No audio or text ever leaves this
              computer.
            </p>
            <p className="settings__row">
              <span>Documentation &amp; help</span>
              <button
                type="button"
                className="settings__link"
                onClick={() => openDocs().catch(() => setDocsFailed(true))}
              >
                underthinker.github.io/verbatim
              </button>
            </p>
            {/* No browser handler (bare Linux session, sandboxed launcher): say
                so rather than swallowing the click. The address stays readable
                above, so it is still copyable by hand. */}
            {docsFailed && (
              <p className="settings__error">
                Couldn&apos;t open your browser. Visit the address above.
              </p>
            )}
            <fieldset className="settings__field">
              <legend>Model licenses</legend>
              <p className="settings__hint">
                Verbatim bundles third-party models under their own licenses.
              </p>
              <ul className="settings__attributions">
                {Array.from(
                  new Map(
                    models.map((m) => [
                      `${m.attribution}·${m.license}`,
                      m,
                    ]),
                  ).values(),
                ).map((m) => (
                  <li key={`${m.attribution}·${m.license}`}>
                    {m.attribution} <span className="models__badge">{m.license}</span>
                  </li>
                ))}
              </ul>
            </fieldset>
          </>
        )}
        </section>
      </div>

      <footer className="settings__footer">
        {saved && <span className="settings__ok" role="status">Saved</span>}
        <button
          className="settings__save"
          onClick={save}
          disabled={hotkeyError !== null}
        >
          Save
        </button>
      </footer>
    </main>
  );
}
