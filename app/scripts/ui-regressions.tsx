import { useState } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { App } from "../src/App";
import { Interactive } from "../src/Interactive";
import { createFakeSource } from "../src/fake";
import { createCoreSource } from "../src/source";

const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

export async function setupRegression(mac = false) {
  const fixture = createFakeSource({ mac }).getSnapshot();
  const source = createCoreSource(async () => {});
  source.receive({
    ...fixture,
    host: {
      ...fixture.host!,
      permissions: mac ? { microphone: "denied", accessibility: "denied", input_monitoring: "denied" } : {},
      instructions: ["Toggle mode. Press a shortcut to start and again to finish.", "Hold-to-talk requires input access or a working GlobalShortcuts portal.", 'binds { Super+Z { spawn "said" "hotkey"; } }'],
    },
  });
  const mount = document.createElement("div");
  document.body.replaceChildren(mount);
  const root = createRoot(mount);
  flushSync(() => root.render(<App source={source} />));
  await frame();
  const steps = [...mount.querySelectorAll<HTMLButtonElement>(".setup-steps button")];
  const footers = [];
  for (const step of steps) {
    flushSync(() => step.click());
    await frame();
    const primary = mount.querySelector(".setup-foot .primary")!;
    const bounds = primary.getBoundingClientRect();
    if (bounds.top < 0 || bounds.bottom > innerHeight || document.documentElement.scrollHeight > innerHeight || document.documentElement.scrollWidth > innerWidth) {
      throw new Error(`${step.textContent} step needs scrolling to reach ${primary.textContent}`);
    }
    footers.push(`${step.textContent}: ${primary.textContent}`);
  }
  const finish = mount.querySelector<HTMLButtonElement>(".setup-foot .primary")!;
  const reason = mount.querySelector(".setup-reason")?.textContent ?? "";
  if (!finish.textContent?.includes("Finish setup") || finish.disabled !== mac || (mac && !reason.includes("Allow Microphone and Accessibility"))) {
    throw new Error(`Finish setup is not reachable with a reason: ${finish.textContent} ${reason}`);
  }
  const result = { mac, width: innerWidth, height: innerHeight, footers, reason };
  root.unmount();
  return result;
}

export async function interactiveRegression() {
  const fixture = createFakeSource().getSnapshot();
  const session = { ...fixture.sessions[0], phase: "reviewing" as const, partial: "" };
  let succeed = false;
  let closed = 0;
  let release: (() => void) | undefined;
  const source = createCoreSource(async () => {
    await new Promise<void>((resolve) => { release = resolve; });
    if (!succeed) throw new Error("Synthetic target unavailable");
    source.receive({ ...fixture, sessions: [{ ...session, phase: "pasted" }] });
  });
  source.receive({ ...fixture, sessions: [session] });
  function Review() {
    const [error, setError] = useState("");
    return <Interactive session={session} source={source} draft={null} onEdit={() => {}}
      send={async (action) => {
        try { await source.dispatch(action); }
        catch (error) { setError(String(error)); throw error; }
      }}
      onClose={() => { closed++; }} note={error ? <span role="alert">{error}</span> : null} />;
  }
  const mount = document.createElement("div");
  document.body.replaceChildren(mount);
  const root = createRoot(mount);
  flushSync(() => root.render(<Review />));
  const insert = () => [...mount.querySelectorAll("button")].find((button) => button.textContent?.trim() === "Insert")!.click();
  insert();
  await frame();
  if (closed !== 0) throw new Error("Review closed before delivery completed");
  release!();
  await frame();
  if (closed !== 0 || !mount.querySelector('[role="alert"]')?.textContent?.includes("Synthetic target unavailable")) {
    throw new Error("Failed Insert closed Review or hid its reason");
  }
  succeed = true;
  insert();
  await frame();
  release!();
  await frame();
  if (Number(closed) !== 1) throw new Error("Successful Insert did not close Review exactly once");
  root.unmount();
  return { passed: "Review stays open while pending and on failure with its reason; closes after successful Insert" };
}
