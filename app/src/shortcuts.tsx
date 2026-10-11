import type { HostState, Settings } from "./contracts";

const fields = [
  ["hotkey", "Dictation"],
  ["interactive_hotkey", "Interactive"],
  ["insert_last_hotkey", "Insert last"],
] as const;

export function shortcutLabel(value: string, mac: boolean) {
  return value.split("+").map((part) => {
    const lower = part.toLowerCase();
    return ({ ctrl: "Ctrl", control: "Ctrl", alt: mac ? "Option" : "Alt", cmd: "Command", super: "Super", meta: "Super", shift: "Shift", space: "Space" } as Record<string, string>)[lower] ?? part.toUpperCase();
  }).join("+");
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
  if (normalized.includes(null)) return mac ? "Use ctrl, alt, cmd or shift with Space or a letter." : "Use ctrl, alt, super or shift with a letter, number, function key or named key.";
  if (new Set(normalized).size !== 3) return "Choose a different shortcut for each action.";
  return null;
}

export function ShortcutFields({ settings, mac, onChange }: {
  settings: Settings;
  mac: boolean;
  onChange: (patch: Partial<Settings>) => void;
}) {
  const error = shortcutError(settings, mac);
  return <>
    {fields.map(([field, label]) => <label className="setting-row" key={field}>
      <span>{label}<small>{shortcutLabel(settings[field], mac)}</small></span>
      <input aria-label={`${label} shortcut`} value={settings[field]} aria-invalid={Boolean(error)} onChange={(event) => onChange({ [field]: event.target.value })} />
    </label>)}
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
