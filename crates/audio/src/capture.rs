use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_channel::{Receiver, Sender, bounded};
use rtrb::{Producer, RingBuffer};
use said_core::{AudioEvent, InputDevice, Result, UtteranceId};

use crate::{
    Preview, Recorder, Resampler,
    pipeline::{self, EVENT_QUEUE, Pipeline},
    transport::{BACKEND_ERROR, CaptureState, DISCONNECTED, copy_input},
    wav::create_audio_dir,
};

pub fn list_devices() -> Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok());
    host.input_devices()?
        .map(|device| {
            let id = device.id()?;
            Ok(InputDevice {
                is_default: default_id.as_ref() == Some(&id),
                id: id.to_string(),
                name: device.description()?.name().to_owned(),
            })
        })
        .collect()
}

fn input_device(id: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    Ok(match id {
        Some(id) => host
            .device_by_id(&id.parse()?)
            .ok_or("selected input device is unavailable")?,
        None => host
            .default_input_device()
            .ok_or("no default input device")?,
    })
}

pub struct CpalRecorder {
    audio_dir: PathBuf,
    active: Option<(Arc<CaptureState>, JoinHandle<Result<()>>)>,
}

impl CpalRecorder {
    pub fn new(audio_dir: impl AsRef<Path>) -> Self {
        Self {
            audio_dir: audio_dir.as_ref().into(),
            active: None,
        }
    }
}

impl Recorder for CpalRecorder {
    fn list_devices(&self) -> Result<Vec<InputDevice>> {
        list_devices()
    }

