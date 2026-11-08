import { useState } from "react";

import type { HostState, Settings } from "./contracts";

export const shortcutFields = [
  ["hotkey", "Dictation"],
  ["interactive_hotkey", "Interactive"],
  ["insert_last_hotkey", "Insert last"],
] as const;

const names = new Map([
  ["ctrl", "Ctrl"],
  ["control", "Ctrl"],
  ["alt", "Alt"],
  ["cmd", "Command"],
  ["super", "Super"],
  ["meta", "Super"],
  ["shift", "Shift"],
  ["space", "Space"],
  ["tab", "Tab"],
  ["enter", "Enter"],
  ["esc", "Esc"],
  ["grave", "`"],
  ["minus", "-"],
  ["equal", "="],
  ["leftbrace", "["],
  ["rightbrace", "]"],
  ["semicolon", ";"],
  ["apostrophe", "'"],
  ["comma", ","],
  ["dot", "."],
  ["slash", "/"],
  ["backslash", "\\"],
]);

const symbols = new Map([
  ["ctrl", "⌃"],
  ["control", "⌃"],
  ["alt", "⌥"],
  ["shift", "⇧"],
  ["cmd", "⌘"],
]);

const codes = new Map([
  ["Space", "space"],
  ["Tab", "tab"],
  ["Enter", "enter"],
  ["Escape", "esc"],
  ["Backquote", "grave"],
  ["Minus", "minus"],
  ["Equal", "equal"],
  ["BracketLeft", "leftbrace"],
  ["BracketRight", "rightbrace"],
  ["Semicolon", "semicolon"],
  ["Quote", "apostrophe"],
  ["Comma", "comma"],
  ["Period", "dot"],
  ["Slash", "slash"],
  ["Backslash", "backslash"],
]);

const modifierKeys = new Map([
  ["Control", "ctrl"],
  ["Alt", "alt"],
  ["AltGraph", "alt"],
  ["Shift", "shift"],
  ["Meta", "meta"],
  ["OS", "meta"],
  ["Super", "meta"],
  ["Hyper", "meta"],
]);

const aliases = new Map([
  ["control", "ctrl"],
  ["meta", "super"],
]);

export const macHost = (host: HostState | undefined) =>
  host !== undefined && "accessibility" in host.permissions;

export const shortcutLabel = (value: string, mac: boolean) =>
  value
    .split("+")
    .map((part) => {
      const lower = part.toLowerCase();

      return lower === "alt" && mac ? "Option" : (names.get(lower) ?? part.toUpperCase());
    })
    .join("+");

export const shortcutKeys = (value: string, mac: boolean) =>
  mac
    ? value
        .split("+")
        .map((part) => symbols.get(part.toLowerCase()) ?? shortcutLabel(part, mac))
        .join("")
    : shortcutLabel(value, mac);

type Keys = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

const modifiers = (event: Partial<Keys>, mac: boolean, held: ReadonlySet<string>) =>
  (
    [
      [event.ctrlKey, "ctrl", "ctrl"],
      [event.altKey, "alt", "alt"],
      [event.shiftKey, "shift", "shift"],
      [event.metaKey, "meta", mac ? "cmd" : "super"],
    ] as const
  ).flatMap(([pressed, key, name]) => (pressed === true || held.has(key) ? [name] : []));

const keyName = (code: string) => {
  if (/^Key[A-Z]$/u.test(code)) {
    return code.slice(3).toLowerCase();
  }

  if (/^Digit[0-9]$/u.test(code)) {
    return code.slice(5);
  }

  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/u.test(code)) {
    return code.toLowerCase();
  }

  return codes.get(code);
};

export const recordKey = (event: Keys, mac: boolean, held: ReadonlySet<string> = new Set()) => {
  const { code } = event;
  const chord = modifiers(event, mac, held);
  const key = keyName(code);

  if (code === "Escape" && chord.length === 0) {
    return null;
  }

  if (key === undefined) {
    return { prompt: "That key can't be used" };
  }

  if (chord.length === 0) {
    return { prompt: `Hold ${mac ? "⌃, ⌥, ⇧ or ⌘" : "Ctrl, Alt, Super or Shift"} too` };
  }

  return { shortcut: [...chord, key].join("+") };
};

