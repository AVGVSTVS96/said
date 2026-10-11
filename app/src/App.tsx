import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { product } from "./product";
import {
  type CoreSource,
  type AppAction,
  disconnectedSource,
  userText,
} from "./source";
import type { createFakeSource } from "./fake";
import { Overlay } from "./Overlay";
import { Interactive } from "./Interactive";
import { History } from "./History";
import { Settings } from "./Settings";
import { Setup } from "./Setup";
import type { Draft } from "./editor";

const screens = [
  "interactive",
  "history",
  "settings",
  "setup",
  "overlay",
] as const;
type Screen = (typeof screens)[number];
type FakeSource = ReturnType<typeof createFakeSource>;
const labels: Record<Screen, string> = {
  interactive: "Interactive",
  history: "History",
  settings: "Settings",
  setup: "Setup",
  overlay: "Overlay",
};
const phases = [
  "recording",
  "finishing",
  "saving",
  "saved",
  "reviewing",
  "pasting",
  "pasted",
  "failed",
] as const;

function FloatingPanel({ children }: { children: React.ReactNode }) {
  const stage = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!isTauri() || !stage.current) return;
    const element = stage.current;
    const observer = new ResizeObserver(() => {
      if (element.offsetHeight)
        void fitWindow(new LogicalSize(element.offsetWidth, element.offsetHeight));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return (
    <main className="stage panel-stage" ref={stage}>
      {children}
    </main>
  );
}

async function fitWindow(size: LogicalSize) {
  const window = getCurrentWindow();
  try {
    await Promise.all([window.setMinSize(size), window.setMaxSize(size)]);
    await window.setSize(size);
    // GTK on Wayland keeps the compositor's size while the window is flagged tiled (niri does
    // that for every window under prefer-no-csd) and applies new bounds only with the next
    // configure; an unmaximize request makes the compositor send one.
    await new Promise(requestAnimationFrame);
    await new Promise(requestAnimationFrame);
    await window.unmaximize();
  } catch {}
}

export function App({
  source = disconnectedSource,
  demo,
}: {
  source?: CoreSource;
  demo?: FakeSource;
}) {
  const params = new URLSearchParams(window.location.search);
  const [screen, setScreen] = useState<Screen>(
    screens.find((screen) => screen === params.get("screen")) ?? "setup",
  );
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [feedback, setFeedback] = useState("");
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const snapshot = useSyncExternalStore(source.subscribe, source.getSnapshot);
  const session =
    screen === "interactive"
      ? [...snapshot.sessions]
          .reverse()
          .find((session) => session.mode === "interactive")
      : snapshot.sessions.at(-1);
  const utterance = selected
    ? snapshot.history.find((row) => row.id === selected)
    : snapshot.history.find((row) => row.id === session?.id);

  useEffect(() => {
    document.title = `${labels[screen]} · ${product.name}`;
    document.documentElement.dataset.screen = screen;
    document.documentElement.dataset.demo = demo ? "true" : "false";
  }, [screen, demo]);

  async function send(action: AppAction) {
    setError("");
    setFeedback("");
    try {
      await source.dispatch(action);
      if (action.type === "edit") {
        const current = source.getSnapshot();
        const accepted =
          userText(
            current.sessions.find((session) => session.id === action.id),
          ) === action.text ||
          current.history.some(
            (row) => row.id === action.id && row.text === action.text,
          );
        if (accepted)
          setDrafts((drafts) => {
            if (drafts[action.id]?.text !== action.text) return drafts;
            const next = { ...drafts };
            delete next[action.id];
            return next;
          });
      } else if (demo) {
        setFeedback(
          `Demo: ${action.type.replaceAll("_", " ")} action recorded. No desktop action was performed.`,
        );
      } else if (action.type === "copy") {
        setFeedback("Copied");
      }
    } catch (error) {
      setError(
        error instanceof Error
          ? error.message
          : "Core could not complete the action.",
      );
      throw error;
    }
  }

  function navigate(next: Screen) {
    setScreen(next);
    setSelected(null);
    setFeedback("");
    setError("");
    const url = new URL(window.location.href);
    url.searchParams.set("screen", next);
    window.history.replaceState(null, "", url);
  }

  const note = error ? (
    <span className="action-error" role="alert">
      {error}
    </span>
  ) : feedback ? (
    <span className="action-feedback" role="status">
      {feedback}
    </span>
  ) : null;

  const editor = (onClose?: () => void) => (
    <Interactive
      key={selected ?? session?.id ?? "empty"}
      session={selected ? undefined : session}
      utterance={utterance}
      source={source}
      send={send}
      draft={drafts[selected ?? session?.id ?? ""] ?? null}
      onEdit={(draft) => {
        const id = selected ?? session?.id;
        if (id) setDrafts((drafts) => ({ ...drafts, [id]: draft }));
      }}
      onClose={onClose}
      note={note}
    />
  );

  if (!source.connected) {
    return <main className="window" role="alert">Host disconnected: {snapshot.connectionError ?? "The core connection is not available."} Restart {product.name} to reconnect.</main>;
  }

  return (
    <>
      {screen === "overlay" ? (
        <Overlay session={session} source={source} send={send} />
      ) : screen === "interactive" ? (
        <FloatingPanel>
          {editor(
            isTauri() ? () => void getCurrentWindow().close() : undefined,
          )}
        </FloatingPanel>
      ) : (
        <main className={`window${screen === "setup" ? " setup-window" : ""}`}>
          {screen === "history" && selected ? (
            editor(() => setSelected(null))
          ) : screen === "history" ? (
            <History
              rows={snapshot.history}
              source={source}
              send={send}
              edit={setSelected}
            />
          ) : screen === "settings" ? (
            <Settings
              key={JSON.stringify(snapshot.settings)}
              settings={snapshot.settings}
              devices={snapshot.devices}
              host={snapshot.host}
              send={send}
            />
          ) : (
            <Setup
              key={snapshot.settings?.hotkey ?? "loading"}
              settings={snapshot.settings}
              devices={snapshot.devices}
              session={session}
              host={snapshot.host}
              send={send}
            />
          )}
          {selected ? null : <p className="window-note">{note}</p>}
        </main>
      )}
      {screen === "overlay" && error ? (
        <p className="overlay-error" role="alert">
          {error}
        </p>
      ) : null}
      {demo && params.get("controls") !== "0" ? (
        <div className="demo-controls" aria-label="Fake stream controls">
          {screens.map((item) => (
            <button
              key={item}
              aria-current={screen === item ? "page" : undefined}
              onClick={() => navigate(item)}
            >
              {labels[item]}
            </button>
          ))}
          <button onClick={() => demo.next()}>Next event</button>
          <button
            onClick={() => {
              demo.reset();
              setSelected(null);
              setDrafts({});
            }}
          >
            Reset
          </button>
          <select
            aria-label="Demo phase"
            value={session?.phase ?? "recording"}
            onChange={(event) =>
              demo.phase(event.target.value as (typeof phases)[number])
            }
          >
            {phases.map((phase) => (
              <option key={phase}>{phase}</option>
            ))}
          </select>
        </div>
      ) : null}
    </>
  );
}
