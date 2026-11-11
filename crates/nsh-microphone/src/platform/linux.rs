//! Linux: cpal's ALSA host captures at the hardware rate and this layer resamples and writes
//! PCM. Building links ALSA, so `libasound2-dev` must be present (CI installs it); PulseAudio
//! and PipeWire systems expose their devices through ALSA's plugin stack, so every device name
//! below comes through it.
//!
//! ALSA offers no consent prompt to ask, so permissions answer `Granted` (a Partial named in
//! docs/parity.md). `Wav` and `Pcm16bits` answer supported; a file output accumulates in memory
//! and `write_wav` writes it on stop.

use std::{
    future::ready,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use cpal::{
    InputCallbackInfo, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use super::write_wav;
use crate::{
    backend::{Backend, BackendFuture, ChunkSink},
    error::{ErrorKind, RecorderError},
    options::{
        Amplitude, AudioChunk, Encoder, InputDevice, Output, PermissionState, RecordConfig,
        StoppedRecording,
    },
};

pub(crate) struct NativeBackend;

/// Linear rate conversion over interleaved frames: `step` is source frames per output frame and
/// `frac` the spot inside the current interval; both carry across buffers so seams interpolate.
struct Resampler {
    step: f64,
    frac: f64,
    prev: Vec<f32>,
}

impl Resampler {
    fn new(hw_rate: u32, rate: u32) -> Self {
        // A zero rate (never allowed by the recorder) falls back to passthrough, not a division.
        let step = if hw_rate > 0 && rate > 0 {
            f64::from(hw_rate) / f64::from(rate)
        } else {
            1.0
        };
        Self {
            step,
            frac: 0.0,
            prev: Vec::new(),
        }
    }
}

/// What the stream callbacks write and the API reads: samples, meters, the drop flags.
struct Capture {
    sink: ChunkSink,
    keep: bool,
    rate: u32,
    channels: u16,
    hw_channels: u16,
    paused: bool,
    resample: Resampler,
    frame: Vec<f32>,
    samples: Vec<i16>,
    frames: u64,
    average: f32,
    peak: f32,
    failure: Option<RecorderError>,
}

impl Capture {
    fn new(config: &RecordConfig, sink: ChunkSink, hw_rate: u32, hw_channels: u16) -> Self {
        Self {
            sink,
            keep: matches!(config.output, Output::File { .. }),
            rate: config.sample_rate,
            channels: config.channels,
            hw_channels,
            paused: false,
            resample: Resampler::new(hw_rate, config.sample_rate),
            frame: Vec::new(),
            samples: Vec::new(),
            frames: 0,
            average: 0.0,
            peak: 0.0,
            failure: None,
        }
    }

    /// Converts one hardware buffer to configured-rate output samples, meters included.
    fn push_buffer(&mut self, hw: &[f32]) -> AudioChunk {
        let channels = usize::from(self.channels.max(1));
        let width = usize::from(self.hw_channels.max(1));
        let mut out = Vec::with_capacity(hw.len());
        for hw_frame in hw.chunks(width) {
            self.frame.clear();
            remap_frame(hw_frame, &mut self.frame, channels);
            if self.resample.prev.len() != channels {
                self.resample.prev.resize(channels, 0.0);
            }
            while self.resample.frac < 1.0 {
                let t = self.resample.frac as f32;
                for channel in 0..channels {
                    let value = self.resample.prev[channel] * (1.0 - t) + self.frame[channel] * t;
                    let loudness = value.abs().min(1.0);
                    self.peak = f32::max(self.peak * 0.9995, loudness);
                    self.average += 0.02 * (loudness - self.average);
                    out.push((value.clamp(-1.0, 1.0) * 32767.0) as i16);
                }
                self.resample.frac += self.resample.step;
            }
            self.resample.frac -= 1.0;
            self.resample.prev.copy_from_slice(&self.frame);
        }
        AudioChunk {
            data: out,
            sample_rate: self.rate,
            channels: self.channels,
        }
    }
}

/// Mixes or splits one interleaved hardware frame to `channels` configured channels; wider
/// layouts collapse to their average.
fn remap_frame(hw: &[f32], out: &mut Vec<f32>, channels: usize) {
    match (hw.len(), channels) {
        (1, 1) => out.push(hw[0]),
        (2, 2) => out.extend_from_slice(hw),
        (1, 2) => {
            out.push(hw[0]);
            out.push(hw[0]);
        }
        (2, 1) => out.push((hw[0] + hw[1]) / 2.0),
        (width, 1) => out.push(hw.iter().sum::<f32>() / width as f32),
        (_, width) => {
            let mix = hw.iter().sum::<f32>() / hw.len() as f32;
            out.resize(width, mix);
        }
    }
}

/// One recording in flight. The lock order is always recording state first, capture second; the
/// stream callbacks never take the first lock.
struct Active {
    stream: Stream,
    capture: Arc<Mutex<Capture>>,
    config: RecordConfig,
    path: Option<String>,
}

enum Phase {
    Idle,
    Active(Active),
    Disposed,
}

/// The device's single recording: one gate, shared by every handle the recorder hands out.
struct Shared {
    phase: Phase,
}

static SHARED: LazyLock<Mutex<Shared>> =
    LazyLock::new(|| Mutex::new(Shared { phase: Phase::Idle }));

/// The recording lock. A poisoned lock keeps its last state and the next call still answers.
fn lock_shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whole milliseconds of `frames` at `rate`, rounded to nearest; a zero rate reports zero.
fn millis(frames: u64, rate: u32) -> u64 {
    if rate == 0 {
        0
    } else {
        (frames * 1000 + u64::from(rate) / 2) / u64::from(rate)
    }
}

/// A cpal failure as one of ours, by kind. cpal carries no numeric host code, so `status` stays
/// empty and the kind alone names the refusal.
fn map_error(error: cpal::Error) -> RecorderError {
    match error.kind() {
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable => {
            RecorderError::new(ErrorKind::NotAvailable)
        }
        cpal::ErrorKind::PermissionDenied => RecorderError::new(ErrorKind::PermissionDenied),
        cpal::ErrorKind::UnsupportedConfig => RecorderError::new(ErrorKind::FormatNotSupported),
        cpal::ErrorKind::InvalidInput => RecorderError::new(ErrorKind::InvalidArgument),
        _ => RecorderError::new(ErrorKind::Backend),
    }
}

/// Reads one ALSA buffer into the shared capture state; the sink runs after the lock is gone so
/// a callback that re-enters the recorder cannot deadlock the audio thread.
fn deliver(hw: &[f32], capture: &Mutex<Capture>) {
    let (sink, chunk) = {
        let mut data = capture.lock().unwrap_or_else(PoisonError::into_inner);
        // A paused recording drops its buffers; a failed stream delivers nothing more.
        if data.paused || data.failure.is_some() {
            return;
        }
        let chunk = data.push_buffer(hw);
        data.frames += u64::try_from(chunk.data.len() / usize::from(chunk.channels.max(1)))
            .unwrap_or(u64::MAX);
        if data.keep {
            data.samples.extend_from_slice(&chunk.data);
        }
        (data.sink.clone(), chunk)
    };
    sink(chunk);
}

/// Records the stream's first fatal failure; the next answer surfaces it instead of a stale
/// meter.
fn record_failure(capture: &Mutex<Capture>, error: cpal::Error) {
    // An input xrun is a skipped buffer, not a dead stream; ALSA recovers and keeps capturing.
    if matches!(error.kind(), cpal::ErrorKind::Xrun) {
        return;
    }
    let mut data = capture.lock().unwrap_or_else(PoisonError::into_inner);
    if data.failure.is_none() {
        // Mid-stream errors are Backend whatever cpal named them; the contract pins them there.
        data.failure = Some(RecorderError::new(ErrorKind::Backend));
    }
}

/// Builds and starts one typed input stream; `to_float` maps the hardware's own samples to f32.
fn open_stream<T>(
    device: &cpal::Device,
    native: StreamConfig,
    capture: Arc<Mutex<Capture>>,
    to_float: fn(T) -> f32,
) -> Result<Stream, RecorderError>
where
    T: cpal::SizedSample,
{
    let failure = capture.clone();
    let stream = device
        .build_input_stream::<T, _, _>(
            native,
            move |data: &[T], _info: &InputCallbackInfo| {
                // One conversion per buffer keeps the audio thread free of per-sample locking.
                let hw: Vec<f32> = data.iter().map(|value| to_float(*value)).collect();
                deliver(&hw, &capture);
            },
            move |error: cpal::Error| record_failure(&failure, error),
            // The timeout bounds the build: `None` would wait indefinitely, and start must not
            // hang.
            Some(Duration::from_secs(2)),
        )
        .map_err(map_error)?;
    // A built stream delivers nothing until play starts it.
    stream.play().map_err(map_error)?;
    Ok(stream)
}

/// Builds the input stream on the hardware's own format, converting every sample to f32 frames.
/// Formats this layer does not convert answer `FormatNotSupported`.
fn build_stream(
    device: &cpal::Device,
    native: StreamConfig,
    capture: Arc<Mutex<Capture>>,
    format: SampleFormat,
) -> Result<Stream, RecorderError> {
    match format {
        SampleFormat::F32 => open_stream::<f32>(device, native, capture, |value| value),
        SampleFormat::I16 => {
            open_stream::<i16>(device, native, capture, |value| f32::from(value) / 32768.0)
        }
        SampleFormat::U16 => open_stream::<u16>(device, native, capture, |value| {
            (f32::from(value) - 32768.0) / 32768.0
        }),
        // Wider ints, doubles and DSD have no converter here; PCM-only is v1's scope.
        _ => Err(RecorderError::new(ErrorKind::FormatNotSupported)),
    }
}

/// Ends a recording and answers what it left behind. The phase swap happens first, so the call
/// resolves exactly once; the lock frees before the stream drops, or a re-entrant sink deadlocks.
fn finish(discard: bool) -> Result<StoppedRecording, RecorderError> {
    let Active {
        stream,
        capture,
        config,
        path,
    } = {
        let mut guard = lock_shared();
        match std::mem::replace(&mut guard.phase, Phase::Idle) {
            Phase::Active(active) => active,
            // Disposed stays disposed: the swap above was only a peek at the phase.
            Phase::Disposed => {
                guard.phase = Phase::Disposed;
                return Err(RecorderError::new(ErrorKind::NotAvailable));
            }
            Phase::Idle => return Err(RecorderError::new(ErrorKind::InvalidState)),
        }
    };
    // Dropping the stream stops and joins ALSA's worker; it must run with the recording lock
    // released, so a sink that calls back into the recorder can take it.
    drop(stream);
    if discard {
        // Cancel keeps nothing: the samples live only in memory until stop writes them.
        return Ok(StoppedRecording::default());
    }
    let data = capture.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(error) = &data.failure {
        // A stream that died mid-flight answers the stop; the partial file is not it.
        return Err(error.clone());
    }
    match path {
        Some(path) => write_wav(&path, config.sample_rate, config.channels, &data.samples),
        None => Ok(StoppedRecording {
            path: None,
            duration_ms: Some(millis(data.frames, config.sample_rate)),
        }),
    }
}

/// Pause and resume drive the stream and flip the capture's flag; the flag lands only when the
/// stream call succeeded, so a failed pause leaves delivery untouched.
fn set_paused(paused: bool) -> Result<(), RecorderError> {
    let mut guard = lock_shared();
    match &mut guard.phase {
        Phase::Active(active) => {
            let answer = if paused {
                active.stream.pause()
            } else {
                active.stream.play()
            };
            answer.map_err(map_error)?;
            // ALSA's pause suspends the PCM without synchronizing its worker, so deliver drops
            // what a callback already running would push.
            active
                .capture
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .paused = paused;
            Ok(())
        }
        Phase::Disposed => Err(RecorderError::new(ErrorKind::NotAvailable)),
        Phase::Idle => Err(RecorderError::new(ErrorKind::InvalidState)),
    }
}

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        // ALSA offers no consent prompt to ask; the session's own access governs capture.
        // A Partial, named in docs/parity.md.
        Box::pin(ready(Ok(PermissionState::Granted)))
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        // There is no prompt to show, so asking changes nothing.
        Box::pin(ready(Ok(PermissionState::Granted)))
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        let host = cpal::default_host();
        // input_devices is devices() kept to capture sides; a device that will not even
        // describe is skipped, not fatal.
        let answer = host
            .input_devices()
            .map(|devices| {
                devices
                    .filter_map(|device| {
                        let name = device.description().ok()?.name().to_owned();
                        Some(InputDevice {
                            id: name.clone(),
                            label: Some(name),
                        })
                    })
                    .collect()
            })
            .map_err(map_error);
        Box::pin(ready(answer))
    }

    fn is_encoder_supported(
        &self,
        encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        // The layer writes PCM itself through the shared WAV writer; compressed encoders are
        // v2 work and answer false.
        let supported = matches!(encoder, Encoder::Wav | Encoder::Pcm16bits);
        Box::pin(ready(Ok(supported)))
    }

    fn start(&self, config: &RecordConfig, sink: ChunkSink) -> Result<(), RecorderError> {
        let mut guard = lock_shared();
        match guard.phase {
            Phase::Disposed => return Err(RecorderError::new(ErrorKind::NotAvailable)),
            Phase::Active(_) => return Err(RecorderError::new(ErrorKind::InProgress)),
            Phase::Idle => {}
        }
        if !matches!(config.encoder, Encoder::Wav | Encoder::Pcm16bits) {
            return Err(RecorderError::new(ErrorKind::FormatNotSupported));
        }
        if matches!(&config.output, Output::File { path } if path.trim().is_empty()) {
            return Err(RecorderError::new(ErrorKind::InvalidArgument));
        }
        let path = match &config.output {
            Output::File { path } => Some(path.clone()),
            Output::Stream => None,
        };
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            return Err(RecorderError::new(ErrorKind::NotAvailable));
        };
        // The hardware's own config is what ALSA opens cleanly; the callback remaps and
        // resamples it into the configured shape.
        let native = device.default_input_config().map_err(map_error)?;
        let capture = Arc::new(Mutex::new(Capture::new(
            config,
            sink,
            native.sample_rate(),
            native.channels(),
        )));
        let stream = build_stream(
            &device,
            native.config(),
            capture.clone(),
            native.sample_format(),
        )?;
        guard.phase = Phase::Active(Active {
            stream,
            capture,
            config: config.clone(),
            path,
        });
        Ok(())
    }

    fn pause(&self) -> Result<(), RecorderError> {
        set_paused(true)
    }

    fn resume(&self) -> Result<(), RecorderError> {
        set_paused(false)
    }

    fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>> {
        let guard = lock_shared();
        let answer = match &guard.phase {
            Phase::Active(active) => {
                let data = active
                    .capture
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some(error) = &data.failure {
                    // A dead stream reports its failure; the meters would be a stale lie.
                    Err(error.clone())
                } else {
                    // A revoked permission can deliver silence instead of an error; these meters
                    // are how the app sees it.
                    Ok(Amplitude {
                        average: data.average,
                        peak: data.peak,
                    })
                }
            }
            Phase::Disposed => Err(RecorderError::new(ErrorKind::NotAvailable)),
            Phase::Idle => Err(RecorderError::new(ErrorKind::InvalidState)),
        };
        drop(guard);
        Box::pin(ready(answer))
    }

    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        Box::pin(ready(finish(false)))
    }

    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        Box::pin(ready(finish(true).map(|_| ())))
    }

    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        let mut guard = lock_shared();
        let old = std::mem::replace(&mut guard.phase, Phase::Disposed);
        // The old recording drops after the lock: its stream teardown joins the audio thread,
        // which a re-entrant sink must be able to unblock.
        drop(guard);
        drop(old);
        Box::pin(ready(Ok(())))
    }
}
