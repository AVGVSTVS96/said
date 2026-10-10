use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::ptr;
use std::rc::Rc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use voice_core::{EngineEvent, EngineOutput, Result, UtteranceId};

use crate::{Engine, native::Library};

// ABI: https://github.com/moonshine-ai/moonshine/blob/v0.1.5/core/moonshine-c-api.h
#[repr(C)]
struct Line {
    text: *const c_char,
    audio: *const f32,
    audio_count: usize,
    start: f32,
    duration: f32,
    id: u64,
    complete: i8,
    updated: i8,
    new: i8,
    text_changed: i8,
    speakers_changed: i8,
    speakers: *const c_void,
    speaker_count: u64,
    latency: u32,
    words: *const c_void,
    word_count: u64,
}

#[repr(C)]
struct Transcript {
    lines: *const Line,
    count: u64,
}

#[repr(C)]
struct OptionValue {
    name: *const c_char,
    value: *const c_char,
}

type Load = unsafe extern "C" fn(*const c_char, u32, *const OptionValue, u64, i32) -> i32;
type Unary = unsafe extern "C" fn(i32, i32) -> i32;

struct Api {
    load: Load,
    free: unsafe extern "C" fn(i32),
    create: unsafe extern "C" fn(i32, u32) -> i32,
    start: Unary,
    stop: Unary,
    free_stream: Unary,
    add: unsafe extern "C" fn(i32, i32, *const f32, u64, i32, u32) -> i32,
    transcribe: unsafe extern "C" fn(i32, i32, u32, *mut *const Transcript) -> i32,
    keyterms: unsafe extern "C" fn(i32, *const c_char) -> i32,
    error: unsafe extern "C" fn(i32) -> *const c_char,
    _library: Library,
}

impl Api {
    fn open(path: &Path) -> Result<Self> {
        let library = Library::open(path)?;
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {
                unsafe { std::mem::transmute::<*mut c_void, $ty>(library.symbol($name)?) }
            };
        }
        let version = symbol!(c"moonshine_get_version", unsafe extern "C" fn() -> i32);
        if unsafe { version() } / 10000 != 3 {
            return Err("Moonshine requires the version 3 transcript ABI".into());
        }
        Ok(Self {
            load: symbol!(c"moonshine_load_transcriber_from_files", Load),
            free: symbol!(c"moonshine_free_transcriber", unsafe extern "C" fn(i32)),
            create: symbol!(
                c"moonshine_create_stream",
                unsafe extern "C" fn(i32, u32) -> i32
            ),
            start: symbol!(c"moonshine_start_stream", Unary),
            stop: symbol!(c"moonshine_stop_stream", Unary),
            free_stream: symbol!(c"moonshine_free_stream", Unary),
            add: symbol!(
                c"moonshine_transcribe_add_audio_to_stream",
                unsafe extern "C" fn(i32, i32, *const f32, u64, i32, u32) -> i32
            ),
            transcribe: symbol!(
                c"moonshine_transcribe_stream",
                unsafe extern "C" fn(i32, i32, u32, *mut *const Transcript) -> i32
            ),
            keyterms: symbol!(
                c"moonshine_transcriber_set_keyterms",
                unsafe extern "C" fn(i32, *const c_char) -> i32
            ),
            error: symbol!(
                c"moonshine_error_to_string",
                unsafe extern "C" fn(i32) -> *const c_char
            ),
            _library: library,
        })
    }

    fn check(&self, status: i32) -> Result<i32> {
        if status < 0 {
            let message = unsafe { (self.error)(status) };
            if message.is_null() {
                return Err(format!("Moonshine error {status}").into());
            }
            return Err(unsafe { CStr::from_ptr(message) }.to_str()?.into());
        }
        Ok(status)
    }
}

pub struct MoonshineModel {
    api: Api,
    transcriber: i32,
}

impl MoonshineModel {
    pub fn load(library: &Path, model: &Path, arch: u32) -> Result<Rc<Self>> {
        use std::os::unix::ffi::OsStrExt;
        if !matches!(arch, 2 | 4 | 5) {
            return Err("Moonshine requires a published streaming architecture".into());
        }
        let api = Api::open(library)?;
        let path = CString::new(model.as_os_str().as_bytes())?;
        let options = [
            OptionValue {
                name: c"ort_providers".as_ptr(),
                value: c"CPU".as_ptr(),
            },
            OptionValue {
                name: c"return_audio_data".as_ptr(),
                value: c"false".as_ptr(),
            },
        ];
        let transcriber = api.check(unsafe {
            (api.load)(
                path.as_ptr(),
                arch,
                options.as_ptr(),
                options.len() as u64,
                30000,
            )
        })?;
        Ok(Rc::new(Self { api, transcriber }))
    }
}

impl Drop for MoonshineModel {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.transcriber) };
    }
}

pub struct Moonshine {
    model: Rc<MoonshineModel>,
    stream: Option<i32>,
    started: bool,
    id: UtteranceId,
    sender: Sender<EngineOutput>,
    tail: Tail,
    buffered_samples: usize,
}

impl Moonshine {
    pub fn new(
        id: UtteranceId,
        library: &Path,
        model: &Path,
        arch: u32,
    ) -> Result<(Self, Receiver<EngineOutput>)> {
        Ok(Self::from_model(
            id,
            MoonshineModel::load(library, model, arch)?,
        ))
    }

    pub fn from_model(
        id: UtteranceId,
        model: Rc<MoonshineModel>,
    ) -> (Self, Receiver<EngineOutput>) {
        let (sender, receiver) = unbounded();
        (
            Self {
                model,
                stream: None,
                started: false,
                id,
                sender,
                tail: Tail::default(),
                buffered_samples: 0,
            },
            receiver,
        )
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.sender.send(EngineOutput { id: self.id, event });
    }

