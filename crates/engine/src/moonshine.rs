use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;
use std::ptr;
use std::rc::Rc;

use crossbeam_channel::{Receiver, Sender, bounded};
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
        let revision = symbol!(
            c"moonshine_voice_runtime_version",
            unsafe extern "C" fn() -> i32
        );
        if unsafe { revision() } != 2 {
            return Err("Moonshine requires the patched voice runtime; run fetch-model.sh".into());
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
    update_samples: usize,
}

impl MoonshineModel {
    pub fn load(
        library: &Path,
        model: &Path,
        arch: u32,
        update_samples: usize,
    ) -> Result<Rc<Self>> {
        use std::os::unix::ffi::OsStrExt;
        if !matches!(arch, 2 | 4 | 5) {
            return Err("Moonshine requires a published streaming architecture".into());
        }
        if update_samples == 0 {
            return Err("Moonshine update_samples must be positive".into());
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
        Ok(Rc::new(Self {
            api,
            transcriber,
            update_samples,
        }))
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
    buffered: Vec<i16>,
}

impl Moonshine {
    pub fn new(
        id: UtteranceId,
        library: &Path,
        model: &Path,
        arch: u32,
        update_samples: usize,
    ) -> Result<(Self, Receiver<EngineOutput>)> {
        Ok(Self::from_model(
            id,
            MoonshineModel::load(library, model, arch, update_samples)?,
        ))
    }

    pub fn from_model(
        id: UtteranceId,
        model: Rc<MoonshineModel>,
    ) -> (Self, Receiver<EngineOutput>) {
        let (sender, receiver) = bounded(64);
        let buffered = Vec::with_capacity(model.update_samples);
        (
            Self {
                model,
                stream: None,
                started: false,
                id,
                sender,
                tail: Tail::default(),
                buffered,
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
                let event = self.tail.commit(index, text, line.start, line.duration)?;
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

    fn add_buffered_audio(&mut self) -> Result<()> {
        if self.buffered.is_empty() {
            return Ok(());
        }
        let samples = normalize(&self.buffered);
        let stream = self.stream.ok_or("stream not started")?;
        self.model.api.check(unsafe {
            (self.model.api.add)(
                self.model.transcriber,
                stream,
                samples.as_ptr(),
                samples.len() as u64,
                16000,
                0,
            )
        })?;
        self.buffered.clear();
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

    fn feed(&mut self, mut pcm16k: &[i16]) {
        if self.stream.is_none() {
            return;
        }
        while !pcm16k.is_empty() {
            let count = pcm16k
                .len()
                .min(self.model.update_samples - self.buffered.len());
            self.buffered.extend_from_slice(&pcm16k[..count]);
            pcm16k = &pcm16k[count..];
            if self.buffered.len() == self.model.update_samples
                && let Err(error) = self.add_buffered_audio().and_then(|_| self.update(false))
            {
                self.failed(error);
                return;
            }
        }
    }

    fn finish(&mut self) {
        let Some(stream) = self.stream else {
            return;
        };
        let handle = self.model.transcriber;
        let result = self
            .add_buffered_audio()
            .and_then(|_| {
                self.model
                    .api
                    .check(unsafe { (self.model.api.stop)(handle, stream) })
            })
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

fn normalize(samples: &[i16]) -> Vec<f32> {
    let peak = samples
        .iter()
        .map(|sample| f32::from(*sample).abs())
        .fold(0.0_f32, f32::max);
    let gain = if peak == 0.0 {
        1.0
    } else {
        (8192.0 / peak).clamp(1.0, 32.0)
    };
    samples
        .iter()
        .map(|sample| f32::from(*sample) * gain / 32768.0)
        .collect()
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

    fn commit(
        &mut self,
        index: usize,
        text: &str,
        start: f32,
        duration: f32,
    ) -> Result<EngineEvent> {
        if text.chars().all(char::is_whitespace) && duration > 0.0 {
            return Err(format!(
                "Speech detected at {:.3}–{:.3}s, but recognition returned empty text",
                start,
                start + duration
            )
            .into());
        }
        let chunk = self.decorate(text);
        self.committed_count = index + 1;
        self.has_text |= !text.is_empty();
        self.partial.clear();
        Ok(EngineEvent::Committed(chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_input_is_amplified_without_clipping_or_turning_silence_into_audio() {
        assert_eq!(normalize(&[0; 1600]), vec![0.0; 1600]);
        assert_eq!(
            normalize(&[i16::MIN, i16::MAX]),
            vec![-1.0, 32767.0 / 32768.0]
        );
        assert_eq!(normalize(&[256, -128]), vec![0.25, -0.125]);
        assert_eq!(normalize(&[1, -1]), vec![32.0 / 32768.0, -32.0 / 32768.0]);
    }

    #[test]
    fn an_empty_completed_line_clears_the_provisional_tail() {
        let mut tail = Tail {
            partial: "wrong".into(),
            ..Tail::default()
        };
        assert_eq!(
            tail.commit(0, "", 0.0, 0.0).unwrap(),
            EngineEvent::Committed(String::new())
        );
        assert!(tail.partial.is_empty());
        assert_eq!(tail.committed_count, 1);
    }

    #[test]
    fn detected_speech_without_recognized_words_is_an_error_before_commit() {
        let mut tail = Tail::default();
        for text in ["", " \t"] {
            let error = tail.commit(0, text, 1.25, 0.75).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Speech detected at 1.250–2.000s")
            );
            assert_eq!(tail.committed_count, 0);
            assert!(!tail.has_text);
        }
    }

    #[test]
    fn repeated_words_in_distinct_audio_segments_survive() {
        let mut tail = Tail::default();
        assert_eq!(
            tail.commit(0, "no no", 0.0, 1.0).unwrap(),
            EngineEvent::Committed("no no".into())
        );
        assert_eq!(tail.decorate("no"), " no");
        assert_eq!(
            tail.commit(1, "no", 1.0, 1.0).unwrap(),
            EngineEvent::Committed(" no".into())
        );
        assert_eq!(
            tail.commit(2, "  café\t", 2.0, 1.0).unwrap(),
            EngineEvent::Committed("   café\t".into())
        );
    }
}
