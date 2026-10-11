use crossbeam_channel::{Receiver, bounded};
use said_core::{Result, SessionEvent};
use std::collections::BTreeMap;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

pub struct Login {
    events: Receiver<SessionEvent>,
    active: bool,
    connection: Connection,
}
impl Login {
    pub fn start() -> Result<Self> {
        let connection = Connection::system()?;
        let manager = Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )?;
        let path: OwnedObjectPath = match manager.call("GetSessionByPID", &(std::process::id(),)) {
            Ok(path) => path,
            Err(error) => match std::env::var("XDG_SESSION_ID") {
                Ok(id) => manager.call("GetSession", &(id,))?,
                Err(_) => return Err(error.into()),
            },
        };
        let properties = Proxy::new(
            &connection,
            "org.freedesktop.login1",
            path.clone(),
            "org.freedesktop.DBus.Properties",
        )?;
        let signals = properties.receive_signal("PropertiesChanged")?;
        let session = Proxy::new(
            &connection,
            "org.freedesktop.login1",
            path,
            "org.freedesktop.login1.Session",
        )?;
        let mut active = session.get_property::<bool>("Active")?;
        let mut locked = session.get_property::<bool>("LockedHint")?;
        let initial_active = active && !locked;
        let (sender, events) = bounded(64);
        sender.send(event(initial_active))?;
        std::thread::spawn(move || {
            let mut previous = initial_active;
            for signal in signals {
                let properties: std::result::Result<
                    (String, BTreeMap<String, OwnedValue>, Vec<String>),
                    _,
                > = signal.body().deserialize();
                let Ok((interface, mut changed, invalidated)) = properties else {
                    break;
                };
                if interface != "org.freedesktop.login1.Session" {
                    continue;
                }
                if invalidated
                    .iter()
                    .any(|name| matches!(name.as_str(), "Active" | "LockedHint"))
                {
                    break;
                }
                if let Some(value) = changed.remove("Active") {
                    active = bool::try_from(value).unwrap_or(false);
                }
                if let Some(value) = changed.remove("LockedHint") {
                    locked = bool::try_from(value).unwrap_or(true);
                }
                let current = active && !locked;
                if current != previous {
                    if sender.send(event(current)).is_err() {
                        return;
                    }
                    previous = current;
                }
            }
            let _ = sender.send(event(false));
        });
        Ok(Self {
            events,
            active: initial_active,
            connection,
        })
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        self.events
            .try_iter()
            .inspect(|event| self.active = matches!(event, SessionEvent::SessionAvailable))
            .collect()
    }
}
impl Drop for Login {
    fn drop(&mut self) {
        let _ = self.connection.clone().close();
    }
}
fn event(active: bool) -> SessionEvent {
    if active {
        SessionEvent::SessionAvailable
    } else {
        SessionEvent::SessionUnavailable {
            reason: "Session locked, inactive or logind disconnected".into(),
        }
    }
}
