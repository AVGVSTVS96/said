use crossbeam_channel::{Receiver, bounded};
use voice_core::{Result, SessionEvent};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
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
        let initial_active = active(&connection, path.clone())?;
        let (sender, events) = bounded(64);
        sender.send(event(initial_active))?;
        let worker_connection = connection.clone();
        std::thread::spawn(move || {
            for _ in signals {
                let current = active(&worker_connection, path.clone()).unwrap_or(false);
                if sender.send(event(current)).is_err() {
                    return;
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
fn active(connection: &Connection, path: OwnedObjectPath) -> Result<bool> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )?;
    Ok(proxy.get_property::<bool>("Active")? && !proxy.get_property::<bool>("LockedHint")?)
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
