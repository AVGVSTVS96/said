import {
  createCoreSource,
  type EngineEvent,
  type SessionView,
  type Settings,
  type UiAction,
  type Utterance,
} from "./source";

const id = "00000000-0000-4000-8000-000000000001";
const target = {
  app_id: "example.editor",
  window_id: "1",
  name: "Text editor",
};
const settings: Settings = {
  hotkey: "super+z",
  interactive_hotkey: "super+x",
  insert_last_hotkey: "super+a",
  engine: "parakeet-v3-fp32",
  microphone: null,
  apps: {
    "example.terminal": { paste: "ctrl+shift+v", send: false },
    "example.editor": { paste: "ctrl+v", send: false },
  },
};
const trace: readonly EngineEvent[] = [
  { type: "partial", text: "the the old value" },
  { type: "partial", text: "the the old value, actually" },
  { type: "committed", text: "the the old value, actually no, " },
  { type: "partial", text: "use cache_key" },
  { type: "committed", text: "use cache_key, no no don't rename it." },
  { type: "final" },
];

function fixtures(): Utterance[] {
  const examples = [
    {
      text: "no no, keep the retry flag. actually, use --retry=2",
      status: "done",
    },
    {
      text: "um, the cache_key should stay exactly as it is",
      status: "recovered",
    },
    { text: "first line\n  second line with spaces\n", status: "done" },
    { text: "", status: "empty" },
  ] as const;
  return examples.map((entry, index) => ({
    id: `00000000-0000-4000-8000-${String(index + 2).padStart(12, "0")}`,
    audio_id: `00000000-0000-4000-8000-${String(index + 2).padStart(12, "0")}`,
    started_at: Date.UTC(2026, 0, 1, 14, 24 - index * 12),
    duration_ms: (index + 1) * 4200,
    engine: settings.engine,
    raw_text: entry.text,
    text: entry.text,
    status: entry.status,
    pasted_to: index === 0 ? target : null,
    pasted_at: index === 0 ? Date.UTC(2026, 0, 1, 14, 24, 5) : null,
  }));
}

export function createFakeSource() {
  let session: SessionView;
  let history: Utterance[];
  let step = 0;
  let editorTail = 0;
  let ownedTail = false;
  const actions: UiAction[] = [];
  const source = createCoreSource(async (action) => {
    if (!["insert", "copy", "edit", "discard", "cancel", "insert_last"].includes(action.type)) throw new Error("This operation needs a native host.");
    actions.push(action as UiAction);
    const actionId = "id" in action ? action.id : history[0]?.id;
    if (!actionId) return;
    const row = history.find((row) => row.id === actionId);
    if (action.type === "edit") {
      if (action.id === session.id) {
        const previous = session.edited ?? session.committed + session.partial;
        if (session.edited === null) editorTail = session.committed.length;
        let prefix = 0;
        while (
          prefix < previous.length &&
          previous[prefix] === action.text[prefix]
        )
          prefix++;
        let suffix = 0;
        while (
          suffix < Math.min(previous.length, action.text.length) - prefix &&
          previous[previous.length - suffix - 1] ===
            action.text[action.text.length - suffix - 1]
        )
          suffix++;
        if (
          previous.length - suffix > editorTail ||
          (prefix >= editorTail && session.partial !== "")
        )
          ownedTail = true;
        else editorTail += action.text.length - previous.length;
        session = { ...session, edited: action.text };
      }
      if (row)
        history = history.map((row) =>
          row.id === action.id ? { ...row, text: action.text } : row,
        );
    }
    if (action.type === "discard" || action.type === "cancel") {
      if (session.id === actionId)
        session = { ...session, phase: "saved", partial: "", level: null };
      history = history.map((row) =>
        row.id === actionId
          ? {
              ...row,
              status: action.type === "cancel" ? "canceled" : "discarded",
            }
          : row,
      );
    }
    publish();
  });

  function publish() {
    source.receive({
      sessions: [session],
      history,
      settings,
      devices: [
        { id: "synthetic-input", name: "Example microphone", is_default: true },
        {
          id: "synthetic-usb",
          name: "Example USB microphone",
          is_default: false,
        },
      ],
    });
  }

  function reset() {
    step = 0;
    editorTail = 0;
    ownedTail = false;
    actions.length = 0;
    history = fixtures();
    session = {
      id,
      mode: "interactive",
      target,
      phase: "recording",
      committed: "um, no, keep ",
      partial: "the the old value",
      level: 0.42,
      edited: null,
    };
    publish();
  }

  function next() {
    const event = trace[step++];
    if (!event || session.phase !== "recording") return;
    if (
      (event.type === "partial" || event.type === "committed") &&
      session.edited !== null
    ) {
      session = {
        ...session,
        edited: ownedTail
          ? session.edited
          : session.edited.slice(0, editorTail) + event.text,
      };
      if (event.type === "committed") {
        editorTail = session.edited!.length;
        ownedTail = false;
      }
    }
    if (event.type === "partial")
      session = {
        ...session,
        partial: event.text,
        level: 0.28 + (step % 3) * 0.19,
      };
    if (event.type === "committed")
      session = {
        ...session,
        committed: session.committed + event.text,
        partial: "",
        level: 0.55,
      };
    if (event.type === "final") {
      session = { ...session, phase: "reviewing", level: null };
      history = [
        {
          id,
          audio_id: id,
          started_at: Date.UTC(2026, 0, 1, 14, 30),
          duration_ms: 9800,
          engine: settings.engine,
          raw_text: session.committed,
          text: session.edited ?? session.committed,
          status: "done",
          pasted_to: null,
          pasted_at: null,
        },
        ...history,
      ];
    }
    publish();
  }

  function phase(value: SessionView["phase"]) {
    session = {
      ...session,
      phase: value,
      level: value === "recording" ? 0.42 : null,
    };
    publish();
  }

  reset();
  return {
    ...source,
    next,
    reset,
    phase,
    actions,
    get step() {
      return step;
    },
  };
}
