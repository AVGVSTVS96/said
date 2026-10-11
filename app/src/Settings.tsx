import { useState } from "react";
import type { CoreSource, InputDevice, Settings as SettingsValue } from "./source";
import type { HostState } from "./contracts";
import { DesktopStatus, ShortcutFields, shortcutError } from "./shortcuts";

export function Settings({
  settings,
  devices,
  host,
  send,
}: {
  settings: SettingsValue | null;
  devices: readonly InputDevice[];
  host?: HostState;
  send: CoreSource["dispatch"];
}) {
  const [draft, setDraft] = useState<SettingsValue | null>(null);
  const value = draft ?? settings;
  const [before, setBefore] = useState("");
  const [appId, setAppId] = useState("");
  if (!value) return <p className="quiet">Waiting for settings.</p>;
  const update = (patch: Partial<SettingsValue>) =>
    setDraft({ ...value, ...patch });
  const mac = Boolean(host && "accessibility" in host.permissions);
  const paste = mac ? "cmd+v" : "ctrl+v";
  const invalidShortcut = shortcutError(value, mac);
  return (
    <section className="settings">
      <div className="group">
        <h2>Recording</h2>
        <label className="setting-row">
          <span>Microphone</span>
          <select
            value={value.microphone ?? ""}
            onChange={(event) =>
              update({ microphone: event.target.value || null })
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
        <ShortcutFields settings={value} mac={mac} onChange={update} />
        <DesktopStatus host={host} />
        <div className="setting-row"><span>Model</span><strong>{value.engine}</strong></div>
      </div>
      <div className="group">
        <h2>Vocabulary</h2>
        <label className="setting-row">
          <span>
            Bias recognition toward these terms
            <small>Engine support varies.</small>
          </span>
          <input
            className="switch"
            type="checkbox"
            role="switch"
            checked={value.vocabulary}
            onChange={(event) => update({ vocabulary: event.target.checked })}
          />
        </label>
        <textarea
          className="terms"
          aria-label="Vocabulary terms, one per line"
          placeholder="One term per line"
          value={vocabulary}
          onChange={(event) => setVocabulary(event.target.value)}
          spellCheck={false}
        />
        <div className="actions">
          <button
            disabled={!host}
            onClick={() =>
              void send({ type: "vocabulary_save", text: vocabulary }).catch(
                () => {},
              )
            }
          >
            Save vocabulary
          </button>
||||||| acc771f
        <label className="setting-row">
          <span>
            Shortcut
            <small>Hold to talk, tap to toggle. Shift for interactive.</small>
          </span>
          <input
            value={value.hotkey}
            onChange={(event) => update({ hotkey: event.target.value })}
          />
        </label>
        <label className="setting-row">
          <span>Engine</span>
          <select
            value={value.engine}
            onChange={(event) => update({ engine: event.target.value })}
          >
            {(host?.models ?? [{ id: value.engine }]).map((model) => (
              <option key={model.id}>{model.id}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="group">
        <h2>Vocabulary</h2>
        <label className="setting-row">
          <span>
            Bias recognition toward these terms
            <small>Engine support varies.</small>
          </span>
          <input
            className="switch"
            type="checkbox"
            role="switch"
            checked={value.vocabulary}
            onChange={(event) => update({ vocabulary: event.target.checked })}
          />
        </label>
        <textarea
          className="terms"
          aria-label="Vocabulary terms, one per line"
          placeholder="One term per line"
          value={vocabulary}
          onChange={(event) => setVocabulary(event.target.value)}
          spellCheck={false}
        />
        <div className="actions">
          <button
            disabled={!host}
            onClick={() =>
              void send({ type: "vocabulary_save", text: vocabulary }).catch(
                () => {},
              )
            }
          >
            Save vocabulary
          </button>
        </div>
      </div>
      <div className="group">
        <h2>Apps</h2>
        <p className="quiet">
          Paste shortcut per app. Auto-send presses Enter after the paste.
        </p>
        {Object.entries(value.apps).map(([id, app]) => (
          <div className="app-rule" key={id}>
            <span className="app-id">{id}</span>
            <input
              aria-label={`Paste shortcut for ${id}`}
              value={app.paste}
              onChange={(event) =>
                update({
                  apps: {
                    ...value.apps,
                    [id]: { ...app, paste: event.target.value },
                  },
                })
              }
            />
            <label className="auto-send">
              Auto-send
              <input
                className="switch"
                type="checkbox"
                role="switch"
                checked={app.send}
                onChange={(event) =>
                  update({
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
        <div className="app-rule">
          <input
            aria-label="App ID"
            placeholder="App ID"
            value={appId}
            onChange={(event) => setAppId(event.target.value)}
          />
          <button
            disabled={!appId.trim() || Boolean(value.apps[appId.trim()])}
            onClick={() => {
              update({
                apps: { ...value.apps, [appId.trim()]: { paste, send: false } },
              });
              setAppId("");
            }}
          >
            Add app
          </button>
        </div>
      </div>
      <div className="group">
        <h2>Storage</h2>
        <div className="setting-row">
          <span>
            Recordings stay until deleted
            {host ? (
              <small>{(host.disk_bytes / 1048576).toFixed(1)} MiB on disk{host.disk_error ? " (last known)" : ""}</small>
            ) : null}
            {host?.disk_error ? (
              <small role="status">Disk usage unavailable: {host.disk_error}</small>
            ) : null}
          </span>
          <span className="actions">
            <input
              type="date"
              aria-label="Delete recordings before"
              value={before}
              onChange={(event) => setBefore(event.target.value)}
            />
            <button
              className="danger"
              disabled={!host || !before}
              onClick={() =>
                void send({
                  type: "history_retention",
                  before: new Date(`${before}T00:00:00`).getTime(),
                }).catch(() => {})
              }
            >
              Delete older
            </button>
          </span>
        </div>
      </div>
      <footer className="window-foot">
        <span className="quiet">{draft ? "Unsaved changes" : ""}</span>
        <span className="actions">
          <button disabled={!draft} onClick={() => setDraft(null)}>
            Reset
          </button>
          <button
            className="primary"
            disabled={!host || !draft || Boolean(invalidShortcut)}
            onClick={() =>
              void send({ type: "settings_save", settings: value })
                .then(() => setDraft(null))
                .catch(() => {})
            }
          >
            Save
          </button>
        </span>
      </footer>
    </section>
  );
}
