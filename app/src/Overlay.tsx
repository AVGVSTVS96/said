import type { CoreSource } from "./source";
import type { SessionView } from "./source";
import { Icon, LevelMeter, Status, TranscriptTail } from "./components";

export function Overlay({
  session,
  source,
  send,
}: {
  session?: SessionView;
  source: CoreSource;
  send: CoreSource["dispatch"];
}) {
  return (
    <main className="overlay-stage" aria-label="Dictation overlay">
      <section className="overlay-panel" aria-label="Live dictation">
        {session ? (
          <>
            <div className="overlay-status">
              <Status session={session} />
              <span className="target">
                {session.target?.name ?? "Clipboard"}
              </span>
              {session.phase === "recording" ? (
                <button
                  className="icon-button"
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
            <div className="overlay-footer">
              <LevelMeter level={session.level} />
              <span>
                {session.phase === "saved"
                  ? "Press Ctrl+Alt+V to insert"
                  : session.phase === "failed"
                    ? "Open history to recover your recording"
                    : "Provisional words are underlined"}
              </span>
            </div>
          </>
        ) : (
          <p className="overlay-empty">
            {source.connected
              ? "Ready for dictation"
              : "Waiting for core connection"}
          </p>
        )}
      </section>
    </main>
  );
}