export const shortcutError = (settings: Settings, mac: boolean) => {
  const allowed = mac
    ? ["ctrl", "alt", "cmd", "shift"]
    : ["ctrl", "control", "alt", "super", "meta", "shift"];

  const keys = mac
    ? /^(?:space|[a-z])$/u
    : /^(?:space|[a-z0-9]|f(?:[1-9]|1[0-9]|2[0-4])|tab|enter|esc|grave|minus|equal|leftbrace|rightbrace|semicolon|apostrophe|comma|dot|slash|backslash)$/u;

  const normalized = shortcutFields.map(([field]) => {
    const parts = settings[field].toLowerCase().split("+");
    const key = parts.pop() ?? "";

    if (!keys.test(key) || parts.length === 0 || parts.some((part) => !allowed.includes(part))) {
      return null;
    }

    const chord = [...new Set(parts.map((part) => aliases.get(part) ?? part))].toSorted();

    return [...chord, key].join("+");
  });

  if (normalized.includes(null)) {
    return mac
      ? "Use Space or a letter together with ⌃, ⌥, ⇧ or ⌘."
      : "Use a letter, number, function key or named key together with Ctrl, Alt, Super or Shift.";
  }

  if (new Set(normalized).size !== 3) {
    return "Choose a different shortcut for each action.";
  }

  return null;
};

const ShortcutRecorder = ({
  label,
  value,
  mac,
  onChange,
}: {
  label: string;
  value: string;
  mac: boolean;
  onChange: (value: string) => void;
}) => {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<ReadonlySet<string>>(new Set());
  const [prompt, setPrompt] = useState("");

  const stop = () => {
    setRecording(false);
    setHeld(new Set());
    setPrompt("");
  };

  const pressed = modifiers({}, mac, held);

  const shown = () => {
    if (!recording) {
      return shortcutKeys(value, mac);
    }

    if (prompt !== "") {
      return prompt;
    }

    if (pressed.length === 0) {
      return "Press keys…";
    }

    return `${shortcutKeys(pressed.join("+"), mac)}${mac ? "" : "+"}…`;
  };

  return (
    <button
      type="button"
      className="shortcut"
      aria-pressed={recording}
      aria-label={`${label} shortcut: ${recording ? "press the new keys, or Escape to cancel" : shortcutLabel(value, mac)}`}
      onClick={() => {
        if (recording) {
          stop();
        } else {
          setRecording(true);
        }
      }}
      onBlur={stop}
      onKeyUp={(event) => {
        const modifier = modifierKeys.get(event.key);

        if (recording && modifier !== undefined) {
          setHeld((current) => new Set([...current].filter((key) => key !== modifier)));
        }
      }}
      onKeyDown={(event) => {
        if (!recording) {
          return;
        }

        event.preventDefault();
        event.stopPropagation();
        const modifier = modifierKeys.get(event.key);

        if (modifier !== undefined) {
          setHeld((current) => new Set(current).add(modifier));
          setPrompt("");

          return;
        }

        const result = recordKey(event, mac, held);

        if (result?.prompt !== undefined) {
          setPrompt(result.prompt);

          return;
        }

        if (result?.shortcut !== undefined) {
          onChange(result.shortcut);
        }

        stop();
      }}
    >
      {shown()}
    </button>
  );
};

export const ShortcutFields = ({
  settings,
  mac,
  onChange,
}: {
  settings: Settings;
  mac: boolean;
  onChange: (patch: Partial<Settings>) => void;
}) => {
  const error = shortcutError(settings, mac);

  return (
    <>
      {shortcutFields.map(([field, label]) => (
        <div className="setting-row" key={field}>
          <span>{label}</span>
          <ShortcutRecorder
            label={label}
            value={settings[field]}
            mac={mac}
            onChange={(value) => {
              onChange({ [field]: value });
            }}
          />
        </div>
      ))}
      {error === null ? null : <p role="alert">{error}</p>}
    </>
  );
};

export const DesktopStatus = ({ host }: { host: HostState | undefined }) => {
  if (host === undefined || host.instructions.length === 0) {
    return null;
  }

  return (
    <div className="desktop-status">
      {host.instructions.map((instruction, index) =>
        instruction.startsWith("binds {") ? (
          <details key={instruction}>
            <summary>Niri shortcuts</summary>
            <pre>{instruction}</pre>
          </details>
        ) : (
          <p key={instruction} role={index === 0 ? "status" : undefined}>
            {instruction}
          </p>
        ),
      )}
    </div>
  );
};
