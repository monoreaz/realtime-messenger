//! Audio I/O for calls. The engine always sees 48 kHz mono: microphone audio arrives as
//! 20 ms `i16` frames, and decoded speech is pushed into a playback buffer. Device
//! sample rates and channel counts are converted with a small linear resampler.
//!
//! On Windows the default WASAPI devices are used via cpal; the cpal streams live on
//! their own thread (they are not `Send` on every backend) until the call ends.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::codec::{FRAME_SAMPLES, SAMPLE_RATE};

/// At most this much decoded audio is buffered for playback (keeps latency bounded).
const MAX_PLAYBACK: usize = SAMPLE_RATE as usize / 4;

/// 48 kHz mono samples waiting to be played.
#[derive(Clone, Default)]
pub struct Playback(Arc<Mutex<VecDeque<f32>>>);

impl Playback {
    pub fn push(&self, pcm: &[i16]) {
        let mut buffer = self.0.lock().unwrap();
        buffer.extend(pcm.iter().map(|sample| *sample as f32 / 32768.0));
        let excess = buffer.len().saturating_sub(MAX_PLAYBACK);
        buffer.drain(..excess);
    }

    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    fn pop(&self) -> f32 {
        self.0.lock().unwrap().pop_front().unwrap_or(0.0)
    }

    #[cfg(test)]
    pub fn take_all(&self) -> Vec<f32> {
        self.0.lock().unwrap().drain(..).collect()
    }
}

/// Microphone frames + speaker buffer for one call. Dropping it stops the devices.
pub struct Audio {
    pub capture: mpsc::Receiver<Vec<i16>>,
    pub playback: Playback,
    _stop: StopGuard,
}

struct StopGuard(Option<std::sync::mpsc::Sender<()>>, Option<tokio::task::JoinHandle<()>>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.take();
        if let Some(task) = self.1.take() {
            task.abort();
        }
    }
}

/// Streaming linear resampler (mono).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub struct Resampler {
    step: f64,
    position: f64,
    previous: f32,
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, position: 0.0, previous: 0.0 }
    }

    /// Converts `input` (at the source rate) and appends to `output` (target rate).
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if (self.step - 1.0).abs() < f64::EPSILON {
            output.extend_from_slice(input);
            self.previous = *input.last().unwrap();
            return;
        }
        // `position` is relative to `previous` (index -1) … input[len-1].
        while self.position < input.len() as f64 {
            let index = self.position.floor();
            let fraction = (self.position - index) as f32;
            let left = if index < 1.0 { self.previous } else { input[index as usize - 1] };
            let right = input[(index as usize).min(input.len() - 1)];
            output.push(left + (right - left) * fraction);
            self.position += self.step;
        }
        self.position -= input.len() as f64;
        self.previous = *input.last().unwrap();
    }
}

/// Collects 48 kHz mono samples into 20 ms `i16` frames.
struct Framer {
    pending: Vec<f32>,
    muted: Arc<AtomicBool>,
    sender: mpsc::Sender<Vec<i16>>,
}

impl Framer {
    fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= FRAME_SAMPLES {
            let muted = self.muted.load(Ordering::Relaxed);
            let frame: Vec<i16> = self
                .pending
                .drain(..FRAME_SAMPLES)
                .map(|sample| if muted { 0 } else { (sample.clamp(-1.0, 1.0) * 32767.0) as i16 })
                .collect();
            // Drop frames rather than block the audio thread if the engine lags.
            let _ = self.sender.try_send(frame);
        }
    }
}

/// A test/development source: a sine tone (or silence when `frequency` is 0) paced in real time.
pub fn synthetic(frequency: f32, muted: Arc<AtomicBool>) -> Audio {
    let (sender, capture) = mpsc::channel(50);
    let task = tokio::spawn(async move {
        let mut framer = Framer { pending: Vec::new(), muted, sender };
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(20));
        let mut n = 0u64;
        loop {
            interval.tick().await;
            let samples: Vec<f32> = (0..FRAME_SAMPLES)
                .map(|_| {
                    n += 1;
                    if frequency == 0.0 { 0.0 } else { (n as f32 / SAMPLE_RATE as f32 * frequency * std::f32::consts::TAU).sin() * 0.3 }
                })
                .collect();
            framer.push(&samples);
            if framer.sender.is_closed() {
                break;
            }
        }
    });
    Audio { capture, playback: Playback::default(), _stop: StopGuard(None, Some(task)) }
}

