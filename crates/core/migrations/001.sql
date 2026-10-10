CREATE TABLE utterances (
    id TEXT PRIMARY KEY NOT NULL,
    audio_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    engine TEXT NOT NULL,
    raw_text TEXT,
    text TEXT,
    status TEXT CHECK (status IN ('done', 'empty', 'canceled', 'discarded', 'recovered', 'failed')),
    pasted_to TEXT,
    pasted_at INTEGER,
    CHECK ((raw_text IS NULL) = (text IS NULL)),
    CHECK ((pasted_to IS NULL) = (pasted_at IS NULL))
) STRICT;

CREATE INDEX utterances_started_at ON utterances(started_at DESC);
CREATE INDEX utterances_audio_id ON utterances(audio_id);

CREATE VIRTUAL TABLE utterances_fts USING fts5(
    raw_text, text, content='utterances', content_rowid='rowid'
);

CREATE TRIGGER utterances_insert AFTER INSERT ON utterances BEGIN
    INSERT INTO utterances_fts(rowid, raw_text, text) VALUES (new.rowid, new.raw_text, new.text);
END;

CREATE TRIGGER utterances_delete AFTER DELETE ON utterances BEGIN
    INSERT INTO utterances_fts(utterances_fts, rowid, raw_text, text)
        VALUES ('delete', old.rowid, old.raw_text, old.text);
END;

CREATE TRIGGER utterances_update AFTER UPDATE OF raw_text, text ON utterances BEGIN
    INSERT INTO utterances_fts(utterances_fts, rowid, raw_text, text)
        VALUES ('delete', old.rowid, old.raw_text, old.text);
    INSERT INTO utterances_fts(rowid, raw_text, text) VALUES (new.rowid, new.raw_text, new.text);
END;

CREATE TRIGGER utterances_original BEFORE UPDATE ON utterances
WHEN new.id IS NOT old.id OR new.audio_id IS NOT old.audio_id
    OR (old.raw_text IS NOT NULL AND new.raw_text IS NOT old.raw_text)
BEGIN
    SELECT RAISE(ABORT, 'original text and audio identity are immutable');
END;

PRAGMA user_version = 1;
