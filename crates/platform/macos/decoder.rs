use said_core::Hotkey;

pub const SHIFT: u64 = 1 << 17;
pub const CONTROL: u64 = 1 << 18;
pub const OPTION: u64 = 1 << 19;
pub const COMMAND: u64 = 1 << 20;
const MODIFIERS: u64 = SHIFT | CONTROL | OPTION | COMMAND;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub keycode: u16,
    pub modifiers: u64,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            keycode: 49,
            modifiers: OPTION,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcuts {
    pub dictation: Shortcut,
    pub interactive: Shortcut,
    pub insert_last: Shortcut,
}
impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            dictation: Shortcut::default(),
            interactive: Shortcut {
                keycode: 7,
                modifiers: OPTION,
            },
            insert_last: Shortcut {
                keycode: 0,
                modifiers: OPTION,
            },
        }
    }
}
impl Shortcuts {
    pub fn validate(&self) -> said_core::Result<()> {
        let keys = [self.dictation, self.interactive, self.insert_last];
        if keys.iter().any(|key| {
            key.keycode >= 128
                || key.keycode == 53
                || key.modifiers == 0
                || key.modifiers & !MODIFIERS != 0
        }) || keys[0] == keys[1]
            || keys[0] == keys[2]
            || keys[1] == keys[2]
        {
            return Err("shortcuts need distinct keys and supported modifiers".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub kind: u32,
    pub keycode: u16,
    pub flags: u64,
    pub repeat: bool,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub hotkey: Option<Hotkey>,
    pub suppress: bool,
}

pub struct Decoder {
    shortcuts: Shortcuts,
    held: Option<u16>,
    swallowed: [bool; 128],
}
impl Decoder {
    pub fn new(shortcuts: Shortcuts) -> Self {
        Self {
            shortcuts,
            held: None,
            swallowed: [false; 128],
        }
    }
    pub fn reset(&mut self) {
        self.held = None;
        self.swallowed.fill(false);
    }
    pub fn decode(&mut self, event: KeyEvent, recording: bool) -> Decoded {
        let mut result = Decoded::default();
        let key = usize::from(event.keycode);
        if event.kind == 11 {
            if key < self.swallowed.len() {
                result.suppress = std::mem::take(&mut self.swallowed[key]);
            }
            if self.held == Some(event.keycode) {
                self.held = None;
                result.hotkey = Some(Hotkey::Up);
            }
            return result;
        }
        if event.kind != 10 {
            return result;
        }
        if key < self.swallowed.len() && self.swallowed[key] {
            result.suppress = true;
            return result;
        }
        if event.repeat {
            return result;
        }
        let flags = event.flags & MODIFIERS;
        result.hotkey = if event.keycode == 53 && recording {
            Some(Hotkey::Cancel)
        } else if event.keycode == self.shortcuts.dictation.keycode
            && flags == self.shortcuts.dictation.modifiers
        {
            self.held = Some(event.keycode);
            Some(Hotkey::Down)
        } else if event.keycode == self.shortcuts.interactive.keycode
            && flags == self.shortcuts.interactive.modifiers
        {
            self.held = Some(event.keycode);
            Some(Hotkey::Interactive)
        } else if event.keycode == self.shortcuts.insert_last.keycode
            && flags == self.shortcuts.insert_last.modifiers
        {
            Some(Hotkey::InsertLast)
        } else {
            None
        };
        result.suppress = result.hotkey.is_some();
        if result.suppress && key < self.swallowed.len() {
            self.swallowed[key] = true;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shortcuts() -> Shortcuts {
        Shortcuts {
            dictation: Shortcut {
                keycode: 49,
                modifiers: CONTROL | OPTION,
            },
            interactive: Shortcut {
                keycode: 49,
                modifiers: CONTROL | OPTION | SHIFT,
            },
            insert_last: Shortcut {
                keycode: 9,
                modifiers: CONTROL | OPTION,
            },
        }
    }
    fn event(kind: u32, keycode: u16, flags: u64) -> KeyEvent {
        KeyEvent {
            kind,
            keycode,
            flags,
            repeat: false,
        }
    }
    #[test]
    fn release_survives_modifier_release_and_repeat() {
        let mut decoder = Decoder::new(shortcuts());
        assert_eq!(
            decoder
                .decode(event(10, 49, CONTROL | OPTION), false)
                .hotkey,
            Some(Hotkey::Down)
        );
        assert!(
            decoder
                .decode(
                    KeyEvent {
                        repeat: true,
                        ..event(10, 49, CONTROL | OPTION)
                    },
                    false
                )
                .suppress
        );
        assert_eq!(
            decoder.decode(event(12, 58, CONTROL), false),
            Decoded::default()
        );
        assert_eq!(
            decoder.decode(event(11, 49, 0), false),
            Decoded {
                hotkey: Some(Hotkey::Up),
                suppress: true
            }
        );
        assert_eq!(decoder.decode(event(11, 49, 0), false), Decoded::default());
    }
    #[test]
    fn interactive_insert_and_cancel_do_not_leak_release() {
        let mut decoder = Decoder::new(shortcuts());
        assert_eq!(
            decoder
                .decode(event(10, 49, CONTROL | OPTION | SHIFT), false)
                .hotkey,
            Some(Hotkey::Interactive)
        );
        assert_eq!(
            decoder.decode(event(11, 49, 0), false).hotkey,
            Some(Hotkey::Up)
        );
        for (key, flags, hotkey) in [
            (9, CONTROL | OPTION, Hotkey::InsertLast),
            (53, 0, Hotkey::Cancel),
        ] {
            assert_eq!(
                decoder.decode(event(10, key, flags), true).hotkey,
                Some(hotkey)
            );
            assert_eq!(
                decoder.decode(event(11, key, 0), false),
                Decoded {
                    hotkey: None,
                    suppress: true
                }
            );
        }
        assert_eq!(decoder.decode(event(10, 53, 0), false), Decoded::default());
    }
    #[test]
    fn unrelated_and_extra_modifiers_pass_through() {
        let mut decoder = Decoder::new(shortcuts());
        for keycode in 0..128 {
            assert_eq!(
                decoder.decode(event(10, keycode, COMMAND), false),
                Decoded::default()
            );
            assert_eq!(
                decoder.decode(event(11, keycode, 0), false),
                Decoded::default()
            );
        }
    }
    #[test]
    fn resetting_after_unavailability_drops_stale_release() {
        let mut decoder = Decoder::new(shortcuts());
        decoder.decode(event(10, 49, CONTROL | OPTION), false);
        decoder.reset();
        assert_eq!(decoder.decode(event(11, 49, 0), false), Decoded::default());
        assert_eq!(
            decoder
                .decode(event(10, 49, CONTROL | OPTION), false)
                .hotkey,
            Some(Hotkey::Down)
        );
    }
    #[test]
    fn configured_shortcut_ignores_caps_lock_but_pairs_its_release() {
        let mut decoder = Decoder::new(Shortcuts {
            dictation: Shortcut {
                keycode: 40,
                modifiers: COMMAND | OPTION,
            },
            ..shortcuts()
        });
        assert_eq!(
            decoder.decode(event(10, 49, CONTROL | OPTION), false),
            Decoded::default()
        );
        assert_eq!(
            decoder
                .decode(event(10, 40, COMMAND | OPTION | (1 << 16)), false)
                .hotkey,
            Some(Hotkey::Down)
        );
        assert_eq!(
            decoder.decode(event(11, 40, 0), false).hotkey,
            Some(Hotkey::Up)
        );
    }
}