/// Opens the default microphone and speakers.
#[cfg(windows)]
pub fn open(muted: Arc<AtomicBool>) -> Result<Audio, String> {
    let (sender, capture) = mpsc::channel(50);
    let playback = Playback::default();
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread_playback = playback.clone();
    std::thread::Builder::new()
        .name("call-audio".into())
        .spawn(move || {
            let streams = devices::start(Framer { pending: Vec::new(), muted, sender }, thread_playback);
            match streams {
                Ok(streams) => {
                    let _ = ready_tx.send(Ok(()));
                    // Blocks until the call ends (sender dropped), keeping the streams alive.
                    let _ = stopped.recv();
                    drop(streams);
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        })
        .map_err(|error| error.to_string())?;
    ready_rx.recv().map_err(|_| "Audio thread failed".to_owned())??;
    Ok(Audio { capture, playback, _stop: StopGuard(Some(stop), None) })
}

/// Development builds on other platforms have no audio devices: send silence.
#[cfg(not(windows))]
pub fn open(muted: Arc<AtomicBool>) -> Result<Audio, String> {
    log::warn!("call audio devices are only implemented on Windows; sending silence");
    Ok(synthetic(0.0, muted))
}

#[cfg(windows)]
mod devices {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};

    use super::{Framer, Playback, Resampler, SAMPLE_RATE};

    pub fn start(framer: Framer, playback: Playback) -> Result<(Stream, Stream), String> {
        let host = cpal::default_host();
        let input = host.default_input_device().ok_or("No microphone found. Connect one and try again.")?;
        let output = host.default_output_device().ok_or("No speakers or headphones found.")?;
        let input_config = input.default_input_config().map_err(|error| format!("Microphone unavailable: {error}"))?;
        let output_config = output.default_output_config().map_err(|error| format!("Speakers unavailable: {error}"))?;

        let input_stream = match input_config.sample_format() {
            SampleFormat::F32 => input_stream::<f32>(&input, input_config.config(), framer),
            SampleFormat::I16 => input_stream::<i16>(&input, input_config.config(), framer),
            SampleFormat::U16 => input_stream::<u16>(&input, input_config.config(), framer),
            SampleFormat::I32 => input_stream::<i32>(&input, input_config.config(), framer),
            other => Err(format!("Unsupported microphone format {other:?}")),
        }?;
        let output_stream = match output_config.sample_format() {
            SampleFormat::F32 => output_stream::<f32>(&output, output_config.config(), playback),
            SampleFormat::I16 => output_stream::<i16>(&output, output_config.config(), playback),
            SampleFormat::U16 => output_stream::<u16>(&output, output_config.config(), playback),
            SampleFormat::I32 => output_stream::<i32>(&output, output_config.config(), playback),
            other => Err(format!("Unsupported speaker format {other:?}")),
        }?;
        input_stream.play().map_err(|error| format!("Microphone could not start: {error}"))?;
        output_stream.play().map_err(|error| format!("Speakers could not start: {error}"))?;
        Ok((input_stream, output_stream))
    }

    fn input_stream<T>(device: &cpal::Device, config: StreamConfig, mut framer: Framer) -> Result<Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let channels = config.channels.max(1) as usize;
        let mut resampler = Resampler::new(config.sample_rate, SAMPLE_RATE);
        let mut mono = Vec::new();
        let mut converted = Vec::new();
        device
            .build_input_stream::<T, _, _>(
                config,
                move |data: &[T], _| {
                    mono.clear();
                    mono.extend(data.chunks(channels).map(|frame| frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32));
                    converted.clear();
                    resampler.process(&mono, &mut converted);
                    framer.push(&converted);
                },
                |error| log::warn!("microphone stream error: {error}"),
                None,
            )
            .map_err(|error| format!("Microphone could not be opened: {error}"))
    }

    fn output_stream<T>(device: &cpal::Device, config: StreamConfig, playback: Playback) -> Result<Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = config.channels.max(1) as usize;
        let mut resampler = Resampler::new(SAMPLE_RATE, config.sample_rate);
        let mut source = Vec::new();
        let mut resampled = Vec::new();
        device
            .build_output_stream::<T, _, _>(
                config,
                move |data: &mut [T], _| {
                    let frames = data.len() / channels;
                    // Pull enough 48 kHz samples to produce `frames` device samples.
                    while resampled.len() < frames {
                        source.clear();
                        source.extend((0..240).map(|_| playback.pop()));
                        resampler.process(&source, &mut resampled);
                    }
                    for (frame, sample) in data.chunks_mut(channels).zip(resampled.drain(..frames)) {
                        frame.fill(T::from_sample(sample));
                    }
                },
                |error| log::warn!("speaker stream error: {error}"),
                None,
            )
            .map_err(|error| format!("Speakers could not be opened: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_preserves_duration() {
        let mut down = Resampler::new(44_100, 48_000);
        let mut output = Vec::new();
        for _ in 0..100 {
            down.process(&vec![0.5f32; 441], &mut output);
        }
        // 1 s of 44.1 kHz → ~1 s of 48 kHz.
        assert!((output.len() as i64 - 48_000).abs() <= 2, "{}", output.len());
        // The first samples ramp up from the initial (silent) history sample.
        assert!(output.iter().skip(2).all(|s| (*s - 0.5).abs() < 1e-6));
        let mut same = Resampler::new(48_000, 48_000);
        let mut copy = Vec::new();
        same.process(&[0.1, 0.2], &mut copy);
        assert_eq!(copy, vec![0.1, 0.2]);
    }

    #[test]
    fn playback_buffer_is_bounded() {
        let playback = Playback::default();
        playback.push(&vec![1000i16; MAX_PLAYBACK + 500]);
        assert_eq!(playback.take_all().len(), MAX_PLAYBACK);
        assert_eq!(playback.pop(), 0.0);
    }
}
