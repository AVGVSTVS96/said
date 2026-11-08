import { useState } from "react";

import { Icon } from "./components";
import type { HostState } from "./contracts";
import { DesktopStatus, macHost, ShortcutFields, shortcutError } from "./shortcuts";
import type { InputDevice, Send, Settings as SettingsValue, Utterance } from "./source";

const pasteChords = [
  ["ctrl+v", "Ctrl+V"],
  ["ctrl+shift+v", "Ctrl+Shift+V"],
  ["shift+insert", "Shift+Insert"],
] as const;

const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });

const Apps = ({
  value,
  history,
  mac,
  update,
}: {
  value: SettingsValue;
  history: readonly Utterance[];
  mac: boolean;
  update: (patch: Partial<SettingsValue>) => void;
}) => {
  const [choice, setChoice] = useState<string | null>(null);
  const [appId, setAppId] = useState("");
  const paste = mac ? "cmd+v" : "ctrl+v";
  const names = new Map<string, string>();

  for (const row of history) {
    if (row.pasted_to && !names.has(row.pasted_to.app_id)) {
      names.set(row.pasted_to.app_id, row.pasted_to.name);
    }
  }

  const recent = [...names].filter(
    ([id]) => value.apps[id] === undefined && !id.startsWith("pid:"),
  );

  const picked =
    choice !== null && (choice === "" || recent.some(([id]) => id === choice))
      ? choice
      : (recent[0]?.[0] ?? "");

  const adding = (picked === "" ? appId : picked).trim();

  return (
    <div className="group">
      <h2>Apps</h2>
      <p className="quiet">
        {mac
          ? "Auto-send presses Return after pasting into that app."
          : "Paste shortcut per app. Auto-send presses Enter after the paste."}
      </p>
      {Object.entries(value.apps).map(([id, app]) => (
        <div className="app-rule" key={id}>
          <span className="app-id">
            {names.get(id) ?? id}
            {names.has(id) ? <small>{id}</small> : null}
          </span>
          {mac ? null : (
            <select
              aria-label={`Paste shortcut for ${id}`}
              value={app.paste}
              onChange={(event) => {
                update({
                  apps: {
                    ...value.apps,
                    [id]: { ...app, paste: event.target.value },
                  },
                });
              }}
            >
              {pasteChords.map(([chord, label]) => (
                <option key={chord} value={chord}>
                  {label}
                </option>
              ))}
            </select>
          )}
          <label className="auto-send">
            Auto-send
            <input
              className="switch"
              type="checkbox"
              role="switch"
              aria-checked={app.send}
              checked={app.send}
              onChange={(event) => {
                update({
                  apps: {
                    ...value.apps,
                    [id]: { ...app, send: event.target.checked },
                  },
                });
              }}
            />
          </label>
          <button
            type="button"
            className="quiet"
            aria-label={`Remove ${names.get(id) ?? id}`}
            title="Remove"
            onClick={() => {
              update({
                apps: Object.fromEntries(Object.entries(value.apps).filter(([key]) => key !== id)),
              });
            }}
          >
            <Icon name="trash" />
          </button>
        </div>
      ))}
      <div className="app-rule">
        {recent.length > 0 ? (
          <select
            aria-label="App to add"
            value={picked}
            onChange={(event) => {
              setChoice(event.target.value);
            }}
          >
            {recent.map(([id, name]) => (
              <option key={id} value={id}>
                {name} ({id})
              </option>
            ))}
            <option value="">Another app ID…</option>
          </select>
        ) : null}
        {picked === "" ? (
          <input
            aria-label="App ID"
            placeholder={mac ? "Bundle ID, e.g. com.apple.Terminal" : "App ID"}
            value={appId}
            onChange={(event) => {
              setAppId(event.target.value);
            }}
          />
        ) : null}
        <button
          type="button"
          disabled={adding === "" || value.apps[adding] !== undefined}
          onClick={() => {
            update({
              apps: { ...value.apps, [adding]: { paste, send: false } },
            });
            setAppId("");
            setChoice(null);
          }}
        >
          Add app
        </button>
      </div>
    </div>
  );
};

