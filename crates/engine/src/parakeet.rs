use std::ffi::{CStr, CString, c_char, c_void};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};
use said_core::{EngineEvent, EngineOutput, Result, UtteranceId};
use serde::Deserialize;

use crate::{Engine, native::Library, sherpa_abi::OfflineRecognizerConfig};

const RATE: usize = 16_000;
const CHUNK: usize = 30 * RATE;
const STRIDE: usize = 25 * RATE;

type Handle = *const c_void;
type Unary = unsafe extern "C" fn(Handle);

struct Api {
    create: unsafe extern "C" fn(*const OfflineRecognizerConfig) -> Handle,
    destroy: Unary,
    stream: unsafe extern "C" fn(Handle) -> Handle,
    destroy_stream: Unary,
    accept: unsafe extern "C" fn(Handle, i32, *const f32, i32),
    decode: unsafe extern "C" fn(Handle, Handle),
    result: unsafe extern "C" fn(Handle) -> *const c_char,
    destroy_result: unsafe extern "C" fn(*const c_char),
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
        let version = symbol!(
            c"SherpaOnnxGetVersionStr",
            unsafe extern "C" fn() -> *const c_char
        );
        if unsafe { CStr::from_ptr(version()) }.to_bytes() != b"1.13.8" {
            return Err("Parakeet requires sherpa-onnx 1.13.8".into());
        }
        Ok(Self {
            create: symbol!(
                c"SherpaOnnxCreateOfflineRecognizer",
                unsafe extern "C" fn(*const OfflineRecognizerConfig) -> Handle
            ),
            destroy: symbol!(c"SherpaOnnxDestroyOfflineRecognizer", Unary),
            stream: symbol!(
                c"SherpaOnnxCreateOfflineStream",
                unsafe extern "C" fn(Handle) -> Handle
            ),
            destroy_stream: symbol!(c"SherpaOnnxDestroyOfflineStream", Unary),
            accept: symbol!(
                c"SherpaOnnxAcceptWaveformOffline",
                unsafe extern "C" fn(Handle, i32, *const f32, i32)
            ),
            decode: symbol!(
                c"SherpaOnnxDecodeOfflineStream",
                unsafe extern "C" fn(Handle, Handle)
            ),
            result: symbol!(
                c"SherpaOnnxGetOfflineStreamResultAsJson",
                unsafe extern "C" fn(Handle) -> *const c_char
            ),
            destroy_result: symbol!(
                c"SherpaOnnxDestroyOfflineStreamResultJson",
                unsafe extern "C" fn(*const c_char)
            ),
            _library: library,
        })
    }
}

pub struct ParakeetModel {
    api: Api,
    recognizer: Handle,
    partial_interval: Duration,
}

