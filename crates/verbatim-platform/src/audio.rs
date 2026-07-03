//! Real microphone capture via `cpal`, behind the `AudioCapture` trait
//! (ARCHITECTURE.md 4.1, ENGINEERING.md 2). Gated on the `cpal-audio` feature;
//! the fakes remain the default test seam.
//!
//! Two constraints shape this module:
//!
//! - **No allocation on the audio callback** (ARCHITECTURE.md 6). The cpal data
//!   callback only downmixes to mono and pushes into a pre-allocated lock-free
//!   ring buffer. All growth (the accumulator `Vec`, resampling) happens off the
//!   real-time thread.
//! - **`cpal::Stream` is `!Send`** on CoreAudio, but the runner holds
//!   `Box<dyn AudioCapture>` on a tokio task and so requires `Send + Sync`. The
//!   stream therefore never leaves a dedicated worker thread; `CpalAudioCapture`
//!   only holds a command channel plus shared atomics, which are `Send + Sync`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use verbatim_engines::{AudioBuffer, PIPELINE_SAMPLE_RATE_HZ};

use crate::errors::CaptureError;
use crate::traits::AudioCapture;

/// How long the worker parks between ring-buffer drains while a capture is live.
/// Short enough that the ring (sized for ~2 s) never overruns under scheduling
/// jitter, long enough not to busy-spin.
const DRAIN_INTERVAL: Duration = Duration::from_millis(15);

/// Commands the public handle sends to the worker that owns the `cpal::Stream`.
enum Command {
    Start(SyncSender<Result<(), CaptureError>>),
    Stop(SyncSender<Result<AudioBuffer, CaptureError>>),
    Abort,
    Shutdown,
}

/// A `cpal`-backed [`AudioCapture`]. Cheap to construct; a worker thread is
/// spawned once and reused across recordings.
pub struct CpalAudioCapture {
    commands: Sender<Command>,
    capturing: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Default for CpalAudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl CpalAudioCapture {
    pub fn new() -> Self {
        let capturing = Arc::new(AtomicBool::new(false));
        let (commands, rx) = mpsc::channel();
        let worker_capturing = Arc::clone(&capturing);
        let worker = thread::Builder::new()
            .name("verbatim-audio".to_owned())
            .spawn(move || worker_loop(rx, worker_capturing))
            .ok();
        Self {
            commands,
            capturing,
            worker,
        }
    }

    /// Round-trip a command that expects a reply, mapping a dead worker to a
    /// backend error rather than panicking.
    fn request<T>(
        &self,
        make: impl FnOnce(SyncSender<Result<T, CaptureError>>) -> Command,
    ) -> Result<T, CaptureError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.commands
            .send(make(reply))
            .map_err(|_| CaptureError::Backend("audio worker is gone".to_owned()))?;
        answer
            .recv()
            .map_err(|_| CaptureError::Backend("audio worker did not answer".to_owned()))?
    }
}

impl Drop for CpalAudioCapture {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AudioCapture for CpalAudioCapture {
    fn start(&mut self) -> Result<(), CaptureError> {
        self.request(Command::Start)
    }

    fn stop(&mut self) -> Result<AudioBuffer, CaptureError> {
        if !self.capturing.load(Ordering::SeqCst) {
            return Err(CaptureError::NotCapturing);
        }
        self.request(Command::Stop)
    }

    fn abort(&mut self) {
        self.capturing.store(false, Ordering::SeqCst);
        let _ = self.commands.send(Command::Abort);
    }

    fn is_capturing(&self) -> bool {
        self.capturing.load(Ordering::SeqCst)
    }
}

/// A live recording owned entirely by the worker thread.
struct Active {
    // Dropped (which stops capture) explicitly in `finish`; held so the stream
    // stays alive for the duration of the recording.
    stream: Stream,
    consumer: HeapCons<f32>,
    native_rate: u32,
    device_lost: Arc<AtomicBool>,
    /// Native-rate mono samples drained off the ring so far.
    accumulator: Vec<f32>,
}

impl Active {
    /// Move everything the callback has produced into the accumulator. Runs on
    /// the worker thread, never the audio callback, so growth is safe here.
    fn drain(&mut self) {
        let mut scratch = [0.0f32; 4096];
        loop {
            let n = self.consumer.pop_slice(&mut scratch);
            if n == 0 {
                break;
            }
            self.accumulator.extend_from_slice(&scratch[..n]);
        }
    }

