import { useRef, useState } from "react";

import {
  duration,
  Icon,
  LevelMeter,
  modifier,
  Status,
  Target,
  targetName,
  Transcript,
} from "./components";
import { editDraft, editorText } from "./editor";
import type { Draft } from "./editor";
import { Playback } from "./playback";
import { userText } from "./source";
import type { CoreSource, Phase, Send, SessionView, UiAction, Utterance } from "./source";

const recordingPhases = new Set<Phase>(["recording", "finishing", "saving"]);
const editablePhases = new Set<Phase>(["recording", "finishing", "saving", "reviewing", "pasted"]);

const editedRange = (before: string, after: string) => {
  let start = 0;

  while (start < before.length && start < after.length && before[start] === after[start]) {
    start += 1;
  }

  let end = 0;

  while (
    end < before.length - start &&
    end < after.length - start &&
    before[before.length - end - 1] === after[after.length - end - 1]
  ) {
    end += 1;
  }

  return [start, after.length - end] as const;
};

const savedText = (session: SessionView | undefined, utterance: Utterance | undefined) => {
  const edited = userText(session);

  if (edited !== null || utterance === undefined || utterance.text === utterance.raw_text) {
    return edited;
  }

  return utterance.text;
};

const editorState = (
  session: SessionView | undefined,
  utterance: Utterance | undefined,
  draft: Draft | null,
) => {
  const committed = session?.committed ?? utterance?.raw_text ?? "";
  const partial = session?.partial ?? "";
  const saved = savedText(session, utterance);
  const shown = draft ?? (saved === null ? null : editDraft(saved));

  return {
    id: session?.id ?? utterance?.id,
    committed,
    partial,
    edited: shown?.edited === true,
    text: editorText(shown, committed, partial),
    recording: session !== undefined && recordingPhases.has(session.phase),
    ready: !session || session.phase === "reviewing" || session.phase === "pasted",
    editable: !session || editablePhases.has(session.phase),
  };
};

const idleLabel = (utterance: Utterance | undefined, connected: boolean) => {
  if (utterance) {
    return "Saved";
  }

  return connected ? "Ready" : "Not connected";
};

const Mirror = ({
  text,
  committed,
  partial,
  edited,
}: {
  text: string;
  committed: string;
  partial: string;
  edited: boolean;
}) => {
  const [from, to] = edited ? editedRange(committed + partial, text) : [0, 0];

  return (
    <div className="editor-mirror" aria-hidden="true">
      {edited ? (
        <>
          {text.slice(0, from)}
          <mark className="edited">{text.slice(from, to)}</mark>
          {text.slice(to)}
        </>
      ) : (
        <Transcript committed={committed} partial={partial} />
      )}
      {text.endsWith("\n") ? "\n" : ""}
    </div>
  );
};

const Header = ({
  session,
  utterance,
  connected,
  onClose,
}: {
  session: SessionView | undefined;
  utterance: Utterance | undefined;
  connected: boolean;
  onClose: (() => void) | undefined;
}) => (
  <div className="row" data-tauri-drag-region>
    {session ? <Status session={session} /> : <Status label={idleLabel(utterance, connected)} />}
    {session?.phase === "recording" ? <LevelMeter level={session.level} /> : null}
    {!session && utterance ? (
      <span className="quiet">{duration(utterance.duration_ms)}</span>
    ) : null}
    <Target name={targetName(session?.target, utterance?.pasted_to)} />
    {onClose ? (
      <button type="button" className="close" aria-label="Close" onClick={onClose}>
        <Icon name="close" />
      </button>
    ) : null}
  </div>
);

export const Interactive = ({
  session,
  utterance,
  source,
  send,
  draft,
  onEdit,
  onClose,
  note,
}: {
  session: SessionView | undefined;
  utterance: Utterance | undefined;
  source: CoreSource;
  send: Send;
  draft: Draft | null;
  onEdit: (draft: Draft) => void;
  onClose: (() => void) | undefined;
  note: React.ReactNode;
}) => {
  const [original, setOriginal] = useState(false);
  const [pending, setPending] = useState(false);
  const composing = useRef(false);

  const { id, committed, partial, edited, text, recording, ready, editable } = editorState(
    session,
    utterance,
    draft,
  );

  const connected = id !== undefined && source.connected;
  const insertable = connected && ready && text.length > 0;

  const act = async (type: "insert" | "copy" | "discard" | "cancel") => {
    if (id === undefined || pending) {
      return;
    }

    setPending(true);
    const handedOver = draft?.edited !== true || (await send({ type: "edit", id, text }));
    const done = handedOver && (await send({ type, id } satisfies UiAction));

    if (
      done &&
      type === "insert" &&
      source.getSnapshot().sessions.some((view) => view.id === id && view.phase === "pasted")
    ) {
      onClose?.();
    }

    setPending(false);
  };

  const edit = (next: string) => {
    onEdit(editDraft(next));

    if (id !== undefined) {
      void send({ type: "edit", id, text: next });
    }
  };

  return (
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Ctrl+Enter inserts from anywhere in the panel, the text and its buttons alike
    <section
      className="panel interactive"
      aria-label="Interactive dictation"
      onKeyDown={(event) => {
        if (
          event.key === "Enter" &&
          (event.ctrlKey || event.metaKey) &&
          !event.nativeEvent.isComposing &&
          !composing.current
        ) {
          event.preventDefault();

          if (insertable) {
            void act("insert");
          }
        }
      }}
    >
      <Header
        session={session}
        utterance={utterance}
        connected={source.connected}
        onClose={onClose}
      />
      <div className="editor">
        <Mirror text={text} committed={committed} partial={partial} edited={edited} />
        <textarea
          aria-label="Dictation text"
          value={text}
          disabled={!connected || !editable}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          autoFocus={id !== undefined}
          onCompositionStart={() => {
            composing.current = true;
            edit(text);
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onChange={(event) => {
            edit(event.target.value);
          }}
          onScroll={(event) => {
            const mirror = event.currentTarget.previousElementSibling;

            if (mirror) {
              mirror.scrollTop = event.currentTarget.scrollTop;
            }
          }}
        />
      </div>
      {original ? (
        <div className="original">
          <b>Original recognition</b>
          <span className="record">
            {committed}
            <span className="provisional">{partial}</span>
          </span>
        </div>
      ) : null}
      <div className="foot">
        <button
          type="button"
          className="quiet"
          aria-expanded={original}
          onClick={() => {
            setOriginal(!original);
          }}
        >
          {original ? "Hide original" : "Show original"}
        </button>
        {note ?? (
          <span className="hint">
            <kbd>{modifier}</kbd>
            <kbd>↩</kbd> inserts
          </span>
        )}
        <span className="spacer" />
        {ready ? <Playback id={id} /> : null}
        <button
          type="button"
          disabled={!connected || pending}
          onClick={() => {
            void act(recording ? "cancel" : "discard");
          }}
        >
          {recording ? "Cancel recording" : "Discard"}
        </button>
        <button
          type="button"
          disabled={!connected || text === "" || recording || pending}
          onClick={() => {
            void act("copy");
          }}
        >
          Copy
        </button>
        <button
          type="button"
          className="primary"
          disabled={!insertable || pending}
          onClick={() => {
            void act("insert");
          }}
        >
          Insert
          <Icon name="arrow" />
        </button>
      </div>
    </section>
  );
};