impl ParakeetModel {
    pub fn load(
        library: &Path,
        directory: &Path,
        threads: i32,
        provider: &str,
        update_samples: usize,
    ) -> Result<Rc<Self>> {
        use std::os::unix::ffi::OsStrExt;
        if threads < 1 || update_samples == 0 {
            return Err("Parakeet threads and update_samples must be positive".into());
        }
        let api = Api::open(library)?;
        let paths = ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"]
            .map(|name| CString::new(directory.join(name).as_os_str().as_bytes()))
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !matches!(provider, "cpu" | "coreml")
            || (provider == "coreml" && !cfg!(target_os = "macos"))
        {
            return Err("Parakeet provider must be cpu or coreml".into());
        }
        let options = directory.join(format!(".sherpa-{}.conf", UtteranceId::random().0));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&options)?;
        let loaded: Result<Handle> = (|| {
            file.write_all(b"EnableCpuMemArena=0\nEnableMemPattern=0\nSessionConfig.session.intra_op.allow_spinning=0\nSessionConfig.session.inter_op.allow_spinning=0\n")?;
            let provider = CString::new(format!(
                "{provider}:{}",
                options.to_str().ok_or("model directory must be UTF-8")?
            ))?;
            let mut config: OfflineRecognizerConfig = unsafe { std::mem::zeroed() };
            config.feat_config.sample_rate = RATE as i32;
            config.feat_config.feature_dim = 128;
            config.model_config.transducer.encoder = paths[0].as_ptr();
            config.model_config.transducer.decoder = paths[1].as_ptr();
            config.model_config.transducer.joiner = paths[2].as_ptr();
            config.model_config.tokens = paths[3].as_ptr();
            config.model_config.num_threads = threads;
            config.model_config.provider = provider.as_ptr();
            config.model_config.model_type = c"nemo_transducer".as_ptr();
            config.decoding_method = c"greedy_search".as_ptr();
            let recognizer = unsafe { (api.create)(&config) };
            if recognizer.is_null() {
                return Err("could not load Parakeet model".into());
            }
            Ok(recognizer)
        })();
        let _ = std::fs::remove_file(options);
        let recognizer = loaded?;
        Ok(Rc::new(Self {
            api,
            recognizer,
            partial_interval: Duration::from_secs_f64(update_samples as f64 / RATE as f64),
        }))
    }

    fn decode(&self, audio: &[i16], offset: usize) -> Result<Vec<Word>> {
        if audio.is_empty() {
            return Ok(Vec::new());
        }
        let stream = unsafe { (self.api.stream)(self.recognizer) };
        if stream.is_null() {
            return Err("could not create Parakeet stream".into());
        }
        let samples: Vec<f32> = audio.iter().map(|&s| f32::from(s) / 32768.0).collect();
        unsafe {
            (self.api.accept)(stream, RATE as i32, samples.as_ptr(), samples.len() as i32);
            (self.api.decode)(self.recognizer, stream);
        }
        let json = unsafe { (self.api.result)(stream) };
        let result = if json.is_null() {
            Err("Parakeet returned no result".into())
        } else {
            let result = (|| {
                let decoded: Decoded =
                    serde_json::from_slice(unsafe { CStr::from_ptr(json) }.to_bytes())?;
                decoded.words(offset, audio.len())
            })();
            unsafe { (self.api.destroy_result)(json) };
            result
        };
        unsafe { (self.api.destroy_stream)(stream) };
        result
    }
}

impl Drop for ParakeetModel {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.recognizer) };
    }
}

#[derive(Deserialize)]
struct Decoded {
    text: String,
    tokens: Vec<String>,
    timestamps: Vec<f64>,
    durations: Vec<f64>,
}

#[derive(Debug, Clone)]
struct Word {
    text: String,
    start: usize,
    end: usize,
}

impl Decoded {
    fn words(self, offset: usize, samples: usize) -> Result<Vec<Word>> {
        if self.tokens.len() != self.timestamps.len() || self.tokens.len() != self.durations.len() {
            return Err("Parakeet returned inconsistent token timestamps".into());
        }
        let mut words: Vec<Word> = Vec::new();
        let mut boundaries = Vec::new();
        let mut bytes = Vec::new();
        let mut previous = 0;
        for ((token, time), duration) in self.tokens.iter().zip(self.timestamps).zip(self.durations)
        {
            if !time.is_finite() || !duration.is_finite() || time < 0.0 || duration < 0.0 {
                return Err("Parakeet returned invalid token time".into());
            }
            let start = (time * RATE as f64).round() as usize;
            let end = ((time + duration) * RATE as f64).round() as usize;
            if start < previous || start > samples + RATE || end > samples + RATE {
                return Err("Parakeet returned unordered or out-of-range token time".into());
            }
            previous = start;
            let piece = if token.starts_with("<0x") && token.ends_with('>') {
                vec![u8::from_str_radix(&token[3..token.len() - 1], 16)?]
            } else {
                token.as_bytes().to_vec()
            };
            if piece.first() == Some(&b' ') || words.is_empty() {
                boundaries.push(bytes.len());
                words.push(Word {
                    text: String::new(),
                    start: offset + start,
                    end: offset + end.max(start + 1),
                });
            }
            words.last_mut().unwrap().end =
                words.last().unwrap().end.max(offset + end.max(start + 1));
            bytes.extend_from_slice(&piece);
        }
        let raw = String::from_utf8(bytes)?;
        if raw.trim_start_matches(' ') != self.text {
            return Err("Parakeet token text differs from its raw transcript".into());
        }
        boundaries.push(raw.len());
        for (word, span) in words.iter_mut().zip(boundaries.windows(2)) {
            word.text = raw
                .get(span[0]..span[1])
                .ok_or("Parakeet split a Unicode word")?
                .to_owned();
        }
        Ok(words)
    }
}

