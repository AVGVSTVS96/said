import type {
  HostAction,
  HostState,
  InputDevice,
  SessionView,
  Settings,
  UiAction,
  Utterance,
} from "./contracts";

export type {
  EngineEvent,
  InputDevice,
  Phase,
  SessionView,
  Settings,
  UiAction,
  Utterance,
} from "./contracts";

export type AppAction = UiAction | HostAction;

export type Send = (action: AppAction) => Promise<boolean>;

const uiActions = new Set<AppAction["type"]>([
  "insert",
  "copy",
  "edit",
  "discard",
  "cancel",
  "insert_last",
]);

export const isUiAction = (action: AppAction): action is UiAction => uiActions.has(action.type);

export const userText = (session?: SessionView) => session?.edited ?? null;

export type CoreSnapshot = Readonly<{
  sessions: readonly SessionView[];
  history: readonly Utterance[];
  settings: Settings | null;
  devices: readonly InputDevice[];
  sequence?: number;
  host?: HostState;
  connectionError?: string;
}>;

type Incoming = Omit<CoreSnapshot, "history"> & {
  history: readonly Utterance[] | null;
};

export interface CoreSource {
  getSnapshot: () => CoreSnapshot;
  subscribe: (listener: () => void) => () => void;
  dispatch: (action: AppAction) => Promise<void>;
  connected: boolean;
}

export const createCoreSource = (send: (action: AppAction) => Promise<void> | void) => {
  let snapshot: CoreSnapshot = {
    sessions: [],
    history: [],
    settings: null,
    devices: [],
  };

  let pending = Promise.resolve();
  let connected = true;
  const listeners = new Set<() => void>();

  const notify = () => {
    for (const listener of listeners) {
      listener();
    }
  };

  return {
    get connected() {
      return connected;
    },
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener);

      return () => {
        listeners.delete(listener);
      };
    },
    dispatch: async (action: AppAction) => {
      const run = async () => {
        if (!connected) {
          throw new Error(snapshot.connectionError ?? "The host connection is not available.");
        }

        await send(action);
      };

      pending = pending.then(run, run);
      await pending;
    },
    receive: (next: Incoming) => {
      if (!connected) {
        return;
      }

      if (
        next.sequence !== undefined &&
        snapshot.sequence !== undefined &&
        next.sequence <= snapshot.sequence
      ) {
        return;
      }

      snapshot = { ...next, history: next.history ?? snapshot.history };
      notify();
    },
    disconnect: (error: string) => {
      connected = false;
      snapshot = {
        sessions: [],
        history: snapshot.history,
        settings: null,
        devices: [],
        connectionError: error,
      };
      notify();
    },
  };
};

export const disconnectedSource: CoreSource = {
  ...createCoreSource(async () => {
    await Promise.reject(new Error("The core connection is not available."));
  }),
  connected: false,
};
