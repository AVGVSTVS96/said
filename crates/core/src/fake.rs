use std::collections::BTreeMap;

use crate::{History, Result, Target, Utterance, UtteranceId};

#[derive(Debug, Default)]
pub struct FakeHistory {
    pub rows: BTreeMap<UtteranceId, Utterance>,
    pub saves: Vec<UtteranceId>,
}

impl History for FakeHistory {
    fn insert(&mut self, utterance: &Utterance) -> Result<()> {
        crate::history::validate(utterance)?;
        if self.rows.contains_key(&utterance.id) {
            return Err("utterance already exists".into());
        }
        self.rows.insert(utterance.id, utterance.clone());
        self.saves.push(utterance.id);
        Ok(())
    }

    fn finish(&mut self, utterance: &Utterance) -> Result<()> {
        crate::history::validate(utterance)?;
        if utterance.raw_text.is_none() {
            return Err("recognition has not finished".into());
        }
        let row = self
            .rows
            .get_mut(&utterance.id)
            .ok_or("unknown utterance")?;
        if row.raw_text.is_some()
            || row.audio_id != utterance.audio_id
            || row.engine != utterance.engine
            || row.started_at != utterance.started_at
        {
            return Err("original text and audio identity are immutable".into());
        }
        *row = utterance.clone();
        self.saves.push(utterance.id);
        Ok(())
    }

    fn get(&self, id: UtteranceId) -> Result<Option<Utterance>> {
        Ok(self.rows.get(&id).cloned())
    }

    fn search(&self, query: &str) -> Result<Vec<Utterance>> {
        let mut rows: Vec<_> = self
            .rows
            .values()
            .filter(|row| {
                row.text.as_deref().unwrap_or_default().contains(query)
                    || row.raw_text.as_deref().unwrap_or_default().contains(query)
            })
            .cloned()
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.started_at));
        Ok(rows)
    }

    fn recover(&self) -> Result<Vec<Utterance>> {
        Ok(self
            .rows
            .values()
            .filter(|row| row.raw_text.is_none())
            .cloned()
            .collect())
    }

    fn edit(&mut self, id: UtteranceId, text: &str) -> Result<()> {
        let row = self.rows.get_mut(&id).ok_or("unknown utterance")?;
        if row.raw_text.is_none() {
            return Err("recognition has not finished".into());
        }
        row.text = Some(text.to_owned());
        Ok(())
    }

    fn record_delivery(&mut self, id: UtteranceId, target: &Target, at: i64) -> Result<()> {
        let row = self.rows.get_mut(&id).ok_or("unknown utterance")?;
        if row.raw_text.is_none() {
            return Err("recognition has not finished".into());
        }
        row.pasted_to = Some(target.clone());
        row.pasted_at = Some(at);
        Ok(())
    }

    fn delete(&mut self, id: UtteranceId) -> Result<Option<UtteranceId>> {
        let row = self.rows.remove(&id).ok_or("unknown utterance")?;
        Ok((!self
            .rows
            .values()
            .any(|other| other.audio_id == row.audio_id))
        .then_some(row.audio_id))
    }
}

impl FakeHistory {
    pub fn last(&self) -> Option<Utterance> {
        self.rows
            .values()
            .filter(|row| row.raw_text.is_some())
            .max_by_key(|row| {
                (
                    row.started_at,
                    self.saves.iter().rposition(|id| *id == row.id),
                )
            })
            .cloned()
    }

    pub fn set_status(&mut self, id: UtteranceId, status: crate::UtteranceStatus) -> Result<()> {
        if !matches!(
            status,
            crate::UtteranceStatus::Canceled | crate::UtteranceStatus::Discarded
        ) {
            return Err("only cancellation or discard may change a completed status".into());
        }
        let row = self.rows.get_mut(&id).ok_or("unknown utterance")?;
        if row.raw_text.is_none() {
            return Err("recognition has not finished".into());
        }
        row.status = Some(status);
        Ok(())
    }
}