fn text(words: &[Word], first: bool) -> String {
    let mut result = String::new();
    for word in words {
        if (!result.is_empty() || !first)
            && !word.text.starts_with(char::is_whitespace)
            && !result.ends_with(char::is_whitespace)
        {
            result.push(' ');
        }
        result.push_str(&word.text);
    }
    if first {
        result.trim_start_matches(' ').to_owned()
    } else {
        result
    }
}

fn merge(previous: &[Word], next: &[Word], offset: usize) -> Vec<Word> {
    if previous.is_empty() {
        return next.to_vec();
    }
    if next.is_empty() {
        return previous.to_vec();
    }
    let count = next
        .iter()
        .take_while(|word| word.start <= offset + 5 * RATE + RATE * 80 / 1000)
        .count();
    if count == 0 {
        return previous.iter().chain(next).cloned().collect();
    }
    #[derive(Clone, Copy, Default)]
    enum Step {
        #[default]
        Previous,
        Next,
        Matched,
    }
    #[derive(Clone, Copy, Default)]
    struct Cell {
        matches: usize,
        error: usize,
        step: Step,
    }
    let width = count + 1;
    let mut cells = vec![Cell::default(); (previous.len() + 1) * width];
    for cell in &mut cells[1..width] {
        cell.step = Step::Next;
    }
    let rank = |cell: Cell| (cell.matches, std::cmp::Reverse(cell.error));
    // NeMo TDT timestamps have an 80 ms encoder-frame resolution.
    let frame = RATE * 80 / 1000;
    for (i, old) in previous.iter().enumerate() {
        for (j, new) in next[..count].iter().enumerate() {
            let mut chosen = Cell {
                step: Step::Previous,
                ..cells[i * width + j + 1]
            };
            let left = Cell {
                step: Step::Next,
                ..cells[(i + 1) * width + j]
            };
            if rank(left) > rank(chosen) {
                chosen = left;
            }
            if old.start <= new.end.saturating_add(frame)
                && new.start <= old.end.saturating_add(frame)
            {
                let diagonal = cells[i * width + j];
                let matched = Cell {
                    matches: diagonal.matches + 1,
                    error: diagonal.error
                        + old.start.abs_diff(new.start)
                        + old.end.abs_diff(new.end),
                    step: Step::Matched,
                };
                if rank(matched) >= rank(chosen) {
                    chosen = matched;
                }
            }
            cells[(i + 1) * width + j + 1] = chosen;
        }
    }
    let (mut i, mut j) = (previous.len(), count);
    let mut pairs = Vec::new();
    while i > 0 || j > 0 {
        match cells[i * width + j].step {
            Step::Matched => {
                i -= 1;
                j -= 1;
                pairs.push((i, j));
            }
            Step::Previous => i -= 1,
            Step::Next => j -= 1,
        }
    }
    let middle = offset + 5 * RATE / 2;
    let boundary = pairs.into_iter().min_by_key(|&(old, new)| {
        let time = previous[old].start.min(next[new].start)
            + next[new].start.abs_diff(previous[old].start) / 2;
        (
            time.abs_diff(middle),
            previous[old].start.abs_diff(next[new].start),
        )
    });
    let Some((old, new)) = boundary else {
        let mut words: Vec<_> = previous.iter().chain(next).cloned().collect();
        words.sort_by_key(|word| word.start);
        return words;
    };
    previous[..=old]
        .iter()
        .chain(&next[new + 1..])
        .cloned()
        .collect()
}

