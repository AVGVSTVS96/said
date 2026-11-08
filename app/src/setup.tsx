import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRef, useState, useSyncExternalStore } from "react";

import { Icon, LevelMeter } from "./components";
import type { DownloadState, HostAction, HostState } from "./contracts";
import { useImperative, useSubscription, useTimeout } from "./outside";
import {
  DesktopStatus,
  macHost,
  ShortcutFields,
  shortcutError,
  shortcutFields,
  shortcutLabel,
} from "./shortcuts";
import type { CoreSource, InputDevice, Phase, Send, SessionView, Settings } from "./source";

type Step = "microphone" | "model" | "permissions" | "shortcut" | "try" | "done";

type Mark = "todo" | "active" | "done" | "failed";

const heard = 0.3;
const required = new Set(["microphone", "accessibility"]);
const unsettled = new Set<Phase>(["recording", "finishing", "saving", "pasting"]);
const busy = new Set(["verifying", "downloading", "loading"]);

const titles: Record<Step, string> = {
  microphone: "Microphone",
  model: "Speech model",
  permissions: "Permissions",
  shortcut: "Shortcut",
  try: "Try it",
  done: "Done",
};

const permissionText = new Map([
  ["microphone", { name: "Microphone", why: "So said can hear you while you dictate." }],
  [
    "accessibility",
    { name: "Accessibility", why: "So said can paste into the app you are using." },
  ],
  [
    "input_monitoring",
    { name: "Input monitoring", why: "Optional. Lets said see shortcuts in every app." },
  ],
]);

const statusText = new Map([
  ["authorized", "Allowed"],
  ["denied", "Not allowed"],
  ["notdetermined", "Not asked yet"],
  ["restricted", "Blocked by a device policy"],
]);

const downloadText = new Map([
  ["missing", "Not downloaded"],
  ["verifying", "Verifying"],
  ["downloading", "Downloading"],
  ["loading", "Loading"],
  ["ready", "Ready"],
  ["failed", "Download failed"],
]);

const mib = (bytes: number) => `${(bytes / 1_048_576).toFixed(0)} MB`;

const permissionName = (permission: string) =>
  permissionText.get(permission)?.name ??
  permission.replaceAll("_", " ").replace(/^./u, (first) => first.toUpperCase());

const pasteProblem = (notification: string | undefined) => {
  if (notification === "Copied") {
    return "said copied the text because it cannot paste yet. Check Permissions.";
  }

  if (notification === undefined || notification === "" || notification.startsWith("Saved")) {
    return "The box lost focus, so said kept the text instead of pasting. Click in the box and try again.";
  }

  return notification;
};

const subscribeToFocus = (update: () => void) => {
  window.addEventListener("focus", update);
  window.addEventListener("blur", update);

  return () => {
    window.removeEventListener("focus", update);
    window.removeEventListener("blur", update);
  };
};

const useWindowFocus = () => useSyncExternalStore(subscribeToFocus, () => document.hasFocus());

const quietly = (source: CoreSource, action: HostAction) => {
  void source.dispatch(action).catch(() => null);
};

const blockedBy = (
  host: HostState | undefined,
  missing: readonly string[],
  invalidShortcut: string | null,
): readonly [string, Step] | null => {
  if (!host) {
    return ["Waiting for said to start", "microphone"];
  }

  if (host.download.state === "failed") {
    return ["The speech model needs a retry", "model"];
  }

  if (host.download.state !== "ready") {
    return ["Waiting for the speech model", "model"];
  }

  if (missing.length > 0) {
    return [
      `Allow ${missing.map(permissionName).join(" and ")}`,
      missing.every((name) => name === "microphone") ? "microphone" : "permissions",
    ];
  }

  return invalidShortcut === null ? null : ["Fix the shortcut", "shortcut"];
};

const progress = (done: boolean, settled: boolean, started: boolean): Mark => {
  if (done) {
    return "done";
  }

  if (settled) {
    return "failed";
  }

  return started ? "active" : "todo";
};

const Keys = ({ value, mac }: { value: string; mac: boolean }) => (
  <span className="keys">
    {shortcutLabel(value, mac)
      .split("+")
      .map((key) => (
        <kbd key={key}>{key}</kbd>
      ))}
  </span>
);

