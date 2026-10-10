import { useDeferredValue, useState } from "react";
import type { CoreSource, Utterance } from "./source";
import { Playback } from "./Playback";
import { duration, Icon } from "./components";

export function History({
  rows,
  source,
  send,
  edit,
}: {
  rows: readonly Utterance[];
  source: CoreSource;
  send: CoreSource["dispatch"];
  edit: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const search = useDeferredValue(query).toLocaleLowerCase();
  const matches = rows
    .filter(
      (row) =>
        source.getSnapshot().host || !search ||
        (row.text ?? "").toLocaleLowerCase().includes(search) ||
        (row.raw_text ?? "").toLocaleLowerCase().includes(search),
    )
    .sort((a, b) => b.started_at - a.started_at);

  return (
    <section>
      <header className="page-heading">
        <div>
          <p className="eyebrow">History</p>
          <h1>Every recording stays here.</h1>
          <p className="subtitle">Original words and edits, saved together.</p>
        </div>
        <span className="count">{rows.length} recordings</span>
      </header>
      <label className="search-field">
        <Icon name="search" />
        <input
          type="search"
          aria-label="Search history"
          placeholder="Search your words"
          value={query}
          onChange={(event) => {
            const query = event.target.value;
            setQuery(query);
            if (source.getSnapshot().host) void send({ type: "history_search", query }).catch(() => {});
          }}
        />
      </label>
      <p className="results" role="status">
        {query
          ? `${matches.length} ${matches.length === 1 ? "result" : "results"}`
          : "Newest first · kept until you delete them"}
      </p>
      <div className="history-list">
        {matches.map((row) => (
          <article key={row.id} className="history-entry">
            <div className="history-meta">
              <time dateTime={new Date(row.started_at).toISOString()}>
                {new Intl.DateTimeFormat(undefined, {
                  month: "short",
                  day: "numeric",
                  hour: "numeric",
                  minute: "2-digit",
                }).format(row.started_at)}
              </time>
              <span>{duration(row.duration_ms)}</span>
              <span className="badge">{row.status ?? "Recognizing"}</span>
              {row.text !== row.raw_text ? (
                <span className="badge">Edited</span>
              ) : null}
            </div>
            <p className="history-text">
              {row.text === null
                ? "Recognition is still in progress."
                : row.text === ""
                  ? "No speech recognized. Audio kept."
                  : row.text}
            </p>
            <div className="history-actions">
              <Playback id={row.id} />
              <button
                disabled={
                  !source.connected ||
                  row.text === null ||
                  row.text.length === 0
                }
                onClick={() =>
                  void send({ type: "copy", id: row.id }).catch(() => {})
                }
              >
                <Icon name="copy" />
                Copy
              </button>
              <button
                disabled={!source.connected || row.text === null}
                onClick={() => edit(row.id)}
              >
                <Icon name="edit" />
                Edit
              </button>
              <button
                disabled={!source.connected || !row.text}
                onClick={() =>
                  void send({ type: "insert", id: row.id }).catch(() => {})
                }
              >
                <Icon name="arrow" />
                Insert again
              </button>
              <button
                className="text-button history-details"
                aria-expanded={expanded === row.id}
                onClick={() => setExpanded(expanded === row.id ? null : row.id)}
              >
                Details {expanded === row.id ? "−" : "+"}
              </button>
            </div>
            {expanded === row.id ? (
              <div className="history-original">
                <p className="eyebrow">Original recognition</p>
                <p className="verbatim">
                  {row.raw_text ?? "Not available yet"}
                </p>
                <p className="quiet">
                  {row.engine}
                  {row.pasted_to
                    ? ` · Inserted into ${row.pasted_to.name}`
                    : " · Saved without delivery"}
                </p>
                <div className="button-group">
                  <button disabled={!source.connected || row.raw_text === null} onClick={() => void send({ type: "history_retry", id: row.id }).catch(() => {})}>Retry recognition</button>
                  <button className="danger" disabled={!source.connected} onClick={() => void send({ type: "history_delete", id: row.id }).catch(() => {})}>
                    Delete recording
                  </button>
                </div>
              </div>
            ) : null}
          </article>
        ))}
        {matches.length === 0 ? (
          <div className="empty-state">
            <Icon name="history" />
            <h2>
              {query ? "No matching words" : "Your first recording goes here"}
            </h2>
            <p>
              {query
                ? "Try a different word or clear your search."
                : "Audio and text will be saved before anything is inserted."}
            </p>
            {query ? (
              <button onClick={() => setQuery("")}>Clear search</button>
            ) : null}
          </div>
        ) : null}
      </div>

    </section>
  );
}
