import {
  Icon,
  LevelMeter,
  sessionLabel,
  Status,
  Target,
  targetName,
  TranscriptTail,
} from "./components";
import { macHost, shortcutLabel } from "./shortcuts";
import type { CoreSnapshot, CoreSource, Send, SessionView } from "./source";

const overlayNote = (session: SessionView, { host, settings }: CoreSnapshot) => {
  const notification = host?.notifications[session.id];

  if (notification !== undefined && notification !== "" && notification !== sessionLabel(session)) {
    return notification;
  }

  if (session.phase === "saved") {
    return settings
      ? `Press ${shortcutLabel(settings.insert_last_hotkey, macHost(host))} to insert`
      : "Use Insert last to insert";
  }

  return session.phase === "failed" ? "Kept in history" : "";
};

export const Overlay = ({
  session,
  source,
  send,
}: {
  session: SessionView | undefined;
  source: CoreSource;
  send: Send;
}) => {
  const snapshot = source.getSnapshot();
  const appId = session?.target?.app_id;
  const autoSend = appId !== undefined && snapshot.settings?.apps[appId]?.send === true;

  return (
    <main className="stage overlay-stage" aria-label="Dictation overlay">
      <section
        className="panel overlay-panel"
        aria-label="Live dictation"
        data-arming={session?.phase === "recording" && session.level === null}
      >
        {session ? (
          <>
            <div className="row">
              <Status session={session}>
                {autoSend ? <span className="quiet">auto-send</span> : null}
              </Status>
              <Target name={targetName(session.target) ?? "Clipboard"} />
              {session.phase === "recording" ? (
                <button
                  type="button"
                  className="close"
                  aria-label="Cancel dictation"
                  disabled={!source.connected}
                  onMouseDown={(event) => {
                    event.preventDefault();
                  }}
                  onClick={() => {
                    void send({ type: "cancel", id: session.id });
                  }}
                >
                  <Icon name="close" />
                </button>
              ) : null}
            </div>
            <TranscriptTail session={session} />
            <div className="foot">
              <LevelMeter level={session.level} />
              <output className="note">{overlayNote(session, snapshot)}</output>
            </div>
          </>
        ) : (
          <p className="quiet">
            {source.connected ? (snapshot.host?.unavailable ?? "Ready") : "Waiting for connection"}
          </p>
        )}
      </section>
    </main>
  );
};
