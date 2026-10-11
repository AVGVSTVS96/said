#[cfg(target_os = "linux")]
use crate::runtime::{Host, HostPlatform};
#[cfg(target_os = "linux")]
use said_audio::Recorder;
#[cfg(target_os = "linux")]
use said_core::Result;
#[cfg(target_os = "linux")]
use said_core::{History, Mode};

#[cfg(target_os = "linux")]
pub fn control<P: HostPlatform, R: Recorder>(
    host: &mut Host<P, R>,
    command: said_platform::linux::control::ControlCommand,
) -> Result<String> {
    use said_platform::linux::control::ControlCommand as C;
    match command {
        C::Start => host.start(Mode::Quick, false)?,
        C::Stop => host.stop()?,
        C::HistorySearch { query } => {
            return Ok(serde_json::to_string(&host.history.search(&query)?)?);
        }
        C::Status => return Ok(serde_json::to_string(&host.snapshot()?)?),
        command => {
            let (hotkey, observable) = match command {
                C::Hotkey | C::Toggle => (said_core::Hotkey::Down, false),
                C::HotkeyInteractive | C::Interactive => (said_core::Hotkey::Interactive, false),
                C::HotkeyInsertLast | C::InsertLast => (said_core::Hotkey::InsertLast, false),
                C::Cancel => (said_core::Hotkey::Cancel, false),
                _ => unreachable!(),
            };
            host.hotkey(hotkey, observable)?;
        }
    }
    Ok("applied".into())
}
