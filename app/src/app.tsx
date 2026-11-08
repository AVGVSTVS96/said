import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { Window } from "@tauri-apps/api/window";
import { useState, useSyncExternalStore } from "react";

import type { Draft } from "./editor";
import type { createFakeSource } from "./fake";
import { History } from "./history";
import { Interactive } from "./interactive";
import { useImperative, useSubscription } from "./outside";
import { Overlay } from "./overlay";
import { product } from "./product";
import { Settings } from "./settings";
import { Setup } from "./setup";
import { disconnectedSource, userText } from "./source";
import type { AppAction, CoreSnapshot, CoreSource, Send } from "./source";

const screens = ["interactive", "history", "settings", "setup", "overlay"] as const;

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

const nextFrame = async () => {
  const frame = Promise.withResolvers<number>();
  requestAnimationFrame(frame.resolve);
  await frame.promise;
};

const reconfigureTiledWaylandWindow = async (window: Window) => {
  await nextFrame();
  await nextFrame();
  await window.unmaximize();
};

const fitWindow = async (size: LogicalSize) => {
  const window = getCurrentWindow();
  await Promise.all([window.setMinSize(size), window.setMaxSize(size)]);
  await window.setSize(size);
  await reconfigureTiledWaylandWindow(window);
};

const fitWindowToContent = (element: HTMLElement | null) => {
  const observer = new ResizeObserver(() => {
    if (element && element.offsetHeight > 0) {
      fitWindow(new LogicalSize(element.offsetWidth, element.offsetHeight)).catch(() => null);
    }
  });

  if (element) {
    observer.observe(element);
  }

  return () => {
    observer.disconnect();
  };
};

const FloatingPanel = ({ children }: { children: React.ReactNode }) => (
  <main className="stage panel-stage" ref={isTauri() ? fitWindowToContent : undefined}>
    {children}
  </main>
);

const unlisten = async (listening: Promise<() => void>) => {
  const stop = await listening;
  stop();
};

const without = (drafts: Record<string, Draft>, id: string) =>
  Object.fromEntries(Object.entries(drafts).filter(([key]) => key !== id));

const accepted = (snapshot: CoreSnapshot, id: string, text: string) =>
  userText(snapshot.sessions.find((view) => view.id === id)) === text ||
  snapshot.history.some((row) => row.id === id && row.text === text);

const latestSession = (snapshot: CoreSnapshot, screen: Screen) =>
  screen === "interactive"
    ? snapshot.sessions.findLast((view) => view.mode === "interactive")
    : snapshot.sessions.at(-1);

const Note = ({ failure, feedback }: { failure: string; feedback: string }) => {
  if (failure !== "") {
    return (
      <span className="action-error" role="alert">
        {failure}
      </span>
    );
  }

  return feedback === "" ? null : <output className="action-feedback">{feedback}</output>;
};

const DemoControls = ({
  demo,
  screen,
  phase,
  navigate,
  reset,
}: {
  demo: FakeSource;
  screen: Screen;
  phase: string;
  navigate: (screen: Screen) => void;
  reset: () => void;
}) => (
  <div className="demo-controls" aria-label="Fake stream controls">
    {screens.map((item) => (
      <button
        type="button"
        key={item}
        aria-current={screen === item ? "page" : undefined}
        onClick={() => {
          navigate(item);
        }}
      >
        {labels[item]}
      </button>
    ))}
    <button
      type="button"
      onClick={() => {
        demo.next();
      }}
    >
      Next event
    </button>
    <button
      type="button"
      onClick={() => {
        demo.reset();
        reset();
      }}
    >
      Reset
    </button>
    <select
      aria-label="Demo phase"
      value={phase}
      onChange={(event) => {
        const picked = phases.find((item) => item === event.target.value);

        if (picked !== undefined) {
          demo.phase(picked);
        }
      }}
    >
      {phases.map((item) => (
        <option key={item}>{item}</option>
      ))}
    </select>
  </div>
);

