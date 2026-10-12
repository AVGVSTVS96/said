import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CoreSource, InputDevice, Settings } from "./source";
import type { HostState } from "./contracts";
import { Icon, LevelMeter } from "./components";
import { DesktopStatus, ShortcutFields, shortcutError, shortcutLabel } from "./shortcuts";

type Step = "microphone" | "model" | "permissions" | "shortcut" | "try" | "done";
type Mark = "todo" | "active" | "done" | "failed";

const heard = 0.3;
const required = ["microphone", "accessibility"];
const titles: Record<Step, string> = {
  microphone: "Microphone",
  model: "Speech model",
  permissions: "Permissions",
  shortcut: "Shortcut",
  try: "Try it",
  done: "Done",
};
const permissionText: Record<string, [string, string]> = {
  microphone: ["Microphone", "So said can hear you while you dictate."],
  accessibility: ["Accessibility", "So said can paste into the app you are using."],
  input_monitoring: ["Input monitoring", "Optional. Lets said see shortcuts in every app."],
};
const statusText: Record<string, string> = {
  authorized: "Allowed",
  denied: "Not allowed",
  notdetermined: "Not asked yet",
  restricted: "Blocked by a device policy",
};
const downloadText: Record<string, string> = {
  missing: "Not downloaded",
  verifying: "Verifying",
  downloading: "Downloading",
  loading: "Loading",
  ready: "Ready",
  failed: "Download failed",
};
const mib = (bytes: number) => `${(bytes / 1048576).toFixed(0)} MB`;
const permissionName = (permission: string) =>
  permissionText[permission]?.[0] ??
  permission.replaceAll("_", " ").replace(/^./, (c) => c.toUpperCase());

function pasteProblem(notification?: string) {
  if (notification === "Copied")
    return "said copied the text because it cannot paste yet. Check Permissions.";
  if (!notification || notification.startsWith("Saved"))
    return "The box lost focus, so said kept the text instead of pasting. Click in the box and try again.";
  return notification;
}

function useWindowFocus() {
  const [focused, setFocused] = useState(() => document.hasFocus());
  useEffect(() => {
    const update = () => setFocused(document.hasFocus());
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    return () => {
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
    };
  }, []);
  return focused;
}

function Keys({ value, mac }: { value: string; mac: boolean }) {
  return (
    <span className="keys">
      {shortcutLabel(value, mac)
        .split("+")
        .map((key) => (
          <kbd key={key}>{key}</kbd>
        ))}
    </span>
  );
}

function Check({ mark, children, detail }: { mark: Mark; children: React.ReactNode; detail?: string }) {
  return (
    <li data-mark={mark}>
      <span className="mark" aria-hidden="true">
        {mark === "done" ? <Icon name="check" /> : mark === "failed" ? <Icon name="close" /> : null}
      </span>
      <span>
        {children}
        {detail ? <small>{detail}</small> : null}
      </span>
    </li>
  );
}