const Storage = ({ host, send }: { host: HostState | undefined; send: Send }) => {
  const [before, setBefore] = useState("");
  const [confirming, setConfirming] = useState(false);
  const cutoff = before === "" ? null : new Date(`${before}T00:00:00`);

  const deleteBefore = async (date: Date) => {
    if (await send({ type: "history_retention", before: date.getTime() })) {
      setConfirming(false);
      setBefore("");
    }
  };

  return (
    <div className="group">
      <h2>Storage</h2>
      <div className="setting-row">
        <span>
          Recordings stay until deleted
          {host ? (
            <small>
              {(host.disk_bytes / 1_048_576).toFixed(1)} MiB on disk
              {host.disk_error === null ? "" : " (last known)"}
            </small>
          ) : null}
          {(host?.disk_error ?? "") === "" ? null : (
            <small>
              <output>Disk usage unavailable: {host?.disk_error}</output>
            </small>
          )}
        </span>
        <span className="actions">
          <input
            type="date"
            aria-label="Delete recordings before"
            value={before}
            onChange={(event) => {
              setBefore(event.target.value);
              setConfirming(false);
            }}
          />
          <button
            type="button"
            className="danger"
            disabled={!host || !cutoff || confirming}
            onClick={() => {
              setConfirming(true);
            }}
          >
            Delete older
          </button>
        </span>
      </div>
      {confirming && cutoff ? (
        <fieldset className="confirm" aria-label="Confirm deletion">
          <p>
            {`Delete every recording and transcript from before ${day.format(cutoff)}? This can't be undone.`}
          </p>
          <span className="actions">
            <button
              type="button"
              autoFocus
              onClick={() => {
                setConfirming(false);
              }}
            >
              Cancel
            </button>
            <button
              type="button"
              className="danger"
              onClick={() => {
                void deleteBefore(cutoff);
              }}
            >
              Delete recordings
            </button>
          </span>
        </fieldset>
      ) : null}
    </div>
  );
};

export const Settings = ({
  settings,
  devices,
  history,
  host,
  send,
}: {
  settings: SettingsValue | null;
  devices: readonly InputDevice[];
  history: readonly Utterance[];
  host: HostState | undefined;
  send: Send;
}) => {
  const [draft, setDraft] = useState<SettingsValue | null>(null);
  const value = draft ?? settings;

  if (!value) {
    return <p className="quiet">Waiting for settings.</p>;
  }

  const update = (patch: Partial<SettingsValue>) => {
    setDraft({ ...value, ...patch });
  };

  const mac = macHost(host);

  const save = async () => {
    if (await send({ type: "settings_save", settings: value })) {
      setDraft(null);
    }
  };

  return (
    <section className="settings">
      <div className="group">
        <h2>Recording</h2>
        <label className="setting-row">
          <span>Microphone</span>
          <select
            value={value.microphone ?? ""}
            onChange={(event) => {
              update({ microphone: event.target.value === "" ? null : event.target.value });
            }}
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
        <div className="setting-row">
          <span>Model</span>
          <strong>{value.engine}</strong>
        </div>
      </div>
      <Apps value={value} history={history} mac={mac} update={update} />
      <Storage host={host} send={send} />
      <footer className="window-foot">
        <span className="quiet">{draft ? "Unsaved changes" : ""}</span>
        <span className="actions">
          <button
            type="button"
            disabled={!draft}
            onClick={() => {
              setDraft(null);
            }}
          >
            Reset
          </button>
          <button
            type="button"
            className="primary"
            disabled={!host || !draft || shortcutError(value, mac) !== null}
            onClick={() => {
              void save();
            }}
          >
            Save
          </button>
        </span>
      </footer>
    </section>
  );
};