pub struct Parakeet {
    model: Rc<ParakeetModel>,
    id: UtteranceId,
    sender: Sender<EngineOutput>,
    audio: Vec<i16>,
    offset: usize,
    pending: Vec<Word>,
    first: bool,
    active: bool,
    started: bool,
    next_partial: Instant,
    decoded_samples: usize,
}

impl Parakeet {
    pub fn from_model(id: UtteranceId, model: Rc<ParakeetModel>) -> (Self, Receiver<EngineOutput>) {
        let (sender, output) = bounded(64);
        (
            Self {
                model,
                id,
                sender,
                audio: Vec::with_capacity(CHUNK),
                offset: 0,
                pending: Vec::new(),
                first: true,
                active: false,
                started: false,
                next_partial: Instant::now(),
                decoded_samples: 0,
            },
            output,
        )
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.sender.send(EngineOutput { id: self.id, event });
    }

    fn words(&self) -> Result<Vec<Word>> {
        let decoded = self.model.decode(&self.audio, self.offset)?;
        Ok(if self.offset == 0 {
            decoded
        } else {
            merge(&self.pending, &decoded, self.offset)
        })
    }

    fn close_chunk(&mut self) -> Result<()> {
        let words = self.words()?;
        let boundary = self.offset + STRIDE;
        let count = words.iter().take_while(|w| w.end <= boundary).count();
        let committed = text(&words[..count], self.first);
        if !committed.is_empty() {
            self.emit(EngineEvent::Committed(committed));
            self.first = false;
        }
        self.pending = words[count..].to_vec();
        self.audio.drain(..STRIDE);
        self.offset += STRIDE;
        self.decoded_samples = self.audio.len();
        self.emit(EngineEvent::Partial(text(&self.pending, self.first)));
        self.next_partial = Instant::now() + self.model.partial_interval;
        Ok(())
    }

    fn consume(&mut self, mut samples: &[i16]) -> Result<()> {
        while !samples.is_empty() {
            if self.audio.len() == CHUNK {
                self.close_chunk()?;
            }
            let count = samples.len().min(CHUNK - self.audio.len());
            self.audio.extend_from_slice(&samples[..count]);
            samples = &samples[count..];
        }
        if self.audio.len() >= RATE / 2
            && self.audio.len() > self.decoded_samples
            && Instant::now() >= self.next_partial
        {
            let partial = text(&self.words()?, self.first);
            let interval = if partial.is_empty() {
                Duration::from_millis(500)
            } else {
                self.model.partial_interval
            };
            self.emit(EngineEvent::Partial(partial));
            self.decoded_samples = self.audio.len();
            self.next_partial = Instant::now() + interval;
        }
        Ok(())
    }

    fn fail(&mut self, error: impl std::fmt::Display) {
        self.active = false;
        self.emit(EngineEvent::Error(error.to_string()));
    }
}

impl Engine for Parakeet {
    fn start(&mut self, _vocabulary: &[String]) -> Result<()> {
        if self.started {
            return Err("Parakeet instance already started".into());
        }
        self.started = true;
        self.active = true;
        self.next_partial = Instant::now() + Duration::from_millis(500);
        Ok(())
    }

    fn feed(&mut self, samples: &[i16]) {
        if self.active
            && let Err(error) = self.consume(samples)
        {
            self.fail(error);
        }
    }

    fn finish(&mut self) {
        if !self.active {
            return;
        }
        match self.words() {
            Ok(words) => {
                let tail = text(&words, self.first);
                self.emit(EngineEvent::Committed(tail));
                self.emit(EngineEvent::Final);
                self.active = false;
            }
            Err(error) => self.fail(error),
        }
    }

    fn cancel(&mut self) {
        self.active = false;
        self.audio.clear();
        self.pending.clear();
    }
}
