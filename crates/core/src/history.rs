use crate::{Result, Target, Utterance, UtteranceId};

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
