use voice_core::Hotkey;

pub const SHIFT: u64 = 1 << 17;
pub const CONTROL: u64 = 1 << 18;
pub const OPTION: u64 = 1 << 19;
pub const COMMAND: u64 = 1 << 20;
const MODIFIERS: u64 = SHIFT | CONTROL | OPTION | COMMAND;

#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    pub keycode: u16,
    pub modifiers: u64,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            keycode: 49,
            modifiers: CONTROL | OPTION,
        }
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
    shortcut: Shortcut,
    held: bool,
    swallowed: [bool; 128],
}
impl Decoder {
    pub fn new(shortcut: Shortcut) -> Self {
        Self {
            shortcut,
            held: false,
            swallowed: [false; 128],
        }
    }
    pub fn reset(&mut self) {
        self.held = false;
        self.swallowed.fill(false);
    }
    pub fn decode(&mut self, event: KeyEvent, recording: bool) -> Decoded {
        let mut result = Decoded::default();
        let key = usize::from(event.keycode);
        if event.kind == 11 {
            if key < self.swallowed.len() {
                result.suppress = std::mem::take(&mut self.swallowed[key]);
            }
            if event.keycode == self.shortcut.keycode && std::mem::take(&mut self.held) {
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
        } else if event.keycode == self.shortcut.keycode && flags == self.shortcut.modifiers {
            self.held = true;
            Some(Hotkey::Down)
        } else if event.keycode == self.shortcut.keycode && flags == self.shortcut.modifiers | SHIFT
        {
            self.held = true;
            Some(Hotkey::Interactive)
        } else if event.keycode == 9 && flags == CONTROL | OPTION {
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
        let mut decoder = Decoder::new(Shortcut::default());
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
        let mut decoder = Decoder::new(Shortcut::default());
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
        let mut decoder = Decoder::new(Shortcut::default());
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
        let mut decoder = Decoder::new(Shortcut::default());
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
        let mut decoder = Decoder::new(Shortcut {
            keycode: 40,
            modifiers: COMMAND | OPTION,
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
