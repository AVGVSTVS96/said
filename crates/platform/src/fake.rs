use std::cell::RefCell;

use crossbeam_channel::{Receiver, Sender, unbounded};
use voice_core::{Hotkey, Result, Target};

use crate::Platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformCall {
    Focus(Target),
    Paste {
        target: Target,
        text: String,
        send: bool,
    },
    MakeOverlay,
}

pub struct FakePlatform {
    pub calls: RefCell<Vec<PlatformCall>>,
    pub target: RefCell<Option<Target>>,
    sender: Sender<Hotkey>,
    receiver: Receiver<Hotkey>,
}

impl FakePlatform {
    pub fn new(target: Option<Target>) -> Self {
        let (sender, receiver) = unbounded();
        Self {
            calls: RefCell::new(vec![]),
            target: RefCell::new(target),
            sender,
            receiver,
        }
    }

    pub fn emit(&self, hotkey: Hotkey) {
        self.sender.send(hotkey).unwrap();
    }
}

impl Platform for FakePlatform {
    type Window = ();

    fn hotkeys(&self) -> Receiver<Hotkey> {
        self.receiver.clone()
    }

    fn focused(&self) -> Option<Target> {
        self.target.borrow().clone()
    }

    fn focus(&self, target: &Target) -> Result<()> {
        self.calls
            .borrow_mut()
            .push(PlatformCall::Focus(target.clone()));
        *self.target.borrow_mut() = Some(target.clone());
        Ok(())
    }

    fn paste(&self, target: &Target, text: &str, send: bool) -> Result<()> {
        if !self
            .focused()
            .as_ref()
            .is_some_and(|current| current.same_window(target))
        {
            return Err("target no longer focused".into());
        }
        self.calls.borrow_mut().push(PlatformCall::Paste {
            target: target.clone(),
            text: text.to_owned(),
            send,
        });
        Ok(())
    }

    fn make_overlay(&self, _: &()) {
        self.calls.borrow_mut().push(PlatformCall::MakeOverlay);
    }
}
