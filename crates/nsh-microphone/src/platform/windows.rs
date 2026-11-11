//! Windows: cpal's WASAPI host captures at the device's own rate and channels and this layer
//! resamples to the configured ones. WASAPI has no runtime consent call, so permissions answer
//! granted; Windows' privacy settings can still deny a start, which the mapped cpal error says.

use std::{
    future::ready,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use cpal::{
    Error as CpalError, ErrorKind as CpalErrorKind, Stream,
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
    fn new(hw_rate: f64, rate: u32) -> Self {
        // A zero rate (never allowed by the recorder) falls back to passthrough, not a division.
        let step = if hw_rate > 0.0 && rate > 0 {
            hw_rate / f64::from(rate)
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

/// What the stream callback writes and the API reads: samples, meters, the drop flags.
struct TapData {
    sink: ChunkSink,
    file: bool,
    rate: u32,
    channels: u16,
    hw_channels: usize,
    paused: bool,
    failed: Option<RecorderError>,
    samples: Vec<i16>,
    frames: u64,
    average: f32,
    peak: f32,
    resample: Resampler,
    frame: Vec<f32>,
}

impl TapData {
    fn new(config: &RecordConfig, sink: ChunkSink, hw_rate: u32, hw_channels: u16) -> Self {
        Self {
            sink,
            file: matches!(config.output, Output::File { .. }),
            rate: config.sample_rate,
            channels: config.channels,
            hw_channels: usize::from(hw_channels.max(1)),
            paused: false,
            failed: None,
            samples: Vec::new(),
            frames: 0,
            average: 0.0,
            peak: 0.0,
            resample: Resampler::new(f64::from(hw_rate), config.sample_rate),
            frame: Vec::new(),
        }
    }

    /// Converts one hardware frame to configured-rate output samples, meters included.
    fn push_frame(&mut self, hw_frame: &[f32], out: &mut Vec<i16>) {
        let channels = usize::from(self.channels.max(1));
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
}

/// A cpal failure as one of our codes. cpal 0.18 carries its reason as message text, which
/// `RecorderError` cannot hold, so the kind mapping is what survives.
fn map_cpal_error(error: CpalError) -> RecorderError {
    match error.kind() {
        CpalErrorKind::DeviceNotAvailable => RecorderError::new(ErrorKind::NotAvailable),
        CpalErrorKind::PermissionDenied => RecorderError::new(ErrorKind::PermissionDenied),
        CpalErrorKind::UnsupportedConfig => RecorderError::new(ErrorKind::FormatNotSupported),
        CpalErrorKind::InvalidInput => RecorderError::new(ErrorKind::InvalidArgument),
        _ => RecorderError::new(ErrorKind::Backend),
    }
}

/// Reads one stream buffer into the shared tap state; the sink runs after the lock is gone so a
/// callback that re-enters the recorder cannot deadlock the audio thread.
fn deliver(data: &[f32], tap: &Mutex<TapData>) {
    let (sink, chunk) = {
        let mut state = tap.lock().unwrap_or_else(PoisonError::into_inner);
        // A paused recording drops its buffers; a failed stream delivers nothing more.
        if state.paused || state.failed.is_some() {
            return;
        }
        let hw = state.hw_channels;
        let mut out = Vec::with_capacity(data.len() / hw * usize::from(state.channels.max(1)));
        // cpal hands over whole interleaved frames; a partial tail (never sent) is ignored.
        for frame in data.chunks(hw) {
            if frame.len() < hw {
                break;
            }
            state.push_frame(frame, &mut out);
        }
        let chunk = AudioChunk {
            data: out,
            sample_rate: state.rate,
            channels: state.channels,
        };
        state.frames += u64::try_from(chunk.data.len() / usize::from(chunk.channels.max(1)))
            .unwrap_or(u64::MAX);
        if state.file {
            state.samples.extend_from_slice(&chunk.data);
        }
        (state.sink.clone(), chunk)
    };
    sink(chunk);
}

/// Records the stream's first fatal failure; the next answer surfaces it instead of a stale
/// meter.
fn on_stream_error(tap: &Mutex<TapData>, error: CpalError) {
    // An input xrun is a skipped buffer, not a dead stream; the host keeps capturing.
    if matches!(error.kind(), CpalErrorKind::Xrun) {
        return;
    }
    // WASAPI's default-device-change notices carry no message and leave capture running; real
    // failures arrive with one.
    if matches!(
        error.kind(),
        CpalErrorKind::StreamInvalidated | CpalErrorKind::DeviceNotAvailable
    ) && error.message().is_none()
    {
        return;
    }
    let mut state = tap.lock().unwrap_or_else(PoisonError::into_inner);
    // Mid-stream failures are Backend whatever cpal named them; RecorderError has no room for
    // the message text.
    if state.failed.is_none() {
        state.failed = Some(RecorderError::new(ErrorKind::Backend));
    }
}

/// One recording in flight. Dropping it drops the stream, which is what halts WASAPI capture.
struct Active {
    stream: Stream,
    tap: Arc<Mutex<TapData>>,
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

/// Pause and resume drive the stream and flip the tap's flag; the flag lands only when the
/// stream call succeeded, so a failed pause leaves delivery untouched.
fn set_paused(paused: bool) -> Result<(), RecorderError> {
    let mut guard = lock_shared();
    match &mut guard.phase {
        Phase::Active(active) => {
            // cpal 0.18 names resume `play`.
            let outcome = if paused {
                active.stream.pause()
            } else {
                active.stream.play()
            };
            outcome.map_err(map_cpal_error)?;
            active
                .tap
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .paused = paused;
            Ok(())
        }
        Phase::Disposed => Err(RecorderError::new(ErrorKind::NotAvailable)),
        Phase::Idle => Err(RecorderError::new(ErrorKind::InvalidState)),
    }
}

/// Ends a recording and answers what it left behind. The phase swap happens first, so the call
/// resolves exactly once; the lock frees before the stream drops, or a re-entrant sink deadlocks.
fn finish(discard: bool) -> Result<StoppedRecording, RecorderError> {
    let Active {
        stream,
        tap,
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
    // WASAPI's drop joins the audio thread, so it must run with the recording lock released.
    drop(stream);
    if discard {
        // Cancel keeps nothing: the samples live only in memory until stop writes them.
        return Ok(StoppedRecording::default());
    }
    let data = tap.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(error) = &data.failed {
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

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        // WASAPI has no consent call to make; the privacy settings speak at stream start.
        Box::pin(ready(Ok(PermissionState::Granted)))
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        // There is no prompt to show; the first stream start is where Windows says no.
        Box::pin(ready(Ok(PermissionState::Granted)))
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        // Stable device ids live behind extra crates; the device's display name is this layer's id.
        let answer = cpal::default_host()
            .input_devices()
            .map(|devices| {
                devices
                    .map(|device| {
                        let name = device.to_string();
                        InputDevice {
                            id: name.clone(),
                            label: Some(name),
                        }
                    })
                    .collect()
            })
            .map_err(map_cpal_error);
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
        let device = host
            .default_input_device()
            .ok_or_else(|| RecorderError::new(ErrorKind::NotAvailable))?;
        // The device's own default config is the one format guaranteed to open; this layer
        // converts rate and channels in software, so no configured value is out of range here.
        // A device that will not open f32 (WASAPI's shared-mix format) answers its refusal.
        let default = device.default_input_config().map_err(map_cpal_error)?;
        let tap = Arc::new(Mutex::new(TapData::new(
            config,
            sink,
            default.sample_rate(),
            default.channels(),
        )));
        let data_tap = tap.clone();
        let error_tap = tap.clone();
        // The timeout bounds the build: `None` would wait indefinitely, and start must not hang.
        let stream = device
            .build_input_stream::<f32, _, _>(
                default.config(),
                move |data, _info| deliver(data, &data_tap),
                move |error| on_stream_error(&error_tap, error),
                Some(Duration::from_secs(2)),
            )
            .map_err(map_cpal_error)?;
        // cpal 0.18 names resume `play`; a built stream delivers nothing until it runs.
        stream.play().map_err(map_cpal_error)?;
        guard.phase = Phase::Active(Active {
            stream,
            tap,
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
                let data = active.tap.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(error) = &data.failed {
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
