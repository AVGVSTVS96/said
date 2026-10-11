use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct UtteranceId(pub Uuid);

impl UtteranceId {
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Target {
    pub app_id: String,
    pub window_id: String,
    pub name: String,
}

impl Target {
    pub fn same_window(&self, other: &Self) -> bool {
        self.app_id == other.app_id && self.window_id == other.window_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UtteranceStatus {
    Done,
    Empty,
    Canceled,
    Discarded,
    Recovered,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Utterance {
    pub id: UtteranceId,
    pub audio_id: UtteranceId,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub engine: String,
    pub raw_text: Option<String>,
    pub text: Option<String>,
    pub status: Option<UtteranceStatus>,
    pub pasted_to: Option<Target>,
    #[ts(type = "number | null")]
    pub pasted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "text", rename_all = "snake_case")]
pub enum EngineEvent {
    Partial(String),
    Committed(String),
    Final,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EngineOutput {
    pub id: UtteranceId,
    pub event: EngineEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Hotkey {
    Down,
    Up,
    Interactive,
    InsertLast,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerRequest {
    Start {
        id: UtteranceId,
    },
    Audio {
        id: UtteranceId,
        #[ts(type = "number")]
        sequence: u64,
    },
    Finish {
        id: UtteranceId,
    },
    Cancel {
        id: UtteranceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerResponse {
    Event {
        id: UtteranceId,
        event: EngineEvent,
    },
    AudioAck {
        id: UtteranceId,
        #[ts(type = "number")]
        sequence: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AudioFrame {
    pub id: UtteranceId,
    pub pcm: Vec<i16>,
    pub level: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioEvent {
    Frame(AudioFrame),
    Stopped {
        id: UtteranceId,
        #[ts(type = "number")]
        duration_ms: u64,
    },
    Disconnected {
        id: UtteranceId,
    },
    Error {
        id: UtteranceId,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Quick,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Recording,
    Finishing,
    Saving,
    Saved,
    Reviewing,
    Pasting,
    Pasted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionView {
    pub id: UtteranceId,
    pub target: Option<Target>,
    pub mode: Mode,
    pub phase: Phase,
    pub committed: String,
    pub partial: String,
    pub level: Option<f32>,
    pub edited: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StartContext {
    pub id: UtteranceId,
    #[ts(type = "number")]
    pub started_at: i64,
    pub engine: String,
    pub target: Option<Target>,
    pub mode: Mode,
    pub send: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SessionEvent {
    ReplayReady {
        id: UtteranceId,
        recognition_id: UtteranceId,
    },
    FocusChanged {
        target: Option<Target>,
    },
    Ui(UiAction),
    Recover {
        utterance: Utterance,
    },
    InsertLast {
        utterance: Option<Utterance>,
    },
    SessionUnavailable {
        reason: String,
    },
    SessionAvailable,
    EditSaved {
        id: UtteranceId,
    },
    EditFailed {
        id: UtteranceId,
        message: String,
    },
    HotkeyDown(StartContext),
    HotkeyUp {
        id: UtteranceId,
    },
    CaptureFailed {
        id: UtteranceId,
        message: String,
    },
    Audio(AudioEvent),
    Engine(EngineOutput),
    Saved {
        id: UtteranceId,
    },
    SaveFailed {
        id: UtteranceId,
        message: String,
    },
    FocusChecked {
        id: UtteranceId,
        target: Option<Target>,
    },
    Pasted {
        id: UtteranceId,
        #[ts(type = "number")]
        at: i64,
    },
    PasteFailed {
        id: UtteranceId,
        message: String,
    },
    Cancel {
        id: UtteranceId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    AllocateReplay {
        id: UtteranceId,
    },
    RecognizeFile {
        id: UtteranceId,
        audio_id: UtteranceId,
        engine: String,
    },
    FocusTarget {
        id: UtteranceId,
        target: Target,
    },
    Copy {
        id: UtteranceId,
        text: String,
    },
    EditHistory {
        id: UtteranceId,
        text: String,
    },
    SetStatus {
        id: UtteranceId,
        status: UtteranceStatus,
    },
    LoadLast,
    StartCapture {
        id: UtteranceId,
    },
    StopCapture {
        id: UtteranceId,
    },
    StartEngine {
        id: UtteranceId,
    },
    FeedEngine {
        id: UtteranceId,
        pcm: Vec<i16>,
    },
    FinishEngine {
        id: UtteranceId,
    },
    CancelEngine {
        id: UtteranceId,
    },
    Save {
        utterance: Utterance,
    },
    CheckFocus {
        id: UtteranceId,
    },
    Paste {
        id: UtteranceId,
        target: Target,
        text: String,
        send: bool,
    },
    RecordDelivery {
        id: UtteranceId,
        target: Target,
        #[ts(type = "number")]
        at: i64,
    },
    Show {
        view: SessionView,
    },
    Notify {
        id: UtteranceId,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiAction {
    Insert { id: UtteranceId },
    Copy { id: UtteranceId },
    Edit { id: UtteranceId, text: String },
    Discard { id: UtteranceId },
    Cancel { id: UtteranceId },
    InsertLast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppSettings {
    pub paste: String,
    pub send: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Settings {
    pub hotkey: String,
    pub interactive_hotkey: String,
    pub insert_last_hotkey: String,
    pub engine: String,
    pub microphone: Option<String>,
    pub apps: BTreeMap<String, AppSettings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostAction {
    SettingsSave {
        settings: Settings,
    },
    DownloadModel,
    Permission {
        permission: String,
    },
    HistorySearch {
        query: String,
    },
    HistoryDelete {
        id: UtteranceId,
    },
    HistoryRetry {
        id: UtteranceId,
    },
    HistoryRetention {
        #[ts(type = "number")]
        before: i64,
    },
    Start {
        mode: Mode,
        scratch: bool,
    },
    Stop,
    SetupComplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ModelInfo {
    pub id: String,
    pub license: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct DownloadState {
    pub state: String,
    pub file: String,
    #[ts(type = "number")]
    pub received: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct HostState {
    pub models: Vec<ModelInfo>,
    pub download: DownloadState,
    pub permissions: BTreeMap<String, String>,
    pub instructions: Vec<String>,
    pub notifications: BTreeMap<UtteranceId, String>,
    #[ts(type = "number")]
    pub disk_bytes: u64,
    pub disk_error: Option<String>,
    pub unavailable: Option<String>,
    pub scratch: Option<UtteranceId>,
    pub setup_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct HostSnapshot {
    #[ts(type = "number")]
    pub sequence: u64,
    pub sessions: Vec<SessionView>,
    pub history: Vec<Utterance>,
    pub settings: Settings,
    pub devices: Vec<InputDevice>,
    pub host: HostState,
}
