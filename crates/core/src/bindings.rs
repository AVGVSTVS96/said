use std::path::Path;

use ts_rs::{ExportError, TS};

use crate::types::*;

pub fn export(output: impl AsRef<Path>) -> std::result::Result<(), ExportError> {
    let config = ts_rs::Config::default().with_out_dir(output.as_ref());
    Utterance::export_all(&config)?;
    Hotkey::export_all(&config)?;
    WorkerRequest::export_all(&config)?;
    WorkerResponse::export_all(&config)?;
    AudioEvent::export_all(&config)?;
    SessionEvent::export_all(&config)?;
    Effect::export_all(&config)?;
    SessionView::export_all(&config)?;
    UiAction::export_all(&config)?;
    Settings::export_all(&config)?;
    InputDevice::export_all(&config)?;
    Ok(())
}
