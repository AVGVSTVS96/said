import { useState } from "react";
import type { InputDevice, SessionView, Settings, CoreSource } from "./source";
import type { HostState } from "./contracts";
import { Icon, LevelMeter } from "./components";

export function Setup({ settings, devices, session, host, send }: {
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
  const text = scratch ?? (scratchSession ? scratchSession.committed + scratchSession.partial : "");
  const model = host?.models.find(model => model.id === settings?.engine);
  const download = host?.download;
  const ready = download?.state === "ready";
  const recording = scratchSession?.phase === "recording";
  async function save() {
    if (settings) await send({ type: "settings_save", settings: { ...settings, microphone: microphone || null, hotkey } });
  }
  return (
    <section className="setup-screen">
      <header className="page-heading"><div><p className="eyebrow">First-run setup</p><h1>Make room for your voice.</h1><p className="subtitle">Choose your input, then try dictation right here.</p></div><span className="badge">Local by default</span></header>
      <div className="setup-grid">
        <section className="setup-step">
          <div className="step-heading"><span className="step-number">1</span><h2>Choose a microphone</h2></div>
          <label className="field-label">Audio input<select value={microphone} onChange={event => setMicrophone(event.target.value)}><option value="">System default</option>{devices.map(device => <option key={device.id} value={device.id}>{device.name}</option>)}</select></label>
          <div className="input-level"><LevelMeter level={scratchSession?.level ?? null} /><span className="quiet">{recording ? "Live input level" : "Start a scratch recording to check the input"}</span></div>
        </section>
        <section className="setup-step">
          <div className="step-heading"><span className="step-number">2</span><h2>Get the local model</h2></div>
          <strong className="model-name">{settings?.engine ?? "Default model"}</strong>
          <p className="quiet">License: {model?.license ?? "Waiting for manifest"} · {model?.vocabulary ? "Vocabulary supported" : "Vocabulary unavailable"}</p>
          {download?.total ? <p>{(download.received / 1048576).toFixed(1)} / {(download.total / 1048576).toFixed(1)} MiB · {download.file}</p> : <p>{download?.file}</p>}
          <progress aria-label="Model download" value={ready ? 1 : download?.total ? download.received / download.total : undefined} max={1} />
          <div className="step-bottom"><span role="status">{download?.state ?? "Waiting for host"}</span><button disabled={!host || download?.state === "downloading" || download?.state === "loading"} onClick={() => void send({ type: "download_model" }).catch(() => {})}>{ready ? "Verify model" : "Download model"}</button></div>
          {download?.error ? <p role="alert">{download.error}</p> : null}
        </section>
        <section className="setup-step">
          <div className="step-heading"><span className="step-number">3</span><h2>Set your shortcut</h2></div>
          <label className="field-label">Dictation shortcut<input value={hotkey} onChange={event => setHotkey(event.target.value)} /></label>
          <p className="quiet">Hold to talk. Tap to toggle. Add Shift for interactive mode.</p>
          <details><summary>Platform setup</summary>{host?.instructions.map(instruction => <p key={instruction}>{instruction}</p>)}</details>
          <button disabled={!host} onClick={() => void save().catch(() => {})}>Save microphone and shortcut</button>
        </section>
        <section className="setup-step">
          <div className="step-heading"><span className="step-number">4</span><h2>Allow access</h2></div>
          {Object.entries(host?.permissions ?? {}).map(([permission, status]) => <div className="permission-row" key={permission}><span>{permission.replaceAll("_", " ")}<small>{status}</small></span><button disabled={status === "authorized"} onClick={() => void send({ type: "permission", permission }).catch(() => {})}>Grant access</button></div>)}
        </section>
      </div>
      <section className="scratch-card">
        <div className="step-heading"><Icon name="mic" /><h2>Try a few words</h2><span className="quiet">Stays in this screen</span></div>
        <label className="field-label">Scratch text<textarea value={text} onChange={event => setScratch(event.target.value)} placeholder="A quiet place to try your words." spellCheck={false} /></label>
        <div className="step-bottom"><p className="quiet">Setup never inserts into another app.</p><button disabled={!ready} onClick={() => { setScratch(null); void (recording ? send({ type: "stop" }) : save().then(() => send({ type: "start", mode: "quick", scratch: true }))).catch(() => {}); }}>{recording ? "Stop recording" : "Try dictation"}</button><button className="primary" disabled={!ready} onClick={() => void save().then(() => send({ type: "setup_complete" })).catch(() => {})}>{host?.setup_complete ? "Setup complete" : "Finish setup"}<Icon name="check" /></button></div>
      </section>
    </section>
  );
}
