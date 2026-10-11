import { useState } from "react";
import type { CoreSource, InputDevice, SessionView, Settings } from "./source";
import type { HostState } from "./contracts";
import { Icon, LevelMeter } from "./components";

const mib = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MiB`;
const label = (permission: string) =>
  permission.replaceAll("_", " ").replace(/^./, (c) => c.toUpperCase());

export function Setup({
  settings,
  devices,
  session,
  host,
  send,
}: {
  settings: Settings | null;
  devices: readonly InputDevice[];
  session?: SessionView;
  host?: HostState;
  send: CoreSource["dispatch"];
}) {
  const [microphone, setMicrophone] = useState(settings?.microphone ?? "");
  const [hotkey, setHotkey] = useState(settings?.hotkey ?? "");
  const [scratch, setScratch] = useState<string | null>(null);
  const scratchSession = session?.id === host?.scratch ? session : undefined;
  const text =
    scratch ??
    (scratchSession ? scratchSession.committed + scratchSession.partial : "");
  const model = host?.models.find((model) => model.id === settings?.engine);
  const download = host?.download;
  const ready = download?.state === "ready";
  const recording = scratchSession?.phase === "recording";
  const busy = download?.state === "downloading" || download?.state === "loading";
  async function save() {
    if (settings)
      await send({
        type: "settings_save",
        settings: { ...settings, microphone: microphone || null, hotkey },
      });
  }
  return (
    <section className="setup">
      <div className="group">
        <label className="setting-row">
          <span>
            <Icon name="mic" /> Microphone
          </span>
          <select
            value={microphone}
            onChange={(event) => setMicrophone(event.target.value)}
          >
            <option value="">System default</option>
            {devices.map((device) => (
              <option key={device.id} value={device.id}>
                {device.name}
              </option>
            ))}
          </select>
        </label>
        <div className="setting-row">
          <span className="quiet">Level</span>
          <LevelMeter level={scratchSession?.level ?? null} />
        </div>
      </div>
      <div className="group">
        <div className="setting-row">
          <span>
            Model <strong>{settings?.engine ?? "…"}</strong>
            <small>
              {model?.license ?? "License pending"} ·{" "}
              {model?.vocabulary ? "vocabulary" : "no vocabulary bias"}
              {download?.total
                ? ` · ${mib(download.received)} of ${mib(download.total)}`
                : ""}
            </small>
          </span>
          <span className="actions">
            <span className="quiet" role="status">
              {download?.state ?? "waiting"}
            </span>
            <button
              disabled={!host || busy}
              onClick={() =>
                void send({ type: "download_model" }).catch(() => {})
              }
            >
              {ready ? "Verify" : "Download"}
            </button>
          </span>
        </div>
        <progress
          aria-label="Model download"
          value={
            ready
              ? 1
              : download?.total
                ? download.received / download.total
                : undefined
          }
          max={1}
        />
        {download?.error ? <p role="alert">{download.error}</p> : null}
      </div>
      <div className="group">
        <label className="setting-row">
          <span>
            Shortcut
            <small>Hold to talk, tap to toggle. Shift for interactive.</small>
          </span>
          <input
            value={hotkey}
            onChange={(event) => setHotkey(event.target.value)}
          />
        </label>
        {host?.instructions.length ? (
          <details>
            <summary>Desktop setup</summary>
            {host.instructions.map((instruction) => (
              <p key={instruction}>{instruction}</p>
            ))}
          </details>
        ) : null}
        <div className="actions">
          <button disabled={!host} onClick={() => void save().catch(() => {})}>
            Save
          </button>
        </div>
      </div>
      {Object.keys(host?.permissions ?? {}).length ? (
        <div className="group">
          {Object.entries(host?.permissions ?? {}).map(([permission, status]) => (
            <div className="setting-row" key={permission}>
              <span>
                {label(permission)}
                <small>{status}</small>
              </span>
              <button
                disabled={status === "authorized"}
                onClick={() =>
                  void send({ type: "permission", permission }).catch(() => {})
                }
              >
                Allow
              </button>
            </div>
          ))}
        </div>
      ) : null}
      <div className="group">
        <textarea
          className="scratch"
          aria-label="Scratch text"
          value={text}
          onChange={(event) => setScratch(event.target.value)}
          placeholder="Try a few words here. Nothing is typed into another app."
          spellCheck={false}
        />
        <div className="actions end">
          <button
            disabled={!ready}
            onClick={() => {
              setScratch(null);
              void (
                recording
                  ? send({ type: "stop" })
                  : save().then(() =>
                      send({ type: "start", mode: "quick", scratch: true }),
                    )
              ).catch(() => {});
            }}
          >
            {recording ? "Stop" : "Try dictation"}
          </button>
          <button
            className="primary"
            disabled={!ready}
            onClick={() =>
              void save()
                .then(() => send({ type: "setup_complete" }))
                .catch(() => {})
            }
          >
            {host?.setup_complete ? "Setup complete" : "Finish setup"}
            <Icon name="check" />
          </button>
        </div>
      </div>
    </section>
  );
}
