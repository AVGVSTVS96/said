import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";

import { App } from "../src/app";
import { createFakeSource, fakeHost } from "../src/fake";
import { Interactive } from "../src/interactive";
import { createCoreSource } from "../src/source";

const frame = async () => {
  const next = Promise.withResolvers<number>();
  requestAnimationFrame(next.resolve);
  await next.promise;
};

const found = <T,>(value: T | null | undefined, what: string): T => {
  if (value === null || value === undefined) {
    throw new Error(`Missing ${what}`);
  }

  return value;
};

export const setupRegression = async (mac = false) => {
  const fixture = createFakeSource({ mac }).getSnapshot();
  const source = createCoreSource(() => {});
  source.receive({
    ...fixture,
    host: {
      ...fakeHost(mac, "ready"),
      permissions: mac
        ? { microphone: "denied", accessibility: "denied", input_monitoring: "denied" }
        : {},
      instructions: [
        "Toggle mode. Press a shortcut to start and again to finish.",
        "Hold-to-talk requires input access or a working GlobalShortcuts portal.",
        'binds { Super+Z { spawn "said" "hotkey"; } }',
      ],
    },
  });
  const mount = document.createElement("div");
  document.body.replaceChildren(mount);
  const root = createRoot(mount);
  flushSync(() => {
    root.render(<App source={source} />);
  });
  await frame();
  const steps = [...mount.querySelectorAll<HTMLButtonElement>(".setup-steps button")];
  const footers = [];

  for (const step of steps) {
    flushSync(() => {
      step.click();
    });
    // oxlint-disable-next-line eslint/no-await-in-loop -- each step has to paint before its footer is measured
    await frame();
    const primary = found(mount.querySelector(".setup-foot .primary"), "primary button");
    const bounds = primary.getBoundingClientRect();

    if (
      bounds.top < 0 ||
      bounds.bottom > innerHeight ||
      document.documentElement.scrollHeight > innerHeight ||
      document.documentElement.scrollWidth > innerWidth
    ) {
      throw new Error(`${step.textContent} step needs scrolling to reach ${primary.textContent}`);
    }

    footers.push(`${step.textContent}: ${primary.textContent}`);
  }

  const finish = found(
    mount.querySelector<HTMLButtonElement>(".setup-foot .primary"),
    "Finish setup",
  );

  const reason = mount.querySelector(".setup-reason")?.textContent ?? "";

  if (
    !finish.textContent.includes("Finish setup") ||
    finish.disabled !== mac ||
    (mac && !reason.includes("Allow Microphone and Accessibility"))
  ) {
    throw new Error(`Finish setup is not reachable with a reason: ${finish.textContent} ${reason}`);
  }

  const result = { mac, width: innerWidth, height: innerHeight, footers, reason };
  root.unmount();

  return result;
};

export const interactiveRegression = async () => {
  const fake = createFakeSource();
  const fixture = fake.getSnapshot();
  const session = { ...fake.session, phase: "reviewing" as const, partial: "" };
  let succeed = false;
  const closes: string[] = [];
  let release = Promise.withResolvers<null>();

  const source = createCoreSource(async () => {
    await release.promise;

    if (!succeed) {
      throw new Error("Synthetic target unavailable");
    }

    source.receive({ ...fixture, sessions: [{ ...session, phase: "pasted" }] });
  });

  source.receive({ ...fixture, sessions: [session] });

  const Review = () => {
    const [failure, setFailure] = useState("");

    return (
      <Interactive
        session={session}
        utterance={undefined}
        source={source}
        draft={null}
        onEdit={() => {}}
        send={async (action) => {
          try {
            await source.dispatch(action);
          } catch (error) {
            setFailure(String(error));

            return false;
          }

          return true;
        }}
        onClose={() => {
          closes.push("closed");
        }}
        note={failure === "" ? null : <span role="alert">{failure}</span>}
      />
    );
  };

  const mount = document.createElement("div");
  document.body.replaceChildren(mount);
  const root = createRoot(mount);
  flushSync(() => {
    root.render(<Review />);
  });

  const insert = () => {
    found(
      [...mount.querySelectorAll("button")].find(
        (button) => button.textContent.trim() === "Insert",
      ),
      "Insert button",
    ).click();
  };

  insert();
  await frame();

  if (closes.length > 0) {
    throw new Error("Review closed before delivery completed");
  }

  release.resolve(null);
  await frame();

  if (
    closes.length > 0 ||
    mount.querySelector('[role="alert"]')?.textContent.includes("Synthetic target unavailable") !==
      true
  ) {
    throw new Error("Failed Insert closed Review or hid its reason");
  }

  succeed = true;
  release = Promise.withResolvers<null>();
  insert();
  await frame();
  release.resolve(null);
  await frame();

  if (closes.length !== 1) {
    throw new Error("Successful Insert did not close Review exactly once");
  }

  root.unmount();

  return {
    passed:
      "Review stays open while pending and on failure with its reason; closes after successful Insert",
  };
};