    /// Stop the stream, drain the tail, and resample to the pipeline rate. A
    /// device that dropped out mid-recording surfaces as `DeviceLost` (E6).
    fn finish(mut self) -> Result<AudioBuffer, CaptureError> {
        // Stopping first guarantees the callback has quiesced before the final
        // drain, so no samples are stranded in the ring.
        let _ = self.stream.pause();
        drop(self.stream);
        loop {
            let mut scratch = [0.0f32; 4096];
            let n = self.consumer.pop_slice(&mut scratch);
            if n == 0 {
                break;
            }
            self.accumulator.extend_from_slice(&scratch[..n]);
        }
        if self.device_lost.load(Ordering::SeqCst) {
            return Err(CaptureError::DeviceLost);
        }
        let samples = resample_to_pipeline_rate(&self.accumulator, self.native_rate)
            .map_err(CaptureError::Backend)?;
        Ok(AudioBuffer {
            samples,
            sample_rate_hz: PIPELINE_SAMPLE_RATE_HZ,
        })
    }
}

fn worker_loop(commands: Receiver<Command>, capturing: Arc<AtomicBool>) {
    let mut active: Option<Active> = None;
    loop {
        match active.take() {
            Some(mut recording) => {
                recording.drain();
                match commands.recv_timeout(DRAIN_INTERVAL) {
                    Ok(Command::Stop(reply)) => {
                        capturing.store(false, Ordering::SeqCst);
                        let _ = reply.send(recording.finish());
                    }
                    Ok(Command::Abort) => {
                        capturing.store(false, Ordering::SeqCst);
                        // `recording` dropped here: stream stops, buffer discarded.
                    }
                    Ok(Command::Start(reply)) => {
                        let _ =
                            reply.send(Err(CaptureError::Backend("already capturing".to_owned())));
                        active = Some(recording);
                    }
                    Ok(Command::Shutdown) => break,
                    Err(RecvTimeoutError::Timeout) => active = Some(recording),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match commands.recv() {
                Ok(Command::Start(reply)) => match start_recording() {
                    Ok(recording) => {
                        capturing.store(true, Ordering::SeqCst);
                        active = Some(recording);
                        let _ = reply.send(Ok(()));
                    }
                    Err(err) => {
                        let _ = reply.send(Err(err));
                    }
                },
                Ok(Command::Stop(reply)) => {
                    let _ = reply.send(Err(CaptureError::NotCapturing));
                }
                Ok(Command::Abort) => {}
                Ok(Command::Shutdown) | Err(_) => break,
            },
        }
    }
}

fn start_recording() -> Result<Active, CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
    let supported = device
        .default_input_config()
        .map_err(|err| CaptureError::Backend(err.to_string()))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = config.channels as usize;
    let native_rate = config.sample_rate.0;

    let device_lost = Arc::new(AtomicBool::new(false));
    // Ring holds ~2 s of mono audio: ample headroom over the drain interval.
    let capacity = (native_rate as usize)
        .saturating_mul(2)
        .max(native_rate as usize);
    let ring = HeapRb::<f32>::new(capacity);
    let (producer, consumer) = ring.split();

    let stream = build_stream(
        &device,
        &config,
        sample_format,
        channels,
        producer,
        Arc::clone(&device_lost),
    )?;
    stream
        .play()
        .map_err(|err| CaptureError::Backend(err.to_string()))?;

    Ok(Active {
        stream,
        consumer,
        native_rate,
        device_lost,
        accumulator: Vec::new(),
    })
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    producer: ringbuf::HeapProd<f32>,
    device_lost: Arc<AtomicBool>,
) -> Result<Stream, CaptureError> {
    let on_error = move |err| {
        tracing::warn!(?err, "audio input stream error");
        device_lost.store(true, Ordering::SeqCst);
    };

    let result = match sample_format {
        SampleFormat::F32 => {
            let mut producer = producer;
            device.build_input_stream(
                config,
                move |data: &[f32], _: &_| push_mono(&mut producer, data, channels, |s| s),
                on_error,
                None,
            )
        }
        SampleFormat::I16 => {
            let mut producer = producer;
            device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    push_mono(&mut producer, data, channels, |s| {
                        f32::from(s) / f32::from(i16::MAX)
                    })
                },
                on_error,
                None,
            )
        }
        SampleFormat::U16 => {
            let mut producer = producer;
            device.build_input_stream(
                config,
                move |data: &[u16], _: &_| {
                    push_mono(&mut producer, data, channels, |s| {
                        (f32::from(s) / f32::from(u16::MAX)) * 2.0 - 1.0
                    })
                },
                on_error,
                None,
            )
        }
        other => {
            return Err(CaptureError::Backend(format!(
                "unsupported sample format: {other:?}"
            )));
        }
    };

    result.map_err(|err| match err {
        cpal::BuildStreamError::DeviceNotAvailable => CaptureError::NoDevice,
        other => CaptureError::Backend(other.to_string()),
    })
}