export const App = ({
  source = disconnectedSource,
  demo,
}: {
  source?: CoreSource;
  demo?: FakeSource;
}) => {
  const params = new URLSearchParams(window.location.search);

  const [screen, setScreen] = useState<Screen>(
    screens.find((item) => item === params.get("screen")) ?? "setup",
  );

  const [selected, setSelected] = useState<string | null>(null);
  const [failure, setFailure] = useState("");
  const [feedback, setFeedback] = useState("");
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const snapshot = useSyncExternalStore(source.subscribe, source.getSnapshot);
  const session = latestSession(snapshot, screen);
  const editing = selected ?? session?.id;
  const utterance = snapshot.history.find((row) => row.id === editing);

  useImperative(() => {
    document.title = `${labels[screen]} · ${product.name}`;
    document.documentElement.dataset.screen = screen;
    document.documentElement.dataset.demo = demo ? "true" : "false";
  }, [screen, demo]);

  useSubscription(() => {
    const stop =
      isTauri() && screen === "setup"
        ? listen<string>("notice", (event) => {
            setFailure(event.payload);
          })
        : null;

    return () => {
      if (stop) {
        void unlisten(stop);
      }
    };
  }, [screen]);

  const settle = (action: AppAction) => {
    if (action.type === "edit") {
      if (accepted(source.getSnapshot(), action.id, action.text)) {
        setDrafts((current) =>
          current[action.id]?.text === action.text ? without(current, action.id) : current,
        );
      }
    } else if (demo) {
      setFeedback(
        `Demo: ${action.type.replaceAll("_", " ")} action recorded. No desktop action was performed.`,
      );
    } else if (action.type === "copy") {
      setFeedback("Copied");
    }
  };

  const send: Send = async (action) => {
    setFailure("");
    setFeedback("");

    try {
      await source.dispatch(action);
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "Core could not complete the action.");

      return false;
    }

    settle(action);

    return true;
  };

  const navigate = (next: Screen) => {
    setScreen(next);
    setSelected(null);
    setFeedback("");
    setFailure("");
    const url = new URL(window.location.href);
    url.searchParams.set("screen", next);
    window.history.replaceState(null, "", url);
  };

  const note = <Note failure={failure} feedback={feedback} />;

  const editor = (onClose: (() => void) | undefined) => (
    <Interactive
      key={editing ?? "empty"}
      session={selected === null ? session : undefined}
      utterance={utterance}
      source={source}
      send={send}
      draft={drafts[editing ?? ""] ?? null}
      onEdit={(draft) => {
        if (editing !== undefined) {
          setDrafts((current) => ({ ...current, [editing]: draft }));
        }
      }}
      onClose={onClose}
      note={failure === "" && feedback === "" ? null : note}
    />
  );

  const windowScreen = () => {
    if (screen === "history" && selected !== null) {
      return editor(() => {
        setSelected(null);
      });
    }

    if (screen === "history") {
      return <History rows={snapshot.history} source={source} send={send} edit={setSelected} />;
    }

    if (screen === "settings") {
      return (
        <Settings
          key={JSON.stringify(snapshot.settings)}
          settings={snapshot.settings}
          devices={snapshot.devices}
          history={snapshot.history}
          host={snapshot.host}
          send={send}
        />
      );
    }

    return (
      <Setup
        key={snapshot.settings ? "ready" : "loading"}
        settings={snapshot.settings}
        devices={snapshot.devices}
        host={snapshot.host}
        source={source}
        send={send}
      />
    );
  };

  const content = () => {
    if (screen === "overlay") {
      return <Overlay session={session} source={source} send={send} />;
    }

    if (screen === "interactive") {
      return (
        <FloatingPanel>
          {editor(
            isTauri()
              ? () => {
                  void getCurrentWindow().close();
                }
              : undefined,
          )}
        </FloatingPanel>
      );
    }

    return (
      <main className={`window${screen === "setup" ? " setup-window" : ""}`}>
        {windowScreen()}
        {selected === null ? <p className="window-note">{note}</p> : null}
      </main>
    );
  };

  if (!source.connected) {
    return (
      <main className="window" role="alert">
        Host disconnected: {snapshot.connectionError ?? "The core connection is not available."}{" "}
        Restart {product.name} to reconnect.
      </main>
    );
  }

  return (
    <>
      {content()}
      {screen === "overlay" && failure !== "" ? (
        <p className="overlay-error" role="alert">
          {failure}
        </p>
      ) : null}
      {demo && params.get("controls") !== "0" ? (
        <DemoControls
          demo={demo}
          screen={screen}
          phase={session?.phase ?? "recording"}
          navigate={navigate}
          reset={() => {
            setSelected(null);
            setDrafts({});
          }}
        />
      ) : null}
    </>
  );
};