const Check = ({
  mark,
  children,
  detail = "",
}: {
  mark: Mark;
  children: React.ReactNode;
  detail?: string;
}) => (
  <li data-mark={mark}>
    <span className="mark" aria-hidden="true">
      {mark === "done" ? <Icon name="check" /> : null}
      {mark === "failed" ? <Icon name="close" /> : null}
    </span>
    <span>
      {children}
      {detail === "" ? null : <small>{detail}</small>}
    </span>
  </li>
);

const MicrophoneStatus = ({
  host,
  level,
  loudest,
}: {
  host: HostState | undefined;
  level: number | null;
  loudest: number;
}) => {
  const previewError = host?.preview_error ?? "";

  if (previewError !== "") {
    return <span className="action-error">{previewError}</span>;
  }

  if (loudest >= heard) {
    return (
      <>
        <Icon name="check" /> Sounds good.
      </>
    );
  }

  return level === null
    ? "Opening the microphone…"
    : "Say a few words. The bars should move while you talk.";
};

const MicrophoneStep = ({
  host,
  devices,
  microphone,
  loudest,
  pick,
  send,
}: {
  host: HostState | undefined;
  devices: readonly InputDevice[];
  microphone: string;
  loudest: number;
  pick: (microphone: string) => void;
  send: Send;
}) => {
  const level = host?.preview ?? null;
  const permission = host?.permissions.microphone;

  return (
    <>
      <label className="field">
        <span>Input</span>
        <select
          value={microphone}
          onChange={(event) => {
            pick(event.target.value);
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
      <output className="setup-status">
        <MicrophoneStatus host={host} level={level} loudest={loudest} />
      </output>
      {permission === undefined || permission === "authorized" ? null : (
        <div className="setting-row">
          <span>
            Microphone access
            <small>{statusText.get(permission) ?? permission}</small>
          </span>
          <button
            type="button"
            onClick={() => {
              void send({ type: "permission", permission: "microphone" });
            }}
          >
            Allow
          </button>
        </div>
      )}
    </>
  );
};

const downloadSize = (download: DownloadState | undefined) => {
  const total = download?.total ?? 0;

  if (download === undefined || total === 0) {
    return "";
  }

  return download.state === "ready"
    ? ` · ${mib(total)}`
    : ` · ${mib(download.received)} of ${mib(total)}`;
};

const downloadAction = (download: DownloadState | undefined) => {
  if (download?.state === "failed") {
    return "Retry";
  }

  return download?.state === "ready" ? "Verify again" : "Download";
};

const downloadProgress = (download: DownloadState | undefined) => {
  if (download?.state === "ready") {
    return 1;
  }

  const total = download?.total ?? 0;

  return download === undefined || total === 0 ? undefined : download.received / total;
};

const ModelStep = ({
  settings,
  host,
  send,
}: {
  settings: Settings | null;
  host: HostState | undefined;
  send: Send;
}) => {
  const download = host?.download;

  return (
    <>
      <div className="setting-row">
        <span>
          <strong>{settings?.engine ?? "…"}</strong>
          <small>
            {downloadText.get(download?.state ?? "") ?? "Waiting"}
            {downloadSize(download)}
          </small>
        </span>
        <button
          type="button"
          disabled={download === undefined || busy.has(download.state)}
          onClick={() => {
            void send({ type: "download_model" });
          }}
        >
          {downloadAction(download)}
        </button>
      </div>
      <progress aria-label="Model download" value={downloadProgress(download)} max={1} />
      {(download?.error ?? "") === "" ? null : (
        <p className="action-error" role="alert">
          {download?.error}
        </p>
      )}
      <p className="quiet setup-note">
        {host?.models.find((model) => model.id === settings?.engine)?.license ?? ""}
      </p>
    </>
  );
};

const PermissionsStep = ({
  permissions,
  send,
}: {
  permissions: readonly (readonly [string, string])[];
  send: Send;
}) => (
  <div className="permission-list">
    {permissions.map(([permission, status]) => (
      <div className="setting-row" key={permission}>
        <span>
          {permissionName(permission)}
          {required.has(permission) ? null : <span className="quiet"> · optional</span>}
          <small>{permissionText.get(permission)?.why ?? ""}</small>
        </span>
        <span className="actions">
          <span className="permission-status" data-granted={status === "authorized"}>
            {status === "authorized" ? <Icon name="check" /> : null}
            {statusText.get(status) ?? status}
          </span>
          {status === "authorized" ? null : (
            <button
              type="button"
              onClick={() => {
                void send({ type: "permission", permission });
              }}
            >
              {permission === "microphone" ? "Allow" : "Open Settings"}
            </button>
          )}
        </span>
      </div>
    ))}
  </div>
);

const TryChecks = ({
  trial,
  host,
  heardTrial,
  delivered,
}: {
  trial: SessionView | undefined;
  host: HostState | undefined;
  heardTrial: boolean;
  delivered: boolean;
}) => {
  const clipboard = host?.input === "clipboard";
  const text = trial ? (trial.edited ?? trial.committed + trial.partial).trim() : "";
  const settled = trial !== undefined && !unsettled.has(trial.phase);
  const recognized = settled && text !== "";
  const started = trial !== undefined;

  const pasteMark = (): Mark => {
    if (delivered) {
      return "done";
    }

    if (!recognized) {
      return "todo";
    }

    return clipboard ? "active" : "failed";
  };

  const pasteDetail = () => {
    if (delivered || !recognized) {
      return "";
    }

    return clipboard
      ? "Copied. Press Ctrl+V in the box to paste it."
      : pasteProblem(trial === undefined ? undefined : host?.notifications[trial.id]);
  };

  const shortcutCheck = () => {
    if (started) {
      return "Shortcut detected";
    }

    return clipboard ? "Press Start" : "Press the shortcut";
  };

  return (
    <ul className="checks" aria-label="Dictation checks">
      <Check mark={started ? "done" : "active"}>{shortcutCheck()}</Check>
      <Check
        mark={progress(heardTrial || recognized, settled, started)}
        detail={
          settled && text === "" && !heardTrial
            ? "Only silence came through. Check the microphone step."
            : ""
        }
      >
        Hearing you
      </Check>
      <Check
        mark={progress(text !== "" && trial?.phase !== "recording", settled, started)}
        detail={settled && text === "" ? "No speech recognized. Speak a little longer." : text}
      >
        Recognized
      </Check>
      <Check mark={pasteMark()} detail={pasteDetail()}>
        {clipboard ? "Pasted from the clipboard" : "Pasted here"}
      </Check>
    </ul>
  );
};

const recordLabel = (trial: SessionView | undefined, clipboard: boolean) => {
  if (trial?.phase === "recording") {
    return "Stop";
  }

  return clipboard ? "Start" : "Shortcut not working? Record with this button";
};

const completedSteps = (
  host: HostState | undefined,
  done: Pick<Record<Step, boolean>, "microphone" | "permissions" | "shortcut" | "try">,
): Record<Step, boolean> => {
  const complete = host?.setup_complete === true;

  return {
    microphone: (done.microphone || complete) && host?.permissions.microphone !== "denied",
    model: host?.download.state === "ready",
    permissions: done.permissions,
    shortcut: done.shortcut,
    try: done.try,
    done: complete,
  };
};

const leads: Record<Exclude<Step, "try">, string> = {
  microphone: "Pick the microphone said should listen to, then say a few words.",
  model:
    "said turns speech into text on this computer. The model downloads once; nothing you say leaves your machine.",
  permissions:
    "macOS asks before an app can hear you or paste for you. This page updates as soon as you allow each one.",
  shortcut: "This is the key you will use to dictate in any app.",
  done: "said is ready.",
};

const Lead = ({
  step,
  input,
  hotkey,
  mac,
}: {
  step: Step;
  input: string | undefined;
  hotkey: string;
  mac: boolean;
}) => {
  if (step !== "try") {
    return leads[step];
  }

  if (input === "clipboard") {
    return (
      <>
        Press Start, say a sentence, then Stop. said copies the text; paste it below with{" "}
        <kbd>Ctrl</kbd> <kbd>V</kbd>.
      </>
    );
  }

  if (input === "toggle") {
    return (
      <>
        Click in the box, press <Keys value={hotkey} mac={mac} />, say a sentence, then press it
        again.
      </>
    );
  }

  return (
    <>
      Click in the box, hold <Keys value={hotkey} mac={mac} />, say a sentence, then let go.
    </>
  );
};

const StepNav = ({
  steps,
  step,
  done,
  go,
}: {
  steps: readonly Step[];
  step: Step;
  done: Record<Step, boolean>;
  go: (step: Step) => void;
}) => (
  <nav className="setup-steps" aria-label="Setup steps">
    <ol>
      {steps.map((item, position) => (
        <li key={item}>
          <button
            type="button"
            aria-current={item === step ? "step" : undefined}
            data-done={done[item]}
            onClick={() => {
              go(item);
            }}
          >
            {done[item] ? (
              <Icon name="check" />
            ) : (
              <span className="step-number">{position + 1}</span>
            )}
            {titles[item]}
          </button>
        </li>
      ))}
    </ol>
  </nav>
);

const ShortcutStep = ({
  configured,
  host,
  mac,
  dirty,
  invalid,
  change,
  save,
}: {
  configured: Settings | null;
  host: HostState | undefined;
  mac: boolean;
  dirty: boolean;
  invalid: boolean;
  change: (patch: Partial<Settings>) => void;
  save: () => void;
}) => (
  <>
    {configured ? <ShortcutFields settings={configured} mac={mac} onChange={change} /> : null}
    <div className="actions end">
      <button type="button" disabled={!dirty || invalid} onClick={save}>
        {dirty ? "Save shortcuts" : "Saved"}
      </button>
    </div>
    <DesktopStatus host={host} />
  </>
);

const TryStep = ({
  trial,
  host,
  heardTrial,
  delivered,
  practice,
  write,
  paste,
  send,
}: {
  trial: SessionView | undefined;
  host: HostState | undefined;
  heardTrial: boolean;
  delivered: boolean;
  practice: string;
  write: (text: string) => void;
  paste: (trial: string) => void;
  send: Send;
}) => {
  const field = useRef<HTMLTextAreaElement>(null);
  const clipboard = host?.input === "clipboard";

  return (
    <>
      <textarea
        ref={field}
        className="practice"
        aria-label="Practice field"
        autoFocus
        value={practice}
        onChange={(event) => {
          write(event.target.value);
        }}
        onPaste={() => {
          if (trial && !unsettled.has(trial.phase)) {
            paste(trial.id);
          }
        }}
        placeholder="Your words land here."
        spellCheck={false}
      />
      <TryChecks trial={trial} host={host} heardTrial={heardTrial} delivered={delivered} />
      <div className="actions">
        <button
          type="button"
          className={clipboard ? undefined : "quiet"}
          disabled={host?.download.state !== "ready"}
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onClick={() => {
            field.current?.focus();
            void send(
              trial?.phase === "recording" ? { type: "stop" } : { type: "start", mode: "quick" },
            );
          }}
        >
          {recordLabel(trial, clipboard)}
        </button>
      </div>
    </>
  );
};

const DoneStep = ({
  configured,
  host,
  mac,
}: {
  configured: Settings | null;
  host: HostState | undefined;
  mac: boolean;
}) => (
  <>
    <dl className="shortcut-summary">
      <dt>Dictate</dt>
      <dd>
        {configured ? <Keys value={configured.hotkey} mac={mac} /> : null}
        <small>
          {host?.input === "hold"
            ? "Hold, speak, let go. The text is pasted where your cursor is."
            : "Press, speak, press again."}
        </small>
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
      said keeps running in the {mac ? "menu bar" : "tray"}. History, Settings and this setup are
      there.
    </p>
  </>
);

const SetupFoot = ({
  step,
  previous,
  blocked,
  invalidShortcut,
  go,
  advance,
}: {
  step: Step;
  previous: Step | undefined;
  blocked: readonly [string, Step] | null;
  invalidShortcut: boolean;
  go: (step: Step) => void;
  advance: () => void;
}) => {
  const forward = () => {
    if (step === "done") {
      return (
        <button
          type="button"
          className="primary"
          onClick={() => {
            if (isTauri()) {
              void getCurrentWindow().close();
            }
          }}
        >
          Close
        </button>
      );
    }

    if (step === "try") {
      return (
        <>
          {blocked ? (
            <button
              type="button"
              className="quiet setup-reason"
              onClick={() => {
                go(blocked[1]);
              }}
            >
              {blocked[0]}
              <Icon name="arrow" />
            </button>
          ) : null}
          <button type="button" className="primary" disabled={blocked !== null} onClick={advance}>
            Finish setup
            <Icon name="check" />
          </button>
        </>
      );
    }

    const stuck = step === "shortcut" && invalidShortcut;

    return (
      <>
        {stuck ? <span className="setup-reason quiet">Fix the shortcut to continue</span> : null}
        <button type="button" className="primary" disabled={stuck} onClick={advance}>
          Continue
          <Icon name="arrow" />
        </button>
      </>
    );
  };

  return (
    <div className="setup-foot">
      {previous === undefined ? null : (
        <button
          type="button"
          className="quiet"
          onClick={() => {
            go(previous);
          }}
        >
          Back
        </button>
      )}
      <span className="spacer" />
      {forward()}
    </div>
  );
};

const useHostSync = (source: CoreSource, step: Step | null, microphone: string) => {
  const focused = useWindowFocus();
  const previewing = focused && step === "microphone";
  const armed = focused && step === "try";

  useImperative(() => {
    quietly(source, {
      type: "preview",
      active: previewing,
      microphone: microphone === "" ? null : microphone,
    });
  }, [source, previewing, microphone]);

  useImperative(() => {
    quietly(source, { type: "trial", armed });
  }, [source, armed]);

  useSubscription(
    () => () => {
      quietly(source, { type: "preview", active: false, microphone: null });
      quietly(source, { type: "trial", armed: false });
    },
    [source],
  );
};

const useLoudest = (level: number | null) => {
  const [meter, setMeter] = useState({ level, loudest: level ?? 0 });

  if (meter.level !== level) {
    setMeter({ level, loudest: level === null ? meter.loudest : Math.max(meter.loudest, level) });
  }

  return [
    meter.loudest,
    () => {
      setMeter({ level, loudest: 0 });
    },
  ] as const;
};

const useTrialPeak = (trial: SessionView | undefined) => {
  const [peak, setPeak] = useState({ id: "", peak: 0 });

  if (
    trial !== undefined &&
    trial.level !== null &&
    (peak.id !== trial.id || trial.level > peak.peak)
  ) {
    setPeak({ id: trial.id, peak: trial.level });
  }

  return peak.id === trial?.id ? peak.peak : 0;
};

const useAutoAdvance = (step: Step, granted: boolean, go: (step: Step) => void) => {
  const [seen, setSeen] = useState({ step, granted, advancing: false });

  if (seen.step !== step || seen.granted !== granted) {
    setSeen({ step, granted, advancing: step === "permissions" && granted && !seen.granted });
  }

  useTimeout(
    () => {
      go("shortcut");
    },
    seen.advancing ? 700 : null,
  );
};

const useShortcutDraft = (settings: Settings | null, mac: boolean) => {
  const [shortcuts, setShortcuts] = useState<Partial<Settings>>({});
  const configured = settings ? { ...settings, ...shortcuts } : null;

  return {
    configured,
    invalidShortcut: configured ? shortcutError(configured, mac) : null,
    dirty:
      settings !== null &&
      shortcutFields.some(
        ([field]) => shortcuts[field] !== undefined && shortcuts[field] !== settings[field],
      ),
    change: (patch: Partial<Settings>) => {
      setShortcuts({ ...shortcuts, ...patch });
    },
    clear: () => {
      setShortcuts({});
    },
  };
};

const deliveredTrial = (
  trial: SessionView | undefined,
  host: HostState | undefined,
  pastedTrial: string,
) =>
  trial !== undefined &&
  (trial.phase === "pasted" || (host?.input === "clipboard" && pastedTrial === trial.id));

const useAutoFinish = (
  trial: SessionView | undefined,
  ready: boolean,
  finish: () => Promise<void>,
) => {
  const [finished, setFinished] = useState("");

  useTimeout(
    () => {
      if (trial) {
        setFinished(trial.id);
        void finish();
      }
    },
    ready && trial !== undefined && finished !== trial.id ? 1200 : null,
  );
};

export const Setup = ({
  settings,
  devices,
  host,
  source,
  send,
}: {
  settings: Settings | null;
  devices: readonly InputDevice[];
  host: HostState | undefined;
  source: CoreSource;
  send: Send;
}) => {
  const permissions = Object.entries(host?.permissions ?? {});
  const mac = macHost(host);

  const steps: Step[] = [
    "microphone",
    "model",
    ...(permissions.some(([name]) => name !== "microphone") ? (["permissions"] as const) : []),
    "shortcut",
    "try",
  ];

  const [step, setStep] = useState<Step>("microphone");
  const [microphone, setMicrophone] = useState(settings?.microphone ?? "");
  const [practice, setPractice] = useState("");
  const [pastedTrial, setPastedTrial] = useState("");
  const [loudest, resetLoudest] = useLoudest(host?.preview ?? null);
  const { configured, invalidShortcut, dirty, change, clear } = useShortcutDraft(settings, mac);

  const missing = permissions
    .filter(([name, status]) => required.has(name) && status !== "authorized")
    .map(([name]) => name);

  const blocked = blockedBy(host, missing, invalidShortcut);
  const trial = source.getSnapshot().sessions.find((session) => session.id === host?.trial);
  const heardTrial = useTrialPeak(trial) >= heard;
  const delivered = deliveredTrial(trial, host, pastedTrial);

  const granted = permissions.length > 0 && missing.length === 0;

  useHostSync(source, host ? step : null, microphone);

  useAutoAdvance(step, granted, setStep);

  const save = async (patch: Partial<Settings> = {}) => {
    if (!configured || invalidShortcut !== null) {
      return true;
    }

    const saved = await send({
      type: "settings_save",
      settings: { ...configured, microphone: microphone === "" ? null : microphone, ...patch },
    });

    if (saved) {
      clear();
    }

    return saved;
  };

  const finish = async () => {
    if (dirty && !(await save())) {
      return;
    }

    if (host?.setup_complete !== true && !(await send({ type: "setup_complete" }))) {
      return;
    }

    setStep("done");
  };

  useAutoFinish(trial, delivered && blocked === null, finish);

  const advance = async () => {
    if (step === "try") {
      await finish();
    } else if (step === "shortcut" && dirty) {
      if (await save()) {
        setStep("try");
      }
    } else {
      setStep(steps[steps.indexOf(step) + 1] ?? "try");
    }
  };

  const doneSteps = completedSteps(host, {
    microphone: loudest >= heard,
    permissions: granted,
    shortcut: settings !== null && !dirty && invalidShortcut === null,
    try: delivered,
  });

  const body: Record<Step, React.ReactNode> = {
    microphone: (
      <MicrophoneStep
        host={host}
        devices={devices}
        microphone={microphone}
        loudest={loudest}
        send={send}
        pick={(value) => {
          setMicrophone(value);
          resetLoudest();
          void save({ microphone: value === "" ? null : value });
        }}
      />
    ),
    model: <ModelStep settings={settings} host={host} send={send} />,
    permissions: <PermissionsStep permissions={permissions} send={send} />,
    shortcut: (
      <ShortcutStep
        configured={configured}
        host={host}
        mac={mac}
        dirty={dirty}
        invalid={invalidShortcut !== null}
        change={change}
        save={() => {
          void save();
        }}
      />
    ),
    try: (
      <TryStep
        trial={trial}
        host={host}
        heardTrial={heardTrial}
        delivered={delivered}
        practice={practice}
        write={setPractice}
        paste={setPastedTrial}
        send={send}
      />
    ),
    done: <DoneStep configured={configured} host={host} mac={mac} />,
  };

  const index = steps.indexOf(step);

  return (
    <section className="setup">
      {step === "done" ? null : <StepNav steps={steps} step={step} done={doneSteps} go={setStep} />}
      <div className="setup-content" key={step}>
        <h1>{step === "done" ? "You are set" : titles[step]}</h1>
        <p className="setup-lead">
          <Lead step={step} input={host?.input} hotkey={configured?.hotkey ?? ""} mac={mac} />
        </p>
        {body[step]}
      </div>
      <SetupFoot
        step={step}
        previous={index > 0 ? steps[index - 1] : undefined}
        blocked={blocked}
        invalidShortcut={invalidShortcut !== null}
        go={setStep}
        advance={() => {
          void advance();
        }}
      />
    </section>
  );
};
