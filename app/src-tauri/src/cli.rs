use crate::runtime::{Host, HostPlatform};
use voice_audio::Recorder;
use voice_core::{History, Mode, Result};

#[cfg(target_os = "linux")]
pub fn control<P: HostPlatform, R: Recorder>(
    host: &mut Host<P, R>,
    command: voice_platform::linux::control::ControlCommand,
) -> Result<String> {
    use voice_platform::linux::control::ControlCommand as C;
    match command {
        C::Start => host.start(Mode::Quick, false)?,
        C::Stop => host.stop()?,
        C::HistorySearch { query } => {
            return Ok(serde_json::to_string(&host.history.search(&query)?)?);
        }
        C::Status => return Ok(serde_json::to_string(&host.snapshot()?)?),
        command => {
            let (hotkey, observable) = match command {
                C::Hotkey | C::Toggle => (voice_core::Hotkey::Down, false),
                C::HotkeyInteractive | C::Interactive => (voice_core::Hotkey::Interactive, false),
                C::HotkeyInsertLast | C::InsertLast => (voice_core::Hotkey::InsertLast, false),
                C::Cancel => (voice_core::Hotkey::Cancel, false),
                _ => unreachable!(),
            };
            host.hotkey(hotkey, observable)?;
        }
    }
    Ok("applied".into())
}
pub fn import_vocabulary(
    paths: &crate::storage::Paths,
    source: &str,
    path: &std::path::Path,
) -> Result<()> {
    let terms: Vec<String> = match source {
        "codex" => {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            let dictionary = value
                .get("dictationDictionary")
                .ok_or("dictationDictionary is missing")?;
            match dictionary {
                serde_json::Value::Array(terms) => terms
                    .iter()
                    .map(|term| {
                        term.as_str()
                            .ok_or_else(|| "dictionary term must be a string".into())
                            .map(str::to_owned)
                    })
                    .collect::<Result<_>>()?,
                serde_json::Value::String(terms) => terms.lines().map(str::to_owned).collect(),
                _ => return Err("dictationDictionary must be a string or array".into()),
            }
        }
        "wispr" => {
            let db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let mut statement = db.prepare("SELECT phrase FROM Dictionary WHERE isDeleted = 0 AND isSnippet = 0 ORDER BY createdAt, id")?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<std::result::Result<_, _>>()?
        }
        _ => return Err("source must be wispr or codex".into()),
    };
    if terms.iter().any(|term| term.contains(['\n', '\r', '\0'])) {
        return Err("vocabulary terms must be single lines".into());
    }
    let file = paths.config.join("vocabulary.txt");
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for term in terms {
        text.push_str(&term);
        text.push('\n');
    }
    crate::storage::atomic_write(&file, text.as_bytes())
}
