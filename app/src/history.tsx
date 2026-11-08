import { useDeferredValue, useState } from "react";

import { duration, Icon } from "./components";
import { Playback } from "./playback";
import type { CoreSource, Send, Utterance } from "./source";

const when = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

const EntryText = ({ text }: { text: string | null }) => {
  if (text === null) {
    return <span className="quiet">Recognizing</span>;
  }

  return text === "" ? <span className="quiet">No speech recognized</span> : text;
};

export const History = ({
  rows,
  source,
  send,
  edit,
}: {
  rows: readonly Utterance[];
  source: CoreSource;
  send: Send;
  edit: (id: string) => void;
}) => {
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const search = useDeferredValue(query).toLocaleLowerCase();
  const hosted = Boolean(source.getSnapshot().host);

  const matches = rows
    .filter(
      (row) =>
        hosted ||
        !search ||
        (row.text ?? "").toLocaleLowerCase().includes(search) ||
        (row.raw_text ?? "").toLocaleLowerCase().includes(search),
    )
    .toSorted((a, b) => b.started_at - a.started_at);

  return (
    <section className="history">
      <label className="search">
        <Icon name="search" />
        <input
          type="search"
          aria-label="Search history"
          placeholder="Search"
          value={query}
          onChange={(event) => {
            const { value } = event.target;
            setQuery(value);

            if (hosted) {
              void send({ type: "history_search", query: value });
            }
          }}
        />
      </label>
      {query === "" ? null : (
        <output className="results">
          {matches.length} {matches.length === 1 ? "result" : "results"}
        </output>
      )}
      {matches.map((row) => {
        const hasText = row.text !== null && row.text !== "";

        return (
          <article key={row.id} className="entry">
            <div className="meta">
              <time dateTime={new Date(row.started_at).toISOString()}>
                {when.format(row.started_at)}
              </time>
              <span>{duration(row.duration_ms)}</span>
              {row.pasted_to ? (
                <span className="target">
                  <Icon name="arrow" />
                  {row.pasted_to.name}
                </span>
              ) : null}
              {row.status === "done" ? null : (
                <span className="badge">{row.status ?? "Recognizing"}</span>
              )}
              {row.text !== null && row.text !== row.raw_text ? (
                <span className="badge">Edited</span>
              ) : null}
            </div>
            <p className="entry-text">
              <EntryText text={row.text} />
            </p>
            <div className="actions">
              <Playback id={row.id} />
              <button
                type="button"
                className="quiet"
                disabled={!source.connected || !hasText}
                onClick={() => {
                  void send({ type: "copy", id: row.id });
                }}
              >
                <Icon name="copy" />
                Copy
              </button>
              <button
                type="button"
                className="quiet"
                disabled={!source.connected || row.text === null}
                onClick={() => {
                  edit(row.id);
                }}
              >
                <Icon name="edit" />
                Edit
              </button>
              <button
                type="button"
                className="quiet"
                disabled={!source.connected || !hasText}
                onClick={() => {
                  void send({ type: "insert", id: row.id });
                }}
              >
                <Icon name="arrow" />
                Insert again
              </button>
              <span className="spacer" />
              <button
                type="button"
                className="quiet"
                aria-expanded={expanded === row.id}
                onClick={() => {
                  setExpanded(expanded === row.id ? null : row.id);
                }}
              >
                {expanded === row.id ? "Hide details" : "Details"}
              </button>
            </div>
            {expanded === row.id ? (
              <div className="original details">
                <b>Original recognition</b>
                <span className="record">{row.raw_text ?? "Not available yet"}</span>
                <p className="quiet">
                  {row.engine}
                  {row.pasted_to ? ` · inserted into ${row.pasted_to.name}` : " · not inserted"}
                </p>
                <div className="actions">
                  <button
                    type="button"
                    className="quiet"
                    disabled={!source.connected || row.raw_text === null}
                    onClick={() => {
                      void send({ type: "history_retry", id: row.id });
                    }}
                  >
                    <Icon name="retry" />
                    Retry recognition
                  </button>
                  <button
                    type="button"
                    className="quiet danger"
                    disabled={!source.connected}
                    onClick={() => {
                      void send({ type: "history_delete", id: row.id });
                    }}
                  >
                    <Icon name="trash" />
                    Delete
                  </button>
                </div>
              </div>
            ) : null}
          </article>
        );
      })}
      {matches.length === 0 ? (
        <p className="empty quiet">{query === "" ? "Nothing recorded yet." : "Nothing matches."}</p>
      ) : null}
    </section>
  );
};
