import { useRef, useState } from "react";
import type { CoreSource, SessionView, UiAction, Utterance } from "./source";
import { userText } from "./source";
import { Playback } from "./Playback";
import {
  duration,
  Icon,
  LevelMeter,
  modifier,
  Status,
  Target,
  Transcript,
} from "./components";
import { editDraft, editorText, type Draft } from "./editor";

function editedRange(before: string, after: string) {
  let start = 0;
  while (
    start < before.length &&
    start < after.length &&
    before[start] === after[start]
  )
    start++;
  let end = 0;
  while (
    end < before.length - start &&
    end < after.length - start &&
    before[before.length - end - 1] === after[after.length - end - 1]
  )
    end++;
  return [start, after.length - end] as const;
}

export function Interactive({
  session,
  utterance,
  source,
  send,
  draft,
  onEdit,
  onClose,
  note,
}: {
  session?: SessionView;
  utterance?: Utterance;
  source: CoreSource;
  send: CoreSource["dispatch"];
  draft: Draft | null;
  onEdit: (draft: Draft) => void;
  onClose?: () => void;
  note?: React.ReactNode;
}) {
  const [original, setOriginal] = useState(false);
  const [pending, setPending] = useState(false);
  const composing = useRef(false);
  const id = session?.id ?? utterance?.id;
  const committed = session?.committed ?? utterance?.raw_text ?? "";
  const partial = session?.partial ?? "";
  const savedEdit =
    userText(session) ??
    (utterance?.text !== utterance?.raw_text ? utterance?.text : null);
  const protectedDraft =
    draft ??
    (savedEdit !== null && savedEdit !== undefined
      ? editDraft(savedEdit)
      : null);
  const text = editorText(protectedDraft, committed, partial);
  const recording =
    session?.phase === "recording" ||
    session?.phase === "finishing" ||
    session?.phase === "saving";
  const ready =
    !session || session.phase === "reviewing" || session.phase === "pasted";
  const editable =
    !session ||
    ["recording", "finishing", "saving", "reviewing", "pasted"].includes(
      session.phase,
    );
  const insertable = Boolean(
    id && ready && source.connected && text.length > 0,
  );
  const [from, to] = protectedDraft?.edited
    ? editedRange(committed + partial, text)
    : [0, 0];

  async function act(type: "insert" | "copy" | "discard" | "cancel") {
    if (!id || pending) return;
    setPending(true);
    try {
      if (draft?.edited) await send({ type: "edit", id, text });
      await send({ type, id } satisfies UiAction);
      if (type === "insert" && source.getSnapshot().sessions.some((view) => view.id === id && view.phase === "pasted")) onClose?.();
    } catch {
    } finally {
      setPending(false);
    }
  }

  return (
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
          if (insertable) void act("insert");
        }
      }}
    >
      <div className="row" data-tauri-drag-region>
        {session ? (
          <Status session={session} />
        ) : (
          <Status
            label={
              utterance ? "Saved" : source.connected ? "Ready" : "Not connected"
            }
          />
        )}
        {session?.phase === "recording" ? (
          <LevelMeter level={session.level} />
        ) : null}
        {!session && utterance ? (
          <span className="quiet">{duration(utterance.duration_ms)}</span>
        ) : null}
        <Target
          name={
            session?.target?.name ||
            session?.target?.app_id ||
            utterance?.pasted_to?.name ||
            utterance?.pasted_to?.app_id
          }
        />
        {onClose ? (
          <button className="close" aria-label="Close" onClick={onClose}>
            <Icon name="close" />
          </button>
        ) : null}
      </div>
      <div className="editor">
        <div className="editor-mirror" aria-hidden="true">
          {protectedDraft?.edited ? (
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
        <textarea
          aria-label="Dictation text"
          value={text}
          disabled={!id || !source.connected || !editable}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          autoFocus={Boolean(id)}
          onCompositionStart={() => {
            composing.current = true;
            onEdit(editDraft(text));
            if (id) void send({ type: "edit", id, text }).catch(() => {});
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onChange={(event) => {
            const next = event.target.value;
            onEdit(editDraft(next));
            if (id) void send({ type: "edit", id, text: next }).catch(() => {});
          }}
          onScroll={(event) => {
            const mirror = event.currentTarget.previousElementSibling;
            if (mirror) mirror.scrollTop = event.currentTarget.scrollTop;
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
          className="quiet"
          aria-expanded={original}
          onClick={() => setOriginal(!original)}
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
          disabled={!id || !source.connected || pending}
          onClick={() => void act(recording ? "cancel" : "discard")}
        >
          {recording ? "Cancel recording" : "Discard"}
        </button>
        <button
          disabled={!id || !source.connected || !text || recording || pending}
          onClick={() => void act("copy")}
        >
          Copy
        </button>
        <button
          className="primary"
          disabled={!insertable || pending}
          onClick={() => void act("insert")}
        >
          Insert
          <Icon name="arrow" />
        </button>
      </div>
    </section>
  );
}