/// Downmix interleaved frames to mono and push into the ring. Allocation-free:
/// `chunks_exact` borrows, and a full ring simply drops samples rather than
/// blocking the real-time callback.
fn push_mono<T: Copy>(
    producer: &mut ringbuf::HeapProd<f32>,
    data: &[T],
    channels: usize,
    to_f32: impl Fn(T) -> f32,
) {
    if channels == 0 {
        return;
    }
    for frame in data.chunks_exact(channels) {
        let mut sum = 0.0f32;
        for &sample in frame {
            sum += to_f32(sample);
        }
        let _ = producer.try_push(sum / channels as f32);
    }
}

/// Resample native-rate mono audio to the 16 kHz pipeline rate with an
/// anti-aliased sinc resampler (rubato). Off the real-time thread.
fn resample_to_pipeline_rate(input: &[f32], in_rate: u32) -> Result<Vec<f32>, String> {
    let out_rate = PIPELINE_SAMPLE_RATE_HZ;
    if in_rate == 0 {
        return Err("input sample rate is zero".to_owned());
    }
    if in_rate == out_rate {
        return Ok(input.to_vec());
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let ratio = f64::from(out_rate) / f64::from(in_rate);
    const CHUNK: usize = 1024;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, CHUNK, 1)
        .map_err(|err| format!("resampler init failed: {err}"))?;

    let expected = (input.len() as f64 * ratio).round() as usize;
    let mut output = Vec::with_capacity(expected + CHUNK);
    let mut offset = 0;
    while offset + CHUNK <= input.len() {
        let resampled = resampler
            .process(&[&input[offset..offset + CHUNK]], None)
            .map_err(|err| format!("resample failed: {err}"))?;
        if let Some(channel) = resampled.into_iter().next() {
            output.extend_from_slice(&channel);
        }
        offset += CHUNK;
    }
    if offset < input.len() {
        let resampled = resampler
            .process_partial(Some(&[&input[offset..]]), None)
            .map_err(|err| format!("resample partial failed: {err}"))?;
        if let Some(channel) = resampled.into_iter().next() {
            output.extend_from_slice(&channel);
        }
    }
    let flushed = resampler
        .process_partial(None::<&[Vec<f32>]>, None)
        .map_err(|err| format!("resample flush failed: {err}"))?;
    if let Some(channel) = flushed.into_iter().next() {
        output.extend_from_slice(&channel);
    }
    output.truncate(expected + resampler.output_delay());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn passthrough_when_already_at_pipeline_rate() {
        let input = vec![0.1, -0.2, 0.3, -0.4];
        assert_eq!(
            resample_to_pipeline_rate(&input, PIPELINE_SAMPLE_RATE_HZ).unwrap(),
            input
        );
    }

    #[test]
    fn empty_input_resamples_to_empty() {
        assert!(resample_to_pipeline_rate(&[], 48_000).unwrap().is_empty());
    }

    #[test]
    fn downsamples_48k_to_16k_preserving_a_tone() {
        // One second of a 440 Hz sine at 48 kHz -> ~16 kHz, one third the length.
        let in_rate = 48_000u32;
        let input: Vec<f32> = (0..in_rate)
            .map(|i| (TAU * 440.0 * i as f32 / in_rate as f32).sin())
            .collect();

        let output = resample_to_pipeline_rate(&input, in_rate).unwrap();

        // Length tracks the 1/3 ratio plus resampler output delay.
        assert!(
            (16_030..=16_050).contains(&output.len()),
            "unexpected length {}",
            output.len()
        );
        // Bounded, and the tone is still present (not silenced to zeros).
        assert!(output.iter().all(|s| s.abs() <= 1.5));
        assert!(output.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn push_mono_averages_channels_without_allocating_growth() {
        let ring = HeapRb::<f32>::new(8);
        let (mut producer, mut consumer) = ring.split();
        // Two stereo frames: (1.0, 0.0) -> 0.5, (0.2, 0.4) -> 0.3.
        push_mono(&mut producer, &[1.0f32, 0.0, 0.2, 0.4], 2, |s| s);

        let mut out = [0.0f32; 4];
        let n = consumer.pop_slice(&mut out);
        assert_eq!(n, 2);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn zero_input_rate_returns_error() {
        let result = resample_to_pipeline_rate(&[0.1, -0.2], 0);
        assert!(result.is_err());
    }

    #[test]
    fn short_input_less_than_one_chunk_resamples_correctly() {
        let in_rate = 48_000u32;
        let input: Vec<f32> = (0..100)
            .map(|i| (TAU * 440.0 * i as f32 / in_rate as f32).sin())
            .collect();
        let output = resample_to_pipeline_rate(&input, in_rate).unwrap();
        assert!(output.len() < 100, "downsampled output shorter than input");
        assert!(!output.is_empty());
        assert!(output.iter().all(|s| s.abs() <= 1.5));
    }

}
