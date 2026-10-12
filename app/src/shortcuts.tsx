import { useState } from "react";
import type { HostState, Settings } from "./contracts";

const fields = [
  ["hotkey", "Dictation"],
  ["interactive_hotkey", "Interactive"],
  ["insert_last_hotkey", "Insert last"],
] as const;

const names: Record<string, string> = {
  ctrl: "Ctrl", control: "Ctrl", alt: "Alt", cmd: "Command", super: "Super", meta: "Super", shift: "Shift",
  space: "Space", tab: "Tab", enter: "Enter", esc: "Esc", grave: "`", minus: "-", equal: "=", leftbrace: "[",
  rightbrace: "]", semicolon: ";", apostrophe: "'", comma: ",", dot: ".", slash: "/", backslash: "\\",
};
const symbols: Record<string, string> = { ctrl: "⌃", control: "⌃", alt: "⌥", shift: "⇧", cmd: "⌘" };
const codes: Record<string, string> = {
  Space: "space", Tab: "tab", Enter: "enter", Escape: "esc", Backquote: "grave", Minus: "minus", Equal: "equal",
  BracketLeft: "leftbrace", BracketRight: "rightbrace", Semicolon: "semicolon", Quote: "apostrophe",
  Comma: "comma", Period: "dot", Slash: "slash", Backslash: "backslash",
};
const modifierKeys: Record<string, string> = {
  Control: "ctrl", Alt: "alt", AltGraph: "alt", Shift: "shift", Meta: "meta", OS: "meta", Super: "meta", Hyper: "meta",
};

export function shortcutLabel(value: string, mac: boolean) {
  return value.split("+").map((part) => {
    const lower = part.toLowerCase();
    return lower === "alt" && mac ? "Option" : names[lower] ?? part.toUpperCase();
  }).join("+");
}

export function shortcutKeys(value: string, mac: boolean) {
  return mac
    ? value.split("+").map((part) => symbols[part.toLowerCase()] ?? shortcutLabel(part, mac)).join("")
    : shortcutLabel(value, mac);
}

type Keys = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

function modifiers(event: Partial<Keys>, mac: boolean, held: ReadonlySet<string>) {
  return [
    (event.ctrlKey || held.has("ctrl")) && "ctrl",
    (event.altKey || held.has("alt")) && "alt",
    (event.shiftKey || held.has("shift")) && "shift",
    (event.metaKey || held.has("meta")) && (mac ? "cmd" : "super"),
  ].filter((part): part is string => Boolean(part));
}

export function recordKey(event: Keys, mac: boolean, held: ReadonlySet<string> = new Set()) {
  const { code } = event;
  const chord = modifiers(event, mac, held);
  const key = /^Key[A-Z]$/.test(code) ? code.slice(3).toLowerCase()
    : /^Digit[0-9]$/.test(code) ? code.slice(5)
    : /^F([1-9]|1[0-9]|2[0-4])$/.test(code) ? code.toLowerCase()
    : codes[code];
  if (code === "Escape" && !chord.length) return null;
  if (!key) return { prompt: "That key can't be used" };
  if (!chord.length) return { prompt: `Hold ${mac ? "⌃, ⌥, ⇧ or ⌘" : "Ctrl, Alt, Super or Shift"} too` };
  return { shortcut: [...chord, key].join("+") };
}

export function shortcutError(settings: Settings, mac: boolean) {
  const shortcuts = fields.map(([field]) => settings[field].toLowerCase());
  const modifiers = mac ? ["ctrl", "alt", "cmd", "shift"] : ["ctrl", "control", "alt", "super", "meta", "shift"];
  const normalized = shortcuts.map((value) => {
    const parts = value.split("+");
    const key = parts.pop() ?? "";
    const keys = mac ? /^(space|[a-z])$/ : /^(space|[a-z0-9]|f([1-9]|1[0-9]|2[0-4])|tab|enter|esc|grave|minus|equal|leftbrace|rightbrace|semicolon|apostrophe|comma|dot|slash|backslash)$/;
    if (!keys.test(key) || !parts.length || parts.some((part) => !modifiers.includes(part))) return null;
    return [...new Set(parts.map((part) => part === "control" ? "ctrl" : part === "meta" ? "super" : part))].sort().concat(key).join("+");
  });
  if (normalized.includes(null)) return mac ? "Use Space or a letter together with ⌃, ⌥, ⇧ or ⌘." : "Use a letter, number, function key or named key together with Ctrl, Alt, Super or Shift.";
  if (new Set(normalized).size !== 3) return "Choose a different shortcut for each action.";
  return null;
}

function ShortcutRecorder({ label, value, mac, onChange }: {
  label: string;
  value: string;
  mac: boolean;
  onChange: (value: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<ReadonlySet<string>>(new Set());
  const [prompt, setPrompt] = useState("");
  const stop = () => {
    setRecording(false);
    setHeld(new Set());
    setPrompt("");
  };
  const pressed = modifiers({}, mac, held);
  return <button
    type="button"
    className="shortcut"
    aria-pressed={recording}
    aria-label={`${label} shortcut: ${recording ? "press the new keys, or Escape to cancel" : shortcutLabel(value, mac)}`}
    onClick={() => recording ? stop() : setRecording(true)}
    onBlur={stop}
    onKeyUp={(event) => {
      const modifier = modifierKeys[event.key];
      if (recording && modifier) setHeld((held) => new Set([...held].filter((key) => key !== modifier)));
    }}
    onKeyDown={(event) => {
      if (!recording) return;
      event.preventDefault();
      event.stopPropagation();
      const modifier = modifierKeys[event.key];
      if (modifier) {
        setHeld((held) => new Set(held).add(modifier));
        return setPrompt("");
      }
      const result = recordKey(event, mac, held);
      if (result?.prompt) return setPrompt(result.prompt);
      if (result?.shortcut) onChange(result.shortcut);
      stop();
    }}
  >
    {recording ? prompt || (pressed.length ? `${shortcutKeys(pressed.join("+"), mac)}${mac ? "" : "+"}…` : "Press keys…") : shortcutKeys(value, mac)}
  </button>;
}

export function ShortcutFields({ settings, mac, onChange }: {
  settings: Settings;
  mac: boolean;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const error = shortcutError(settings, mac);
  return <>
    {fields.map(([field, label]) => <div className="setting-row" key={field}>
      <span>{label}</span>
      <ShortcutRecorder label={label} value={settings[field]} mac={mac} onChange={(value) => onChange({ [field]: value })} />
    </div>)}
    {error ? <p role="alert">{error}</p> : null}
  </>;
}

export function DesktopStatus({ host }: { host?: HostState }) {
  return host?.instructions.length ? <div className="desktop-status">
    {host.instructions.map((instruction, index) => instruction.startsWith("binds {")
      ? <details key={instruction}><summary>Niri shortcuts</summary><pre>{instruction}</pre></details>
      : <p key={instruction} role={index === 0 ? "status" : undefined}>{instruction}</p>)}
  </div> : null;
}