    fn start(&mut self, id: UtteranceId, device: Option<&str>) -> Result<Receiver<AudioEvent>> {
        if self.active.is_some() {
            return Err("capture already active; stop before starting another utterance".into());
        }
        let device = input_device(device)?;
        let config = device.default_input_config()?;
        let rate = config.sample_rate();
        let channels = config.channels();
        let mut resampler = Resampler::new(rate, channels)?;
        let (producer, mut consumer) = RingBuffer::new(rate as usize * channels as usize * 2);
        create_audio_dir(&self.audio_dir)?;
        let (sender, receiver) = bounded(EVENT_QUEUE);
        let mut pipeline = Pipeline::new(
            id,
            &self.audio_dir.join(format!("{}.wav", id.0)),
            sender.clone(),
        )?;
        let state = Arc::new(CaptureState::default());
        let worker_state = state.clone();
        let (ready_tx, ready_rx) = bounded(1);
        let worker = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let result = (|| {
                    let stream = build_stream(&device, &config, producer, worker_state.clone())?;
                    stream.play()?;
                    let _ = ready_tx.try_send(Ok(()));
                    while !worker_state.stop.load(Ordering::Acquire) {
                        if consumer.slots() > 0 {
                            pipeline::drain(&mut consumer, &mut resampler, &mut pipeline)?;
                        } else {
                            thread::sleep(Duration::from_millis(3));
                        }
                        pipeline.checkpoint()?;
                        if pipeline.receiver_gone {
                            break;
                        }
                    }
                    worker_state.stop.store(true, Ordering::Release);
                    drop(stream);
                    let mut failure = worker_state.failure.load(Ordering::Acquire);
                    if failure == BACKEND_ERROR
                        && matches!(
                            device.default_input_config(),
                            Err(cpal::DefaultStreamConfigError::DeviceNotAvailable)
                        )
                    {
                        failure = DISCONNECTED;
                    }
                    pipeline::finish(consumer, resampler, pipeline, failure)
                })();
                if let Err(error) = &result {
                    let _ = ready_tx.try_send(Err(error.to_string()));
                    report_error(&sender, id, error.to_string());
                }
                result
            })?;
        match ready_rx.recv()? {
            Ok(()) => self.active = Some((state, worker)),
            Err(error) => {
                state.stop.store(true, Ordering::Release);
                let _ = worker.join();
                return Err(error.into());
            }
        }
        Ok(receiver)
    }

    fn stop(&mut self) -> Result<()> {
        let (state, worker) = self.active.take().ok_or("capture is not active")?;
        state.stop.store(true, Ordering::Release);
        worker.join().map_err(|_| "audio capture thread panicked")?
    }
    fn preview(&self, device: Option<&str>) -> Result<Preview> {
        let device = input_device(device)?;
        let config = device.default_input_config()?;
        let window = (config.sample_rate() as usize * config.channels() as usize / 30).max(1);
        let (producer, mut consumer) =
            RingBuffer::new(config.sample_rate() as usize * config.channels() as usize);
        let (sender, levels) = bounded(16);
        let (ready_tx, ready_rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("audio-preview".into())
            .spawn(move || {
                let state = Arc::new(CaptureState::default());
                let started = (|| -> Result<cpal::Stream> {
                    let stream = build_stream(&device, &config, producer, state.clone())?;
                    stream.play()?;
                    Ok(stream)
                })();
                let stream = match started {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let (mut sum, mut count) = (0.0f64, 0);
                while !worker_stop.load(Ordering::Acquire) {
                    if state.stop.load(Ordering::Acquire) {
                        let _ = sender.send(Err("Microphone stopped responding".into()));
                        break;
                    }
                    while let Ok(sample) = consumer.pop() {
                        sum += (sample as f64).powi(2);
                        count += 1;
                        if count == window {
                            let _ = sender.try_send(Ok(crate::level((sum / count as f64).sqrt())));
                            (sum, count) = (0.0, 0);
                        }
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                drop(stream);
            })?;
        match ready_rx.recv()? {
            Ok(()) => Ok(Preview::new(levels, Some((stop, worker)))),
            Err(error) => {
                let _ = worker.join();
                Err(error.into())
            }
        }
    }
}

impl Drop for CpalRecorder {
    fn drop(&mut self) {
        if self.active.is_some() {
            let _ = self.stop();
        }
    }
}

fn report_error(sender: &Sender<AudioEvent>, id: UtteranceId, message: String) {
    let _ = sender.try_send(AudioEvent::Error { id, message });
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    producer: Producer<f32>,
    state: Arc<CaptureState>,
) -> Result<cpal::Stream> {
    match config.sample_format() {
        cpal::SampleFormat::I8 => stream::<i8>(device, config, producer, state),
        cpal::SampleFormat::I16 => stream::<i16>(device, config, producer, state),
        cpal::SampleFormat::I24 => stream::<cpal::I24>(device, config, producer, state),
        cpal::SampleFormat::I32 => stream::<i32>(device, config, producer, state),
        cpal::SampleFormat::I64 => stream::<i64>(device, config, producer, state),
        cpal::SampleFormat::U8 => stream::<u8>(device, config, producer, state),
        cpal::SampleFormat::U16 => stream::<u16>(device, config, producer, state),
        cpal::SampleFormat::U32 => stream::<u32>(device, config, producer, state),
        cpal::SampleFormat::U64 => stream::<u64>(device, config, producer, state),
        cpal::SampleFormat::F32 => stream::<f32>(device, config, producer, state),
        cpal::SampleFormat::F64 => stream::<f64>(device, config, producer, state),
        _ => Err("unsupported microphone sample format".into()),
    }
}

fn stream<T: SizedSample + Copy>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    mut producer: Producer<f32>,
    state: Arc<CaptureState>,
) -> Result<cpal::Stream>
where
    f32: FromSample<T>,
{
    let channels = config.channels() as usize;
    let errors = state.clone();
    Ok(device.build_input_stream(
        &config.clone().into(),
        move |data: &[T], _| {
            copy_input(data, channels, &mut producer, &state, |sample| {
                sample.to_sample::<f32>()
            });
        },
        move |error| {
            errors.fail(match error {
                cpal::StreamError::DeviceNotAvailable => DISCONNECTED,
                _ => BACKEND_ERROR,
            });
        },
        None,
    )?)
}
