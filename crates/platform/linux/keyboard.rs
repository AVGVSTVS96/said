use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use said_core::Result;
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1, zwp_virtual_keyboard_v1,
};
use xkbcommon::xkb;

struct State;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ignore zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1);

pub struct VirtualKeyboard {
    keyboard: zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
    queue: EventQueue<State>,
    control: u32,
    shift: u32,
    alt: u32,
    started: Instant,
}

impl VirtualKeyboard {
    pub fn connect(runtime: &Path, display: &OsStr) -> Result<Self> {
        let connection = Connection::from_socket(UnixStream::connect(runtime.join(display))?)?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
        let handle = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&handle, 1..=1, ())?;
        let manager: zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1 =
            globals.bind(&handle, 1..=1, ())?;
        let keyboard = manager.create_virtual_keyboard(&seat, &handle, ());
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "us",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("could not compile paste keymap")?;
        let contents = CString::new(keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1))?;
        let descriptor = unsafe { libc::memfd_create(c"said-keymap".as_ptr(), libc::MFD_CLOEXEC) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        file.write_all(contents.as_bytes_with_nul())?;
        keyboard.keymap(1, file.as_fd(), contents.as_bytes_with_nul().len() as u32);
        queue.roundtrip(&mut State)?;
        Ok(Self {
            keyboard,
            queue,
            control: 1 << keymap.mod_get_index(xkb::MOD_NAME_CTRL),
            shift: 1 << keymap.mod_get_index(xkb::MOD_NAME_SHIFT),
            alt: 1 << keymap.mod_get_index(xkb::MOD_NAME_ALT),
            started: Instant::now(),
        })
    }

    pub fn physical_chord(&mut self, keys: &[u32]) -> Result<()> {
        let time = self.started.elapsed().as_millis() as u32;
        for key in keys {
            self.keyboard.key(time, *key, 1);
            self.queue.roundtrip(&mut State)?;
        }
        for key in keys.iter().rev() {
            self.keyboard.key(time, *key, 0);
            self.queue.roundtrip(&mut State)?;
        }
        Ok(())
    }

    pub fn tap(&mut self, key: u32, control: bool, shift: bool, alt: bool) -> Result<()> {
        let modifiers = if control { self.control } else { 0 }
            | if shift { self.shift } else { 0 }
            | if alt { self.alt } else { 0 };
        self.keyboard.modifiers(modifiers, 0, 0, 0);
        let time = self.started.elapsed().as_millis() as u32;
        self.keyboard.key(time, key, 1);
        self.keyboard.key(time, key, 0);
        self.keyboard.modifiers(0, 0, 0, 0);
        self.queue.roundtrip(&mut State)?;
        Ok(())
    }
}

impl Drop for VirtualKeyboard {
    fn drop(&mut self) {
        self.keyboard.destroy();
        let _ = self.queue.roundtrip(&mut State);
    }
}
