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

export function userText(session?: SessionView) {
  return session?.edited ?? null;
}

export type CoreSnapshot = Readonly<{
  sessions: readonly SessionView[];
  history: readonly Utterance[];
  settings: Settings | null;
  devices: readonly InputDevice[];
  sequence?: number;
  host?: HostState;
  connectionError?: string;
}>;

export interface CoreSource {
  getSnapshot(): CoreSnapshot;
  subscribe(listener: () => void): () => void;
  dispatch(action: AppAction): Promise<void>;
  connected: boolean;
}

export function createCoreSource(send: (action: AppAction) => Promise<void>) {
  let snapshot: CoreSnapshot = {
    sessions: [],
    history: [],
    settings: null,
    devices: [],
  };
  let pending = Promise.resolve();
  let connected = true;
  const listeners = new Set<() => void>();
  return {
    get connected() { return connected; },
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    dispatch(action: AppAction) {
      pending = pending.catch(() => {}).then(() => {
        if (!connected) throw new Error(snapshot.connectionError ?? "The host connection is not available.");
        return send(action);
      });
      return pending;
    },
    receive(next: CoreSnapshot) {
      if (!connected) return;
      if (next.sequence !== undefined && snapshot.sequence !== undefined && next.sequence <= snapshot.sequence) return;
      snapshot = next;
      listeners.forEach((listener) => listener());
    },
    disconnect(error: string) {
      connected = false;
      snapshot = { ...snapshot, sessions: [], settings: null, devices: [], host: undefined, connectionError: error };
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
