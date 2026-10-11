import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { DesktopStatus, ShortcutFields, shortcutError, shortcutLabel } from "./shortcuts";
import type { HostState, Settings } from "./contracts";

const settings: Settings = {
  hotkey: "super+z", interactive_hotkey: "super+x", insert_last_hotkey: "super+a",
  engine: "local-model", vocabulary: false, microphone: null, apps: {},
};

test("all configured shortcuts are displayed and invalid or duplicate chords cannot save", () => {
  expect(shortcutError(settings, false)).toBeNull();
  expect(shortcutError({ ...settings, interactive_hotkey: "SUPER+Z" }, false)).toContain("different");
  expect(shortcutError({ ...settings, hotkey: "super+unknown" }, false)).toContain("named key");
  const html = renderToStaticMarkup(<ShortcutFields settings={settings} mac={false} onChange={() => {}} />);
  for (const key of ["Super+Z", "Super+X", "Super+A"]) expect(html).toContain(key);
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
