import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { DesktopStatus, ShortcutFields, recordKey, shortcutError, shortcutKeys, shortcutLabel } from "./shortcuts";
import type { HostState, Settings } from "./contracts";

const settings: Settings = {
  hotkey: "super+z", interactive_hotkey: "super+x", insert_last_hotkey: "super+a",
  engine: "local-model", microphone: null, apps: {},
};

test("all configured shortcuts are displayed and invalid or duplicate chords cannot save", () => {
  expect(shortcutError(settings, false)).toBeNull();
  expect(shortcutError({ ...settings, interactive_hotkey: "SUPER+Z" }, false)).toContain("different");
  expect(shortcutError({ ...settings, hotkey: "super+unknown" }, false)).toContain("named key");
  const html = renderToStaticMarkup(<ShortcutFields settings={settings} mac={false} onChange={() => {}} />);
  for (const key of ["Super+Z", "Super+X", "Super+A"]) expect(html).toContain(key);
});

test("the recorder turns physical keys into the settings syntax and asks for a modifier", () => {
  const press = (code: string, held: Partial<KeyboardEvent> = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...held });
  expect(recordKey(press("Space", { altKey: true }), true)).toEqual({ shortcut: "alt+space" });
  expect(recordKey(press("KeyZ", { metaKey: true }), false)).toEqual({ shortcut: "super+z" });
  expect(recordKey(press("KeyK", { metaKey: true, shiftKey: true }), true)).toEqual({ shortcut: "shift+cmd+k" });
  expect(recordKey(press("KeyZ"), false, new Set(["meta"]))).toEqual({ shortcut: "super+z" });
  expect(recordKey(press("F5", { ctrlKey: true }), false)).toEqual({ shortcut: "ctrl+f5" });
  expect(recordKey(press("Period", { ctrlKey: true }), false)).toEqual({ shortcut: "ctrl+dot" });
  expect(recordKey(press("Escape"), false)).toBeNull();
  expect(recordKey(press("KeyA"), true)).toHaveProperty("prompt");
  expect(recordKey(press("ArrowUp", { altKey: true }), true)).toHaveProperty("prompt");
  expect(shortcutError({ ...settings, hotkey: "shift+cmd+k", interactive_hotkey: "alt+x", insert_last_hotkey: "alt+a" }, true)).toBeNull();
});

test("recorder labels use Mac symbols and spelled-out names elsewhere", () => {
  expect(shortcutKeys("alt+space", true)).toBe("⌥Space");
  expect(shortcutKeys("ctrl+shift+cmd+k", true)).toBe("⌃⇧⌘K");
  expect(shortcutKeys("super+z", false)).toBe("Super+Z");
  expect(shortcutKeys("ctrl+leftbrace", false)).toBe("Ctrl+[");
  const html = renderToStaticMarkup(<ShortcutFields settings={{ ...settings, hotkey: "alt+space", interactive_hotkey: "alt+x", insert_last_hotkey: "alt+a" }} mac onChange={() => {}} />);
  expect(html).toContain("⌥Space");
  expect(html).toContain('aria-label="Dictation shortcut: Option+Space"');
});

test("Mac insert hints name Option and use the configured key", () => {
  expect(shortcutLabel("alt+a", true)).toBe("Option+A");
  expect(shortcutLabel("cmd+shift+x", true)).toBe("Command+Shift+X");
});

test("toggle capability and its cause are visible without expanding desktop setup", () => {
  const host = { instructions: ["Toggle mode. Press again to stop.", "No readable physical keyboards.", "binds { Super+Z { spawn \"said\" \"hotkey\"; } }"] } as HostState;
  const html = renderToStaticMarkup(<DesktopStatus host={host} />);
  expect(html).toContain('<p role="status">Toggle mode. Press again to stop.</p>');
  expect(html.indexOf("No readable physical keyboards")).toBeLessThan(html.indexOf("<details>"));
});
