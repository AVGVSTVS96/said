use crossbeam_channel::{Receiver, Sender, bounded};
use futures_util::StreamExt;
use said_core::{Result, SessionEvent};
use std::collections::BTreeMap;
use tauri::async_runtime;
use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, proxy::SignalStream};

pub struct Login {
    events: Receiver<SessionEvent>,
    active: bool,
    connection: Connection,
}
impl Login {
    pub fn start() -> Result<Self> {
        let (connection, signals, active, locked) = async_runtime::block_on(connect())?;
        let initial_active = active && !locked;
        let (sender, events) = bounded(64);
        sender.send(event(initial_active))?;
        async_runtime::spawn(watch(signals, sender, active, locked));
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
        let _ = async_runtime::block_on(self.connection.clone().close());
    }
}

async fn connect() -> Result<(Connection, SignalStream<'static>, bool, bool)> {
    let connection = Connection::system().await?;
    let manager = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let path: OwnedObjectPath = match manager
        .call("GetSessionByPID", &(std::process::id(),))
        .await
    {
        Ok(path) => path,
        Err(error) => match std::env::var("XDG_SESSION_ID") {
            Ok(id) => manager.call("GetSession", &(id,)).await?,
            Err(_) => return Err(error.into()),
        },
    };
    let properties = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        path.clone(),
        "org.freedesktop.DBus.Properties",
    )
    .await?;
    let signals = properties.receive_signal("PropertiesChanged").await?;
    let session = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )
    .await?;
    let active = session.get_property::<bool>("Active").await?;
    let locked = session.get_property::<bool>("LockedHint").await?;
    Ok((connection, signals, active, locked))
}

async fn watch(
    mut signals: SignalStream<'static>,
    sender: Sender<SessionEvent>,
    mut active: bool,
    mut locked: bool,
) {
    let mut previous = active && !locked;
    while let Some(signal) = signals.next().await {
        let properties: std::result::Result<
            (String, BTreeMap<String, OwnedVariant>, Vec<String>),
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
