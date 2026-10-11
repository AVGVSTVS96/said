use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use crossbeam_channel::{Sender, bounded};
use futures_util::StreamExt;
use said_core::{Hotkey, Result};
use tokio::sync::{oneshot, watch};

pub struct PortalHotkeys {
    stop: Option<oneshot::Sender<()>>,
    enabled: watch::Sender<bool>,
    available: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PortalHotkeys {
    pub fn start(trigger: String, sender: Sender<Hotkey>) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (stop, stopped) = oneshot::channel();
        let (enabled, enabled_events) = watch::channel(true);
        let (ready, readiness) = bounded(1);
        let available = Arc::new(AtomicBool::new(false));
        let thread_available = available.clone();
        let thread = thread::spawn(move || {
            runtime.block_on(async {
                let result = listen(
                    trigger,
                    sender.clone(),
                    ready.clone(),
                    stopped,
                    enabled_events,
                    &thread_available,
                )
                .await;
                thread_available.store(false, Ordering::Release);
                if let Err(error) = result {
                    let _ = ready.send(Err(error.to_string()));
                }
            });
        });
        let mut portal = Self {
            stop: Some(stop),
            enabled,
            available,
            thread: Some(thread),
        };
        match readiness.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(())) => Ok(portal),
            Ok(Err(error)) => {
                portal.shutdown();
                Err(error.into())
            }
            Err(error) => {
                portal.shutdown();
                Err(format!("GlobalShortcuts registration did not finish: {error}").into())
            }
        }
    }

    pub fn available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }
    pub fn set_enabled(&self, enabled: bool) {
        let _ = self.enabled.send(enabled);
    }

    fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PortalHotkeys {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn listen(
    trigger: String,
    sender: Sender<Hotkey>,
    ready: Sender<std::result::Result<(), String>>,
    mut stopped: oneshot::Receiver<()>,
    mut enabled: watch::Receiver<bool>,
    available: &AtomicBool,
) -> Result<()> {
    let portal = tokio::select! {
        _ = &mut stopped => return Ok(()),
        result = GlobalShortcuts::new() => result?,
    };
    let session = tokio::select! {
        _ = &mut stopped => return Ok(()),
        result = portal.create_session() => result?,
    };
    let mut tracker = PortalTracker::default();
    let result = async {
        let session_path: String = serde_json::from_value(serde_json::to_value(&session)?)?;
        let mut activated = portal.receive_activated().await?;
        let mut deactivated = portal.receive_deactivated().await?;
        let mut closed = session.receive_closed().await?;
        let interactive = format!("SHIFT+{trigger}");
        let shortcuts = [
            NewShortcut::new("dictation", "Dictate").preferred_trigger(trigger.as_str()),
            NewShortcut::new("interactive", "Interactive dictation")
                .preferred_trigger(interactive.as_str()),
            NewShortcut::new("insert-last", "Insert last dictation")
                .preferred_trigger("CTRL+ALT+v"),
        ];
        portal
            .bind_shortcuts(&session, &shortcuts, None)
            .await?
            .response()?;
        available.store(true, Ordering::Release);
        ready.send(Ok(()))?;
        loop {
            tokio::select! {
                _ = closed.next() => break,
                changed = enabled.changed() => {
                    if changed.is_err() { break; }
                    for event in tracker.reset() { sender.send(event)?; }
                }
                event = activated.next() => {
                    let Some(event) = event else { break; };
                    if event.session_handle().as_str() == session_path && *enabled.borrow() {
                        for event in tracker.activated(event.shortcut_id()) { sender.send(event)?; }
                    }
                }
                event = deactivated.next() => {
                    let Some(event) = event else { break; };
                    if event.session_handle().as_str() == session_path {
                        for event in tracker.deactivated(event.shortcut_id()) { sender.send(event)?; }
                    }
                }
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };
    let result = tokio::select! { _ = &mut stopped => Ok(()), result = result => result };
    available.store(false, Ordering::Release);
    for event in tracker.reset() {
        let _ = sender.send(event);
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), session.close()).await;
    result
}

#[derive(Default)]
struct PortalTracker {
    active: BTreeSet<String>,
}

impl PortalTracker {
    fn activated(&mut self, shortcut: &str) -> Vec<Hotkey> {
        let event = match shortcut {
            "dictation" => Hotkey::Down,
            "interactive" => Hotkey::Interactive,
            "insert-last" => Hotkey::InsertLast,
            _ => return Vec::new(),
        };
        if !self.active.insert(shortcut.into()) {
            return Vec::new();
        }
        vec![event]
    }
    fn deactivated(&mut self, shortcut: &str) -> Vec<Hotkey> {
        if self.active.remove(shortcut) && matches!(shortcut, "dictation" | "interactive") {
            vec![Hotkey::Up]
        } else {
            Vec::new()
        }
    }
    fn reset(&mut self) -> Vec<Hotkey> {
        let recording = self.active.contains("dictation") || self.active.contains("interactive");
        self.active.clear();
        if recording {
            vec![Hotkey::Up]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_and_deactivation_are_real_edges() {
        let mut tracker = PortalTracker::default();
        assert_eq!(tracker.activated("dictation"), [Hotkey::Down]);
        assert!(tracker.activated("dictation").is_empty());
        assert!(tracker.deactivated("foreign").is_empty());
        assert_eq!(tracker.deactivated("dictation"), [Hotkey::Up]);
        assert!(tracker.deactivated("dictation").is_empty());
        assert_eq!(tracker.activated("interactive"), [Hotkey::Interactive]);
        assert_eq!(tracker.reset(), [Hotkey::Up]);
    }
}
