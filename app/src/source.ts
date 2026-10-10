import type {
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

export function userText(session?: SessionView) {
  return session && "edited" in session && typeof session.edited === "string"
    ? session.edited
    : null;
}

export type CoreSnapshot = Readonly<{
  sessions: readonly SessionView[];
  history: readonly Utterance[];
  settings: Settings | null;
  devices: readonly InputDevice[];
}>;

export interface CoreSource {
  getSnapshot(): CoreSnapshot;
  subscribe(listener: () => void): () => void;
  dispatch(action: UiAction): Promise<void>;
  connected: boolean;
}

export function createCoreSource(send: (action: UiAction) => Promise<void>) {
  let snapshot: CoreSnapshot = {
    sessions: [],
    history: [],
    settings: null,
    devices: [],
  };
  let pending = Promise.resolve();
  const listeners = new Set<() => void>();
  return {
    connected: true,
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    dispatch(action: UiAction) {
      pending = pending.catch(() => {}).then(() => send(action));
      return pending;
    },
    receive(next: CoreSnapshot) {
      snapshot = next;
      listeners.forEach((listener) => listener());
    },
  };
}

export const disconnectedSource: CoreSource = {
  ...createCoreSource(async () => {
    throw new Error("The core connection is not available.");
  }),
  connected: false,
};