export function Setup({
  settings,
  devices,
  host,
  source,
  send,
}: {
  settings: Settings | null;
  devices: readonly InputDevice[];
  host?: HostState;
  source: CoreSource;
  send: CoreSource["dispatch"];
}) {
  const focused = useWindowFocus();
  const permissions = Object.entries(host?.permissions ?? {});
  const mac = Boolean(host && "accessibility" in host.permissions);
  const steps: Step[] = [
    "microphone",
    "model",
    ...(permissions.some(([name]) => name !== "microphone") ? (["permissions"] as const) : []),
    "shortcut",
    "try",
  ];
  const [step, setStep] = useState<Step>("microphone");
  const [microphone, setMicrophone] = useState(settings?.microphone ?? "");
  const [shortcuts, setShortcuts] = useState<Partial<Settings>>({});
  const [loudest, setLoudest] = useState(0);
  const [trialPeak, setTrialPeak] = useState({ id: "", peak: 0 });
  const [practice, setPractice] = useState("");
  const [pastedTrial, setPastedTrial] = useState("");
  const practiceField = useRef<HTMLTextAreaElement>(null);

  const configured = settings ? { ...settings, ...shortcuts } : null;
  const invalidShortcut = configured ? shortcutError(configured, mac) : null;
  const dirty = Boolean(
    settings && Object.entries(shortcuts).some(([field, value]) => settings[field as keyof Settings] !== value),
  );
  const missing = permissions
    .filter(([name, status]) => required.includes(name) && status !== "authorized")
    .map(([name]) => name);
  const download = host?.download;
  const ready = download?.state === "ready";
  const blocked: [string, Step] | null = !host
    ? ["Waiting for said to start", "microphone"]
    : !ready
      ? [download?.state === "failed" ? "The speech model needs a retry" : "Waiting for the speech model", "model"]
      : missing.length
        ? [
            `Allow ${missing.map(permissionName).join(" and ")}`,
            missing.every((name) => name === "microphone") ? "microphone" : "permissions",
          ]
        : invalidShortcut
          ? ["Fix the shortcut", "shortcut"]
          : null;

  const level = host?.preview ?? null;
  const trial = source.getSnapshot().sessions.find((session) => session.id === host?.trial);
  const trialText = trial ? (trial.edited ?? trial.committed + trial.partial).trim() : "";
  const trialSettled = Boolean(trial && !["recording", "finishing", "saving", "pasting"].includes(trial.phase));
  const delivered = Boolean(
    trial && (trial.phase === "pasted" || (host?.input === "clipboard" && pastedTrial === trial.id)),
  );
  const microphonePermission = host?.permissions.microphone;

  useEffect(() => {
    if (level !== null) setLoudest((loudest) => Math.max(loudest, level));
  }, [level]);
  useEffect(() => {
    if (trial?.level != null)
      setTrialPeak((current) => ({
        id: trial.id,
        peak: current.id === trial.id ? Math.max(current.peak, trial.level!) : trial.level!,
      }));
  }, [trial?.id, trial?.level]);

  const previewing = Boolean(host) && step === "microphone" && focused;
  useEffect(() => {
    void source
      .dispatch({ type: "preview", active: previewing, microphone: microphone || null })
      .catch(() => {});
  }, [source, previewing, microphone]);
  const armed = Boolean(host) && step === "try" && focused;
  useEffect(() => {
    void source.dispatch({ type: "trial", armed }).catch(() => {});
  }, [source, armed]);
  useEffect(
    () => () => {
      void source.dispatch({ type: "preview", active: false, microphone: null }).catch(() => {});
      void source.dispatch({ type: "trial", armed: false }).catch(() => {});
    },
    [source],
  );

  const granted = permissions.length > 0 && missing.length === 0;
  const wasGranted = useRef(granted);
  useEffect(() => {
    if (step === "permissions" && granted && !wasGranted.current) {
      const timer = setTimeout(() => setStep("shortcut"), 700);
      wasGranted.current = granted;
      return () => clearTimeout(timer);
    }
    wasGranted.current = granted;
  }, [step, granted]);

  useEffect(() => {
    if (step === "try") practiceField.current?.focus();
  }, [step]);

  async function save(patch: Partial<Settings> = {}) {
    if (!configured || invalidShortcut) return;
    await send({
      type: "settings_save",
      settings: { ...configured, microphone: microphone || null, ...patch },
    });
    setShortcuts({});
  }
  async function finish() {
    if (dirty) await save();
    if (!host?.setup_complete) await send({ type: "setup_complete" });
    setStep("done");
  }

  const finished = useRef("");
  const trialId = trial?.id;
  const canFinish = !blocked;
  useEffect(() => {
    if (!delivered || !trialId || !canFinish || finished.current === trialId) return;
    const timer = setTimeout(() => {
      finished.current = trialId;
      void finish().catch(() => {});
    }, 1200);
    return () => clearTimeout(timer);
  }, [delivered, trialId, canFinish]);

  function next() {
    if (step === "shortcut" && dirty) {
      void save()
        .then(() => setStep("try"))
        .catch(() => {});
      return;
    }
    if (step === "try") {
      void finish().catch(() => {});
      return;
    }
    setStep(steps[steps.indexOf(step) + 1] ?? "try");
  }

  const hotkey = configured?.hotkey ?? "";
  const doneSteps: Record<Step, boolean> = {
    microphone: (loudest >= heard || Boolean(host?.setup_complete)) && microphonePermission !== "denied",
    model: ready,
    permissions: granted,
    shortcut: Boolean(settings) && !dirty && !invalidShortcut,
    try: delivered,
    done: Boolean(host?.setup_complete),
  };

  const body = {
    microphone: (
      <>
        <label className="field">
          <span>Input</span>
          <select
            value={microphone}
            onChange={(event) => {
              const value = event.target.value;
              setMicrophone(value);
              setLoudest(0);
              void save({ microphone: value || null }).catch(() => {});
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
        <LevelMeter level={level} bars={40} className="large" />
        <p className="setup-status" role="status">
          {host?.preview_error ? (
            <span className="action-error">{host.preview_error}</span>
          ) : loudest >= heard ? (
            <>
              <Icon name="check" /> Sounds good.
            </>
          ) : level === null ? (
            "Opening the microphone…"
          ) : (
            "Say a few words. The bars should move while you talk."
          )}
        </p>
        {microphonePermission && microphonePermission !== "authorized" ? (
          <div className="setting-row">
            <span>
              Microphone access
              <small>{statusText[microphonePermission] ?? microphonePermission}</small>
            </span>
            <button onClick={() => void send({ type: "permission", permission: "microphone" }).catch(() => {})}>
              Allow
            </button>
          </div>
        ) : null}
      </>
    ),
    model: (
      <>
        <div className="setting-row">
          <span>
            <strong>{settings?.engine ?? "…"}</strong>
            <small>
              {downloadText[download?.state ?? ""] ?? "Waiting"}
              {download?.total && download.state !== "ready"
                ? ` · ${mib(download.received)} of ${mib(download.total)}`
                : download?.total
                  ? ` · ${mib(download.total)}`
                  : ""}
            </small>
          </span>
          <button
            disabled={!host || download?.state === "verifying" || download?.state === "downloading" || download?.state === "loading"}
            onClick={() => void send({ type: "download_model" }).catch(() => {})}
          >
            {download?.state === "failed" ? "Retry" : ready ? "Verify again" : "Download"}
          </button>
        </div>
        <progress
          aria-label="Model download"
          value={ready ? 1 : download?.total ? download.received / download.total : undefined}
          max={1}
        />
        {download?.error ? (
          <p className="action-error" role="alert">
            {download.error}
          </p>
        ) : null}
        <p className="quiet setup-note">
          {host?.models.find((model) => model.id === settings?.engine)?.license ?? ""}
        </p>
      </>
    ),
    permissions: (
      <div className="permission-list">
        {permissions.map(([permission, status]) => (
          <div className="setting-row" key={permission}>
            <span>
              {permissionName(permission)}
              {!required.includes(permission) ? <span className="quiet"> · optional</span> : null}
              <small>{permissionText[permission]?.[1] ?? ""}</small>
            </span>
            <span className="actions">
              <span className="permission-status" data-granted={status === "authorized"}>
                {status === "authorized" ? <Icon name="check" /> : null}
                {statusText[status] ?? status}
              </span>
              {status === "authorized" ? null : (
                <button onClick={() => void send({ type: "permission", permission }).catch(() => {})}>
                  {permission === "microphone" ? "Allow" : "Open Settings"}
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
    ),
    shortcut: (
      <>
        {configured ? (
          <ShortcutFields
            settings={configured}
            mac={mac}
            onChange={(patch) => setShortcuts({ ...shortcuts, ...patch })}
          />
        ) : null}
        <div className="actions end">
          <button disabled={!dirty || Boolean(invalidShortcut)} onClick={() => void save().catch(() => {})}>
            {dirty ? "Save shortcuts" : "Saved"}
          </button>
        </div>
        <DesktopStatus host={host} />
      </>
    ),
    try: (
      <>
        <textarea
          ref={practiceField}
          className="practice"
          aria-label="Practice field"
          value={practice}
          onChange={(event) => setPractice(event.target.value)}
          onPaste={() => trial && trialSettled && setPastedTrial(trial.id)}
          placeholder="Your words land here."
          spellCheck={false}
        />
        <ul className="checks" aria-label="Dictation checks">
          <Check mark={trial ? "done" : "active"}>
            {trial ? "Shortcut detected" : host?.input === "clipboard" ? "Press Start" : "Press the shortcut"}
          </Check>
          <Check
            mark={
              (trialPeak.id === trial?.id && trialPeak.peak >= heard) || (trialSettled && trialText)
                ? "done"
                : trialSettled
                  ? "failed"
                  : trial
                    ? "active"
                    : "todo"
            }
            detail={trialSettled && !trialText && trialPeak.peak < heard ? "Only silence came through. Check the microphone step." : undefined}
          >
            Hearing you
          </Check>
          <Check
            mark={trialText && trial?.phase !== "recording" ? "done" : trialSettled ? "failed" : trial ? "active" : "todo"}
            detail={trialSettled && !trialText ? "No speech recognized. Speak a little longer." : trialText || undefined}
          >
            Recognized
          </Check>
          <Check
            mark={delivered ? "done" : trialSettled && trialText && host?.input !== "clipboard" ? "failed" : trialSettled && trialText ? "active" : "todo"}
            detail={
              delivered || !trialSettled || !trialText
                ? undefined
                : host?.input === "clipboard"
                  ? "Copied. Press Ctrl+V in the box to paste it."
                  : pasteProblem(host?.notifications[trial!.id])
            }
          >
            {host?.input === "clipboard" ? "Pasted from the clipboard" : "Pasted here"}
          </Check>
        </ul>
        <div className="actions">
          <button
            className={host?.input === "clipboard" ? undefined : "quiet"}
            disabled={!ready}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => {
              practiceField.current?.focus();
              void send(trial?.phase === "recording" ? { type: "stop" } : { type: "start", mode: "quick" }).catch(() => {});
            }}
          >
            {trial?.phase === "recording"
              ? "Stop"
              : host?.input === "clipboard"
                ? "Start"
                : "Shortcut not working? Record with this button"}
          </button>
        </div>
      </>
    ),
    done: (
      <>
        <dl className="shortcut-summary">
          <dt>Dictate</dt>
          <dd>
            {configured ? <Keys value={configured.hotkey} mac={mac} /> : null}
            <small>{host?.input === "hold" ? "Hold, speak, let go. The text is pasted where your cursor is." : "Press, speak, press again."}</small>
          </dd>
          <dt>Review first</dt>
          <dd>
            {configured ? <Keys value={configured.interactive_hotkey} mac={mac} /> : null}
            <small>Opens an editor so you can fix the text before inserting it.</small>
          </dd>
          <dt>Insert last</dt>
          <dd>
            {configured ? <Keys value={configured.insert_last_hotkey} mac={mac} /> : null}
            <small>Pastes your last dictation again.</small>
          </dd>
        </dl>
        <p className="quiet setup-note">
          said keeps running in the {mac ? "menu bar" : "tray"}. History, Settings and this setup are there.
        </p>
      </>
    ),
  } satisfies Record<Step, React.ReactNode>;

  const lead: Record<Step, React.ReactNode> = {
    microphone: "Pick the microphone said should listen to, then say a few words.",
    model: "said turns speech into text on this computer. The model downloads once; nothing you say leaves your machine.",
    permissions: "macOS asks before an app can hear you or paste for you. This page updates as soon as you allow each one.",
    shortcut: "This is the key you will use to dictate in any app.",
    try:
      host?.input === "clipboard" ? (
        <>Press Start, say a sentence, then Stop. said copies the text; paste it below with <kbd>Ctrl</kbd> <kbd>V</kbd>.</>
      ) : host?.input === "toggle" ? (
        <>Click in the box, press <Keys value={hotkey} mac={mac} />, say a sentence, then press it again.</>
      ) : (
        <>Click in the box, hold <Keys value={hotkey} mac={mac} />, say a sentence, then let go.</>
      ),
    done: "said is ready.",
  };

  const index = steps.indexOf(step);
  return (
    <section className="setup">
      {step === "done" ? null : (
        <nav className="setup-steps" aria-label="Setup steps">
          <ol>
            {steps.map((item, position) => (
              <li key={item}>
                <button
                  aria-current={item === step ? "step" : undefined}
                  data-done={doneSteps[item]}
                  onClick={() => setStep(item)}
                >
                  {doneSteps[item] ? <Icon name="check" /> : <span className="step-number">{position + 1}</span>}
                  {titles[item]}
                </button>
              </li>
            ))}
          </ol>
        </nav>
      )}
      <div className="setup-content" key={step}>
        <h1>{step === "done" ? "You are set" : titles[step]}</h1>
        <p className="setup-lead">{lead[step]}</p>
        {body[step]}
      </div>
      <div className="setup-foot">
        {index > 0 ? (
          <button className="quiet" onClick={() => setStep(steps[index - 1])}>
            Back
          </button>
        ) : null}
        <span className="spacer" />
        {step === "done" ? (
          <button className="primary" onClick={() => void (isTauri() ? getCurrentWindow().close() : undefined)}>
            Close
          </button>
        ) : step === "try" ? (
          <>
            {blocked ? (
              <button className="quiet setup-reason" onClick={() => setStep(blocked[1])}>
                {blocked[0]}
                <Icon name="arrow" />
              </button>
            ) : null}
            <button className="primary" disabled={Boolean(blocked)} onClick={next}>
              Finish setup
              <Icon name="check" />
            </button>
          </>
        ) : (
          <>
            {step === "shortcut" && invalidShortcut ? <span className="setup-reason quiet">Fix the shortcut to continue</span> : null}
            <button className="primary" disabled={step === "shortcut" && Boolean(invalidShortcut)} onClick={next}>
              Continue
              <Icon name="arrow" />
            </button>
          </>
        )}
      </div>
    </section>
  );
}
