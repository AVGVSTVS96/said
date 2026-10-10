import { useState } from "react";
import type { InputDevice, SessionView, Settings } from "./source";
import { Icon, LevelMeter, Unavailable } from "./components";

export function Setup({
  settings,
  devices,
  session,
}: {
  settings: Settings | null;
  devices: readonly InputDevice[];
  session?: SessionView;
}) {
  const [microphone, setMicrophone] = useState(settings?.microphone ?? "");
  const [hotkey, setHotkey] = useState(settings?.hotkey ?? "");
  const [scratch, setScratch] = useState("");
  return (
    <section className="setup-screen">
      <header className="page-heading">
        <div>
          <p className="eyebrow">First-run setup</p>
          <h1>Make room for your voice.</h1>
          <p className="subtitle">
            Choose your input, then try dictation right here.
          </p>
        </div>
        <span className="badge">Local by default</span>
      </header>
      <div className="setup-grid">
        <section className="setup-step">
          <div className="step-heading">
            <span className="step-number">1</span>
            <h2>Choose a microphone</h2>
          </div>
          <label className="field-label">
            Audio input
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
          <div className="input-level">
            <LevelMeter level={session?.level ?? null} />
            <span className="quiet">
              {session?.level === null || !session
                ? "Waiting for microphone frames"
                : "Live input level"}
            </span>
          </div>
          <p className="quiet">The meter follows actual microphone frames.</p>
        </section>
        <section className="setup-step">
          <div className="step-heading">
            <span className="step-number">2</span>
            <h2>Get the local model</h2>
          </div>
          <strong className="model-name">
            {settings?.engine ?? "Default model"}
          </strong>
          <p className="quiet">Download size and license are unavailable.</p>
          <progress aria-label="Model download" value={0} max={100} />
          <div className="step-bottom">
            <span className="quiet">Model status unavailable</span>
            <button disabled>Download model</button>
          </div>
        </section>
        <section className="setup-step">
          <div className="step-heading">
            <span className="step-number">3</span>
            <h2>Set your shortcut</h2>
          </div>
          <label className="field-label">
            Dictation shortcut
            <input
              value={hotkey}
              placeholder="Shortcut unavailable"
              onChange={(event) => setHotkey(event.target.value)}
            />
          </label>
          <p className="quiet">
            Hold to talk. Tap to toggle. Add Shift for interactive mode.
          </p>
          <details>
            <summary>Linux / Niri setup</summary>
            <p>
              Installation instructions are unavailable in this build. Without
              input access, the shortcut toggles recording.
            </p>
          </details>
        </section>
        <section className="setup-step">
          <div className="step-heading">
            <span className="step-number">4</span>
            <h2>Allow access</h2>
          </div>
          <div className="permission-row">
            <span>Microphone</span>
            <button disabled>Grant access</button>
          </div>
          <div className="permission-row">
            <span>
              Accessibility <small>macOS</small>
            </span>
            <button disabled>Grant access</button>
          </div>
          <p className="quiet">
            Permission status is unavailable in this build.
          </p>
        </section>
      </div>
      <section className="scratch-card">
        <div className="step-heading">
          <Icon name="mic" />
          <h2>Try a few words</h2>
          <span className="quiet">Stays in this screen</span>
        </div>
        <label className="field-label">
          Scratch text
          <textarea
            value={scratch}
            onChange={(event) => setScratch(event.target.value)}
            placeholder="A quiet place to try your words."
            spellCheck={false}
          />
        </label>
        <div className="step-bottom">
          <p className="quiet">Setup never inserts into another app.</p>
          <button className="primary" disabled>
            Finish setup
            <Icon name="check" />
          </button>
        </div>
      </section>
      <Unavailable>
        Setup is unavailable in this build. Microphone and shortcut changes here
        are a preview; you can type in the scratch field.
      </Unavailable>
    </section>
  );
}
