import { useRef, useState } from "react";
import type { SessionView, UiAction, Utterance } from "./source";
import { userText } from "./source";
import type { CoreSource } from "./source";
import { duration, Icon, LevelMeter, Status, Unavailable } from "./components";
import { editDraft, editorText, type Draft } from "./editor";

export function Interactive({
  session,
  utterance,
  source,
  send,
  draft,
  onEdit,
}: {
  session?: SessionView;
  utterance?: Utterance;
  source: CoreSource;
  send: CoreSource["dispatch"];
  draft: Draft | null;
  onEdit: (draft: Draft) => void;
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
  const liveTail = !draft && partial && text.endsWith(partial) ? partial : "";
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

  async function act(type: "insert" | "copy" | "discard" | "cancel") {
    if (!id || pending) return;
    setPending(true);
    try {
      if (draft?.edited) await send({ type: "edit", id, text });
      await send({ type, id } satisfies UiAction);
    } catch {
    } finally {
      setPending(false);
    }
  }

  return (
    <section className="interactive-screen">
      <header className="page-heading">
        <div>
          <p className="eyebrow">Interactive dictation</p>
          <h1>Your words, with room to edit.</h1>
        </div>
        {session ? (
          <Status session={session} />
        ) : (
          <span className="quiet">
            {utterance ? "Saved recording" : "Ready when you are"}
          </span>
        )}
      </header>
      <div className="editor-card">
        <div className="editor-toolbar">
          <span className="target">
            <Icon name="arrow" />
            {session?.target?.name ??
              utterance?.pasted_to?.name ??
              "Recorded target"}
          </span>
          {session ? (
            <LevelMeter level={session.level} />
          ) : (
            <span className="quiet">
              {utterance
                ? duration(utterance.duration_ms)
                : "Local recognition"}
            </span>
          )}
        </div>
        <div className="editor-surface">
          <div className="editor-mirror" aria-hidden="true">
            <span>
              {liveTail ? text.slice(0, text.length - liveTail.length) : text}
            </span>
            <span className="provisional">{liveTail}</span>
            {text.endsWith("\n") ? "\n" : ""}
          </div>
          <textarea
            aria-label="Dictation text"
            placeholder="Your words will appear here. You can edit at any time."
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
              if (id)
                void send({ type: "edit", id, text: next }).catch(() => {});
            }}
            onScroll={(event) => {
              const mirror = event.currentTarget.previousElementSibling;
              if (mirror) {
                mirror.scrollTop = event.currentTarget.scrollTop;
                mirror.scrollLeft = event.currentTarget.scrollLeft;
              }
            }}
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
          />
        </div>
        <div className="editor-caption">
          <span>
            {protectedDraft?.edited
              ? "Your edits stay yours. New words keep streaming."
              : "Underlined words are provisional."}
          </span>
          <button
            className="text-button"
            aria-expanded={original}
            onClick={() => setOriginal(!original)}
          >
            Original {original ? "−" : "+"}
          </button>
        </div>
        {original ? (
          <div className="original-panel">
            <p className="eyebrow">
              Original recognition · never changed by edits
            </p>
            <p className="verbatim">
              {committed}
              <span className="provisional">{partial}</span>
            </p>
          </div>
        ) : null}
        <div className="editor-actions">
          <div className="button-group">
            <button
              disabled={!id || !source.connected || pending}
              onClick={() => void act(recording ? "cancel" : "discard")}
            >
              {recording ? "Cancel recording" : "Discard"}
            </button>
            <button
              disabled={
                !id || !source.connected || !text || recording || pending
              }
              onClick={() => void act("copy")}
            >
              <Icon name="copy" />
              Copy
            </button>
          </div>
          <button
            className="primary"
            disabled={!insertable || pending}
            onClick={() => void act("insert")}
          >
            Insert
            <Icon name="arrow" />
          </button>
        </div>
      </div>
      <div className="below-editor">
        <p>
          Enter adds a line. <kbd>Ctrl / ⌘</kbd> + <kbd>Enter</kbd> inserts into
          the recorded app.
        </p>
        <button disabled>
          <Icon name="play" />
          Play recording
        </button>
      </div>
      <Unavailable>
        Audio playback is unavailable in this build. Insert pastes your text
        without submitting it.
      </Unavailable>
    </section>
  );
}
