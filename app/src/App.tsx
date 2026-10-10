import { useEffect, useState, useSyncExternalStore } from "react";
import { product } from "./product";
import {
  type CoreSource,
  type UiAction,
  disconnectedSource,
  userText,
} from "./source";
import type { createFakeSource } from "./fake";
import { Icon } from "./components";
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

  async function send(action: UiAction) {
    setError("");
    setFeedback("");
    try {
      await source.dispatch(action);
      if (action.type === "edit") {
        const current = source.getSnapshot();
        const accepted =
          userText(
            current.sessions.find((session) => session.id === action.id),
          ) !== null ||
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
      }
      if (action.type !== "edit")
        setFeedback(
          demo
            ? `Demo: ${action.type.replaceAll("_", " ")} action recorded. No desktop action was performed.`
            : "Action sent to core.",
        );
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

  return (
    <>
      {screen === "overlay" ? (
        <Overlay session={session} source={source} send={send} />
      ) : (
        <div className="app-shell">
          <aside className="sidebar">
            <div className="product-name">
              {product.name}
              <span>Dictation</span>
            </div>
            <nav aria-label="Main navigation">
              {(["interactive", "history", "settings", "setup"] as const).map(
                (item) => (
                  <button
                    key={item}
                    aria-current={screen === item ? "page" : undefined}
                    onClick={() => navigate(item)}
                  >
                    <Icon
                      name={
                        item === "interactive" || item === "setup"
                          ? "mic"
                          : item
                      }
                    />
                    {labels[item]}
                  </button>
                ),
              )}
            </nav>
            <div className="sidebar-note">
              <span className="status-dot" />
              {demo
                ? "Synthetic demo"
                : source.connected
                  ? "Connected to core"
                  : "Core not connected"}
            </div>
          </aside>
          <main className="main-content">
            {screen === "interactive" ? (
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
              />
            ) : null}
            {screen === "history" ? (
              <History
                rows={snapshot.history}
                source={source}
                send={send}
                edit={(id) => {
                  setSelected(id);
                  setScreen("interactive");
                }}
              />
            ) : null}
            {screen === "settings" ? (
              <Settings
                key={JSON.stringify(snapshot.settings)}
                settings={snapshot.settings}
                devices={snapshot.devices}
              />
            ) : null}
            {screen === "setup" ? (
              <Setup
                key={snapshot.settings?.hotkey ?? "loading"}
                settings={snapshot.settings}
                devices={snapshot.devices}
                session={session}
              />
            ) : null}
            {feedback ? (
              <p className="action-feedback" role="status">
                {feedback}
              </p>
            ) : null}
            {error ? (
              <p className="action-error" role="alert">
                {error}
                <button className="text-button" onClick={() => setError("")}>
                  Dismiss
                </button>
              </p>
            ) : null}
          </main>
        </div>
      )}
      {screen === "overlay" && error ? (
        <p className="overlay-error" role="alert">
          {error}
        </p>
      ) : null}
      {demo && params.get("controls") !== "0" ? (
        <div className="demo-controls" aria-label="Fake stream controls">
          <span>Synthetic stream</span>
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
              demo.phase(
                event.target.value as NonNullable<typeof session>["phase"],
              )
            }
          >
            {[
              "recording",
              "finishing",
              "saving",
              "saved",
              "reviewing",
              "pasting",
              "pasted",
              "failed",
            ].map((phase) => (
              <option key={phase}>{phase}</option>
            ))}
          </select>
          <button
            onClick={() =>
              navigate(screen === "overlay" ? "interactive" : "overlay")
            }
          >
            {screen === "overlay" ? "Interactive" : "Overlay"}
          </button>
        </div>
      ) : null}
    </>
  );
}
