use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Result, SCHEMA, SCHEMA_VERSION, Target, Utterance, UtteranceId, UtteranceStatus};

pub trait History {
    fn insert(&mut self, utterance: &Utterance) -> Result<()>;
    fn finish(&mut self, utterance: &Utterance) -> Result<()>;
    fn get(&self, id: UtteranceId) -> Result<Option<Utterance>>;
    fn search(&self, query: &str) -> Result<Vec<Utterance>>;
    fn recover(&self) -> Result<Vec<Utterance>>;
    fn edit(&mut self, id: UtteranceId, text: &str) -> Result<()>;
    fn record_delivery(&mut self, id: UtteranceId, target: &Target, at: i64) -> Result<()>;
    fn delete(&mut self, id: UtteranceId) -> Result<Option<UtteranceId>>;
}

pub struct SqliteHistory {
    db: Connection,
    audio: PathBuf,
}

impl SqliteHistory {
    pub fn open(directory: &Path) -> Result<Self> {
        crate::storage::private_directory(directory)?;
        let audio = directory.join("audio");
        crate::storage::private_directory(&audio)?;
        let path = directory.join("history.sqlite");
        crate::storage::private_file(&path)?;
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;",
        )?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                let tx = db.transaction()?;
                tx.execute_batch(SCHEMA)?;
                tx.commit()?;
            }
            SCHEMA_VERSION => {}
            _ => return Err(format!("unsupported history schema {version}").into()),
        }
        fs::File::open(directory)?.sync_all()?;
        let history = Self { db, audio };
        history.clean_deleted_audio()?;
        Ok(history)
    }

    pub fn last(&self) -> Result<Option<Utterance>> {
        Ok(self.db.query_row("SELECT * FROM utterances WHERE raw_text IS NOT NULL ORDER BY started_at DESC, rowid DESC LIMIT 1", [], decode).optional()?)
    }

    pub fn set_status(&mut self, id: UtteranceId, status: UtteranceStatus) -> Result<()> {
        if !matches!(
            status,
            UtteranceStatus::Canceled | UtteranceStatus::Discarded
        ) {
            return Err("only cancellation or discard may change a completed status".into());
        }
        require_one(self.db.execute(
            "UPDATE utterances SET status = ?2 WHERE id = ?1 AND raw_text IS NOT NULL",
            params![id.0.to_string(), status_name(status)],
        )?)
    }

    pub fn audio_path(&self, id: UtteranceId) -> PathBuf {
        self.audio.join(format!("{}.wav", id.0))
    }

    fn clean_deleted_audio(&self) -> Result<()> {
        for item in fs::read_dir(&self.audio)? {
            let item = item?;
            let path = item.path();
            if path.extension().and_then(|s| s.to_str()) != Some("wav") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
            else {
                continue;
            };
            let references: i64 = self.db.query_row(
                "SELECT count(*) FROM utterances WHERE audio_id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )?;
            if references == 0 {
                fs::remove_file(path)?;
            }
        }
        fs::File::open(&self.audio)?.sync_all()?;
        Ok(())
    }

    fn rows(&self, sql: &str, query: Option<&str>) -> Result<Vec<Utterance>> {
        let mut statement = self.db.prepare(sql)?;
        let rows = match query {
            Some(query) => statement.query_map([query], decode)?,
            None => statement.query_map([], decode)?,
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

impl History for SqliteHistory {
    fn insert(&mut self, utterance: &Utterance) -> Result<()> {
        validate(utterance)?;
        self.db.execute("INSERT INTO utterances (id,audio_id,started_at,duration_ms,engine,raw_text,text,status,pasted_to,pasted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![utterance.id.0.to_string(), utterance.audio_id.0.to_string(), utterance.started_at, utterance.duration_ms as i64, utterance.engine, utterance.raw_text, utterance.text, utterance.status.map(status_name), utterance.pasted_to.as_ref().map(serde_json::to_string).transpose()?, utterance.pasted_at])?;
        Ok(())
    }

    fn finish(&mut self, utterance: &Utterance) -> Result<()> {
        validate(utterance)?;
        if utterance.raw_text.is_none() {
            return Err("recognition has not finished".into());
        }
        require_one(self.db.execute("UPDATE utterances SET duration_ms=?1,raw_text=?2,text=?3,status=?4 WHERE id=?5 AND audio_id=?6 AND raw_text IS NULL AND engine=?7 AND started_at=?8", params![utterance.duration_ms as i64,utterance.raw_text,utterance.text,utterance.status.map(status_name),utterance.id.0.to_string(),utterance.audio_id.0.to_string(),utterance.engine,utterance.started_at])?)
    }

    fn get(&self, id: UtteranceId) -> Result<Option<Utterance>> {
        Ok(self
            .db
            .query_row(
                "SELECT * FROM utterances WHERE id = ?1",
                [id.0.to_string()],
                decode,
            )
            .optional()?)
    }

    fn search(&self, query: &str) -> Result<Vec<Utterance>> {
        if query.is_empty() {
            self.rows(
                "SELECT * FROM utterances ORDER BY started_at DESC, rowid DESC",
                None,
            )
        } else {
            self.rows("SELECT u.* FROM utterances u JOIN utterances_fts f ON f.rowid=u.rowid WHERE utterances_fts MATCH ?1 ORDER BY u.started_at DESC, u.rowid DESC", Some(query))
        }
    }

    fn recover(&self) -> Result<Vec<Utterance>> {
        self.rows(
            "SELECT * FROM utterances WHERE raw_text IS NULL ORDER BY started_at, rowid",
            None,
        )
    }

    fn edit(&mut self, id: UtteranceId, text: &str) -> Result<()> {
        require_one(self.db.execute(
            "UPDATE utterances SET text=?1 WHERE id=?2 AND raw_text IS NOT NULL",
            params![text, id.0.to_string()],
        )?)
    }

    fn record_delivery(&mut self, id: UtteranceId, target: &Target, at: i64) -> Result<()> {
        safe_time(at)?;
        require_one(self.db.execute(
            "UPDATE utterances SET pasted_to=?1,pasted_at=?2 WHERE id=?3 AND raw_text IS NOT NULL",
            params![serde_json::to_string(target)?, at, id.0.to_string()],
        )?)
    }

    fn delete(&mut self, id: UtteranceId) -> Result<Option<UtteranceId>> {
        let tx = self.db.transaction()?;
        let row = tx.query_row(
            "SELECT * FROM utterances WHERE id=?1",
            [id.0.to_string()],
            decode,
        )?;
        tx.execute("DELETE FROM utterances WHERE id=?1", [id.0.to_string()])?;
        let references: i64 = tx.query_row(
            "SELECT count(*) FROM utterances WHERE audio_id=?1",
            [row.audio_id.0.to_string()],
            |row| row.get(0),
        )?;
        tx.commit()?;
        if references != 0 {
            return Ok(None);
        }
        match fs::remove_file(self.audio_path(row.audio_id)) {
            Ok(()) => fs::File::open(&self.audio)?.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(Some(row.audio_id))
    }
}

fn require_one(count: usize) -> Result<()> {
    if count == 1 {
        Ok(())
    } else {
        Err("unknown utterance or invalid history transition".into())
    }
}

pub(crate) fn validate(row: &Utterance) -> Result<()> {
    safe_time(row.started_at)?;
    if row.duration_ms > 9_007_199_254_740_991 {
        return Err("duration exceeds JavaScript safe integer range".into());
    }
    if let Some(at) = row.pasted_at {
        safe_time(at)?;
    }
    if row.raw_text.is_some() != row.text.is_some()
        || row.raw_text.is_some() != row.status.is_some()
        || row.pasted_to.is_some() != row.pasted_at.is_some()
        || (row.raw_text.is_none() && row.pasted_at.is_some())
    {
        return Err("inconsistent utterance fields".into());
    }
    Ok(())
}

fn safe_time(value: i64) -> Result<()> {
    if (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value) {
        Ok(())
    } else {
        Err("time exceeds JavaScript safe integer range".into())
    }
}

fn status_name(status: UtteranceStatus) -> &'static str {
    match status {
        UtteranceStatus::Done => "done",
        UtteranceStatus::Empty => "empty",
        UtteranceStatus::Canceled => "canceled",
        UtteranceStatus::Discarded => "discarded",
        UtteranceStatus::Recovered => "recovered",
        UtteranceStatus::Failed => "failed",
    }
}

fn decode(row: &Row<'_>) -> rusqlite::Result<Utterance> {
    fn invalid(
        index: usize,
        error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, error.into())
    }
    let id: String = row.get(0)?;
    let audio_id: String = row.get(1)?;
    let status: Option<String> = row.get(7)?;
    let target: Option<String> = row.get(8)?;
    Ok(Utterance {
        id: UtteranceId(uuid::Uuid::parse_str(&id).map_err(|error| invalid(0, error))?),
        audio_id: UtteranceId(uuid::Uuid::parse_str(&audio_id).map_err(|error| invalid(1, error))?),
        started_at: row.get(2)?,
        duration_ms: u64::try_from(row.get::<_, i64>(3)?).map_err(|error| invalid(3, error))?,
        engine: row.get(4)?,
        raw_text: row.get(5)?,
        text: row.get(6)?,
        status: status
            .map(|s| match s.as_str() {
                "done" => Ok(UtteranceStatus::Done),
                "empty" => Ok(UtteranceStatus::Empty),
                "canceled" => Ok(UtteranceStatus::Canceled),
                "discarded" => Ok(UtteranceStatus::Discarded),
                "recovered" => Ok(UtteranceStatus::Recovered),
                "failed" => Ok(UtteranceStatus::Failed),
                _ => Err(invalid(7, "unknown utterance status")),
            })
            .transpose()?,
        pasted_to: target
            .map(|text| serde_json::from_str(&text).map_err(|error| invalid(8, error)))
            .transpose()?,
        pasted_at: row.get(9)?,
    })
}
