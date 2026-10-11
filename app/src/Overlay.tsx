import type { CoreSource, SessionView } from "./source";
import {
  Icon,
  LevelMeter,
  sessionLabel,
  Status,
  Target,
  TranscriptTail,
} from "./components";

export function Overlay({
  session,
  source,
  send,
}: {
  session?: SessionView;
  source: CoreSource;
  send: CoreSource["dispatch"];
}) {
  const snapshot = source.getSnapshot();
  const settings = snapshot.settings;
  const autoSend =
    session?.target && settings?.apps[session.target.app_id]?.send;
  const notification = session && snapshot.host?.notifications[session.id];
  const note = !session
    ? ""
    : notification && notification !== sessionLabel(session)
      ? notification
      : session.phase === "saved"
        ? "Press Ctrl+Alt+V to insert"
        : session.phase === "failed"
          ? "Kept in history"
          : "";
  return (
    <main className="stage overlay-stage" aria-label="Dictation overlay">
      <section className="panel overlay-panel" aria-label="Live dictation">
        {session ? (
          <>
            <div className="row">
              <Status session={session}>
                {autoSend ? <span className="quiet">auto-send</span> : null}
              </Status>
              <Target
                name={session.target?.name || session.target?.app_id || "Clipboard"}
              />
              {session.phase === "recording" ? (
                <button
                  className="close"
                  aria-label="Cancel dictation"
                  disabled={!source.connected}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() =>
                    void send({ type: "cancel", id: session.id }).catch(
                      () => {},
                    )
                  }
                >
                  <Icon name="close" />
                </button>
              ) : null}
            </div>
            <TranscriptTail session={session} />
            <div className="foot">
              <LevelMeter level={session.level} />
              <span className="note" role="status">
                {note}
              </span>
            </div>
          </>
        ) : (
          <p className="quiet">
            {source.connected
              ? (snapshot.host?.unavailable ?? "Ready")
              : "Waiting for connection"}
          </p>
        )}
      </section>
    </main>
  );
}