    fn update(&mut self, finalizing: bool) -> Result<()> {
        let mut transcript = ptr::null();
        let transcriber = self.model.transcriber;
        let stream = self.stream.ok_or("stream not started")?;
        self.model.api.check(unsafe {
            (self.model.api.transcribe)(transcriber, stream, 0, &mut transcript)
        })?;
        if transcript.is_null() {
            return Err("Moonshine returned no transcript".into());
        }
        let transcript = unsafe { &*transcript };
        let lines = if transcript.count == 0 {
            &[][..]
        } else {
            if transcript.lines.is_null()
                || transcript.count > isize::MAX as u64 / size_of::<Line>() as u64
            {
                return Err("Moonshine returned invalid lines".into());
            }
            unsafe { std::slice::from_raw_parts(transcript.lines, transcript.count as usize) }
        };
        let mut partial = String::new();
        if lines.len() < self.tail.committed_count {
            return Err("Moonshine removed committed lines".into());
        }
        for (index, line) in lines.iter().enumerate().skip(self.tail.committed_count) {
            if line.text.is_null() {
                return Err("Moonshine returned null text".into());
            }
            let text = unsafe { CStr::from_ptr(line.text) }.to_str()?;
            if line.complete != 0 {
                let event = self.tail.commit(index, text);
                self.emit(event);
            } else {
                if finalizing {
                    return Err("Moonshine left an incomplete line after stop".into());
                }
                partial.push_str(&self.tail.decorate(text));
            }
        }
        if finalizing {
            self.emit(EngineEvent::Final);
        } else if partial != self.tail.partial {
            self.tail.partial = partial.clone();
            self.emit(EngineEvent::Partial(partial));
        }
        Ok(())
    }

    fn close_stream(&mut self) {
        if let Some(stream) = self.stream.take() {
            unsafe { (self.model.api.free_stream)(self.model.transcriber, stream) };
        }
    }

    fn failed(&mut self, error: impl std::fmt::Display) {
        self.emit(EngineEvent::Error(error.to_string()));
        self.close_stream();
    }
}

impl Engine for Moonshine {
    fn start(&mut self, vocabulary: &[String]) -> Result<()> {
        if self.started {
            return Err("Moonshine instances are scoped to one utterance".into());
        }
        if vocabulary
            .iter()
            .any(|term| term.contains(',') || term.contains('\0'))
        {
            return Err("Moonshine vocabulary terms cannot contain commas or NUL".into());
        }
        let terms = CString::new(vocabulary.join(","))?;
        let handle = self.model.transcriber;
        self.model
            .api
            .check(unsafe { (self.model.api.keyterms)(handle, terms.as_ptr()) })?;
        self.started = true;
        let stream = self
            .model
            .api
            .check(unsafe { (self.model.api.create)(handle, 0) })?;
        self.stream = Some(stream);
        self.model
            .api
            .check(unsafe { (self.model.api.start)(handle, stream) })?;
        Ok(())
    }

    fn feed(&mut self, pcm16k: &[i16]) {
        let Some(stream) = self.stream else {
            return;
        };
        let handle = self.model.transcriber;
        let samples: Vec<f32> = pcm16k
            .iter()
            .map(|sample| f32::from(*sample) / 32768.0)
            .collect();
        let result = self
            .model
            .api
            .check(unsafe {
                (self.model.api.add)(
                    handle,
                    stream,
                    samples.as_ptr(),
                    samples.len() as u64,
                    16000,
                    0,
                )
            })
            .and_then(|_| {
                self.buffered_samples += pcm16k.len();
                if self.buffered_samples >= 8000 {
                    self.buffered_samples %= 8000;
                    self.update(false)?;
                }
                Ok(())
            });
        if let Err(error) = result {
            self.failed(error);
        }
    }

    fn finish(&mut self) {
        let Some(stream) = self.stream else {
            return;
        };
        let handle = self.model.transcriber;
        let result = self
            .model
            .api
            .check(unsafe { (self.model.api.stop)(handle, stream) })
            .and_then(|_| self.update(true));
        if let Err(error) = result {
            self.failed(error);
        }
        self.close_stream();
    }

    fn cancel(&mut self) {
        self.close_stream();
    }
}

impl Drop for Moonshine {
    fn drop(&mut self) {
        self.close_stream();
    }
}

#[derive(Default)]
struct Tail {
    committed_count: usize,
    has_text: bool,
    partial: String,
}

impl Tail {
    fn decorate(&self, text: &str) -> String {
        if self.has_text && !text.is_empty() {
            format!(" {text}")
        } else {
            text.into()
        }
    }

    fn commit(&mut self, index: usize, text: &str) -> EngineEvent {
        let chunk = self.decorate(text);
        self.committed_count = index + 1;
        self.has_text |= !text.is_empty();
        self.partial.clear();
        EngineEvent::Committed(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_completed_line_clears_the_provisional_tail() {
        let mut tail = Tail {
            partial: "wrong".into(),
            ..Tail::default()
        };
        assert_eq!(tail.commit(0, ""), EngineEvent::Committed(String::new()));
        assert!(tail.partial.is_empty());
        assert_eq!(tail.committed_count, 1);
    }

    #[test]
    fn repeated_words_in_distinct_audio_segments_survive() {
        let mut tail = Tail::default();
        assert_eq!(
            tail.commit(0, "no no"),
            EngineEvent::Committed("no no".into())
        );
        assert_eq!(tail.decorate("no"), " no");
        assert_eq!(tail.commit(1, "no"), EngineEvent::Committed(" no".into()));
        assert_eq!(
            tail.commit(2, "  café\t"),
            EngineEvent::Committed("   café\t".into())
        );
    }
}
