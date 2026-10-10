import { useState } from "react";
import type { InputDevice, Settings as SettingsValue } from "./source";
import type { HostState } from "./contracts";
import type { CoreSource } from "./source";

export function Settings({
  settings,
  devices, host, send,
}: {
  settings: SettingsValue | null;
  devices: readonly InputDevice[];
  host?: HostState;
  send: CoreSource["dispatch"];
}) {
  const [draft, setDraft] = useState<SettingsValue | null>(null);
  const value = draft ?? settings;
  const [vocabulary, setVocabulary] = useState(host?.vocabulary_text ?? "");
  const [before, setBefore] = useState("");
  return (
    <section>
      <header className="page-heading">
        <div>
          <p className="eyebrow">Settings</p>
          <h1>A few things, set your way.</h1>
          <p className="subtitle">
            Recognition stays verbatim with every setting.
          </p>
        </div>
      </header>
      {value ? (
        <>
          <div className="settings-group">
            <h2>Recording</h2>
            <label className="setting-row">
              <span>
                <strong>Microphone</strong>
                <small>A disconnected microphone stops the recording.</small>
              </span>
              <select
                value={value.microphone ?? ""}
                onChange={(event) =>
                  setDraft({ ...value, microphone: event.target.value || null })
                }
              >
                <option value="">System default</option>
                {devices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="setting-row">
              <span>
                <strong>Dictation shortcut</strong>
                <small>Hold to talk. Tap to start or stop.</small>
              </span>
              <input
                aria-label="Dictation shortcut"
                value={value.hotkey}
                onChange={(event) =>
                  setDraft({ ...value, hotkey: event.target.value })
                }
              />
            </label>
            <label className="setting-row">
              <span>
                <strong>Recognition engine</strong>
                <small>Current recognition engine.</small>
              </span>
              <select aria-label="Recognition engine" value={value.engine} onChange={event => setDraft({ ...value, engine: event.target.value })}>
                {(host?.models ?? [{ id: value.engine }]).map(model => <option key={model.id}>{model.id}</option>)}
              </select>
            </label>
          </div>
          <div className="settings-group">
            <h2>Vocabulary</h2>
            <label className="setting-row">
              <span>
                <strong>Use vocabulary</strong>
                <small>
                  Bias recognition toward terms in vocabulary.txt. Engine
                  support varies.
                </small>
              </span>
              <input
                className="switch"
                type="checkbox"
                role="switch"
                checked={value.vocabulary}
                onChange={(event) =>
                  setDraft({ ...value, vocabulary: event.target.checked })
                }
              />
            </label>
          </div>
          <label className="field-label">Vocabulary terms, one per line<textarea value={vocabulary} onChange={event => setVocabulary(event.target.value)} spellCheck={false} /></label>
          <button disabled={!host} onClick={() => void send({ type: "vocabulary_save", text: vocabulary }).catch(() => {})}>Save vocabulary</button>
          <div className="settings-group">
            <h2>App insertion</h2>
            <p className="quiet">
              Auto-send presses Enter after pasting in quick mode. Turn it on
              only for an app you intend to submit to.
            </p>
            {Object.entries(value.apps).map(([id, app]) => (
              <div className="app-setting" key={id}>
                <span className="app-id">{id}</span>
                <label>
                  Paste shortcut
                  <input
                    value={app.paste}
                    onChange={(event) =>
                      setDraft({
                        ...value,
                        apps: {
                          ...value.apps,
                          [id]: { ...app, paste: event.target.value },
                        },
                      })
                    }
                  />
                </label>
                <label className="auto-send">
                  Auto-send
                  <input
                    className="switch"
                    type="checkbox"
                    role="switch"
                    checked={app.send}
                    onChange={(event) =>
                      setDraft({
                        ...value,
                        apps: {
                          ...value.apps,
                          [id]: { ...app, send: event.target.checked },
                        },
                      })
                    }
                  />
                </label>
              </div>
            ))}
            {Object.keys(value.apps).length === 0 ? (
              <p>No per-app overrides. Auto-send is off.</p>
            ) : null}
          </div>
          <div className="settings-group">
            <h2>History storage</h2>
            <div className="setting-row">
              <span>
                <strong>Keep recordings until deleted</strong>
                <small>{host ? `${(host.disk_bytes / 1048576).toFixed(1)} MiB on disk` : "Storage appears after connection."}</small>
              </span>
              <label>Delete recordings before<input type="date" value={before} onChange={event => setBefore(event.target.value)} /></label>
              <button className="danger" disabled={!host || !before} onClick={() => void send({ type: "history_retention", before: new Date(before).getTime() }).catch(() => {})}>Delete older recordings</button>
            </div>
          </div>
          <footer className="settings-footer">
            <span className="quiet">
              {draft ? "Unsaved changes" : "Current settings"}
            </span>
            <div className="button-group">
              <button disabled={!draft} onClick={() => setDraft(null)}>
                Reset changes
              </button>
              <button className="primary" disabled={!host || !draft} onClick={() => void send({ type: "settings_save", settings: value }).then(() => setDraft(null)).catch(() => {})}>
                Save settings
              </button>
            </div>
          </footer>

        </>
      ) : (
        <div className="empty-state">
          <h2>Waiting for settings</h2>
          <p>Settings appear when the app connection is ready.</p>
        </div>
      )}
    </section>
  );
}
