//! The recorder as TSX sees it: `NativeRecorder`, a class whose async members reject with a
//! `code` on failure. Results are plain values; the gate and the platform stay on the Rust side.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nsh_core::prelude::*;
use tokio::sync::mpsc;

use crate::{
    backend::ChunkSink,
    error::{ErrorKind, RecorderError},
    options::{AudioChunk, Encoder, Output, RecordConfig},
    recorder::Recorder,
};

/// The options of one recording, from TSX. Unset fields keep `RecordConfig::default()`'s.
#[derive(JsObject, Clone, Debug, Default)]
pub struct NativeRecordConfig {
    /// Samples per second; the default is 44_100.
    pub sample_rate: Option<u32>,
    /// One or two; the default is one.
    pub channels: Option<u16>,
    /// Target bits per second; the default is 128_000.
    pub bit_rate: Option<u32>,
    /// The encoder, as TSX names it (`"aacLc"`); the default is `"aacLc"`.
    pub encoder: Option<String>,
    /// The file to write. Absent means a stream output: chunks reach the `onChunk` listener.
    pub path: Option<String>,
}

/// One microphone `listInputDevices` reports.
#[derive(JsObject, Clone, Debug)]
pub struct NativeInputDevice {
    /// The platform's identifier, opaque to the app.
    pub id: String,
    /// A human-readable name, when the platform gives one.
    pub label: Option<String>,
}

/// The loudness of the running recording, normalized to `0.0..=1.0`.
#[derive(JsObject, Clone, Debug)]
pub struct NativeAmplitude {
    /// The recent average level.
    pub average: f32,
    /// The recent peak level.
    pub peak: f32,
}

/// One buffer of a stream recording.
#[derive(JsObject, Clone, Debug)]
pub struct NativeAudioChunk {
    /// The interleaved 16-bit samples.
    pub data: Vec<i16>,
    /// The sample rate this buffer was captured at.
    pub sample_rate: u32,
    /// How many interleaved channels this buffer carries.
    pub channels: u16,
}

/// What a stopped recording left behind.
#[derive(JsObject, Clone, Debug)]
pub struct NativeStoppedRecording {
    /// The file written, for a file output.
    pub path: Option<String>,
    /// How long the recording ran, when the platform measured it.
    pub duration_ms: Option<u64>,
}

/// Where a handle's stream chunks go: the channel `on_chunk` opened, or nowhere.
type ChunkSlot = Arc<Mutex<Option<mpsc::UnboundedSender<AudioChunk>>>>;

/// A recording handle. Create one and keep it; every handle shares the platform's recording gate.
#[class]
pub struct NativeRecorder {
    recorder: Recorder,
    chunks: ChunkSlot,
}

#[class]
impl NativeRecorder {
    /// The platform's recorder.
    pub fn new() -> NativeRecorder {
        NativeRecorder {
            recorder: Recorder::new(),
            chunks: ChunkSlot::default(),
        }
    }

    /// Whether recording is allowed: `"granted"`, `"denied"` or `"undetermined"`.
    pub async fn check_permissions(this: Instance<Self>) -> Result<String, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let permission = recorder.check_permissions().await.map_err(into_js)?;
        Ok(permission.name().to_owned())
    }

    /// Asks for recording to be allowed, prompting when the platform shows a prompt.
    pub async fn request_permissions(this: Instance<Self>) -> Result<String, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let permission = recorder.request_permissions().await.map_err(into_js)?;
        Ok(permission.name().to_owned())
    }

    /// The microphones the app could record from.
    pub async fn list_input_devices(
        this: Instance<Self>,
    ) -> Result<Vec<NativeInputDevice>, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let devices = recorder.list_input_devices().await.map_err(into_js)?;
        Ok(devices
            .into_iter()
            .map(|device| NativeInputDevice {
                id: device.id,
                label: device.label,
            })
            .collect())
    }

    /// Whether the platform can write `encoder`, named as TSX names it (`"aacLc"`, `"wav"`, ...).
    pub async fn is_encoder_supported(
        this: Instance<Self>,
        encoder: String,
    ) -> Result<bool, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let encoder = encoder_from_name(&encoder).ok_or_else(|| {
            invalid("encoder must be one of the AudioEncoder names, such as \"aacLc\"")
        })?;
        recorder
            .is_encoder_supported(encoder)
            .await
            .map_err(into_js)
    }

    /// Starts one recording: to `config.path`'s file, or, without a path, as a stream whose
    /// chunks reach the listener `onChunk` set (a stream with no listener drops them).
    pub async fn start(
        this: Instance<Self>,
        config: Option<NativeRecordConfig>,
    ) -> Result<(), JsError> {
        let (recorder, chunks) =
            this.with(|native| (native.recorder.clone(), native.chunks.clone()));
        let config = to_record_config(config.unwrap_or_default())?;
        recorder
            .start(&config, chunk_sink(chunks))
            .await
            .map_err(into_js)
    }

    /// Calls `listener` with each chunk of the stream recordings this handle starts, replacing
    /// any earlier listener of the handle. The returned function ends it, and so does the end of
    /// what called it (a component).
    pub fn on_chunk(
        &self,
        #[args(chunk)] listener: Callback<NativeAudioChunk>,
        cx: CallCx<'_>,
    ) -> Stop {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mine = sender.clone();
        *lock(&self.chunks) = Some(sender);
        // The sink is Send and fires from the platform's thread; the callback runs on the engine.
        cx.start(async move {
            while let Some(chunk) = receiver.recv().await {
                listener.post(to_native_chunk(chunk));
            }
        });
        let chunks = Arc::clone(&self.chunks);
        Stop::new(move || clear_chunks(&chunks, &mine))
    }

    /// Pauses the running recording; any other state answers `ERR_MICROPHONE_INVALID_STATE`.
    pub fn pause(&self) -> Result<(), JsError> {
        self.recorder.pause().map_err(into_js)
    }

    /// Resumes a paused recording; any other state answers `ERR_MICROPHONE_INVALID_STATE`.
    pub fn resume(&self) -> Result<(), JsError> {
        self.recorder.resume().map_err(into_js)
    }

    /// The loudness of the running or paused recording.
    pub async fn amplitude(this: Instance<Self>) -> Result<NativeAmplitude, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let amplitude = recorder.amplitude().await.map_err(into_js)?;
        Ok(NativeAmplitude {
            average: amplitude.average,
            peak: amplitude.peak,
        })
    }

    /// Stops the recording and resolves with what it left behind.
    pub async fn stop(this: Instance<Self>) -> Result<NativeStoppedRecording, JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let stopped = recorder.stop().await.map_err(into_js)?;
        Ok(NativeStoppedRecording {
            path: stopped.path,
            duration_ms: stopped.duration_ms,
        })
    }

    /// Ends the recording and discards its output.
    pub async fn cancel(this: Instance<Self>) -> Result<(), JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        recorder.cancel().await.map_err(into_js)
    }

    /// Ends the handle: cancels a running recording first; later calls answer
    /// `ERR_MICROPHONE_NOT_AVAILABLE`. Named `release`: the class's own `dispose` only frees the value.
    pub async fn release(this: Instance<Self>) -> Result<(), JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        recorder.dispose().await.map_err(into_js)
    }
}

impl Default for NativeRecorder {
    fn default() -> Self {
        Self::new()
    }
}

fn lock(chunks: &ChunkSlot) -> MutexGuard<'_, Option<mpsc::UnboundedSender<AudioChunk>>> {
    chunks.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sink a stream start hands the recorder: each chunk goes to the handle's listener, or is
/// dropped when it has none.
fn chunk_sink(chunks: ChunkSlot) -> ChunkSink {
    Arc::new(move |chunk| {
        if let Some(sender) = lock(&chunks).as_ref() {
            // A closed channel means the listener ended; its chunks have nowhere to go.
            let _ = sender.send(chunk);
        }
    })
}

/// Ends the listener whose channel `mine` is. A listener set since replaced this one, and its
/// channel stays.
fn clear_chunks(chunks: &ChunkSlot, mine: &mpsc::UnboundedSender<AudioChunk>) {
    let mut slot = lock(chunks);
    if slot
        .as_ref()
        .is_some_and(|current| current.same_channel(mine))
    {
        *slot = None;
    }
}

fn to_native_chunk(chunk: AudioChunk) -> NativeAudioChunk {
    NativeAudioChunk {
        data: chunk.data,
        sample_rate: chunk.sample_rate,
        channels: chunk.channels,
    }
}

/// The error a rejection carries: its message, and the `code` apps match on.
fn into_js(error: RecorderError) -> JsError {
    JsError::Error(error.kind().message().to_owned()).with_code(error.code())
}

fn invalid(message: &str) -> JsError {
    JsError::Error(message.to_owned()).with_code(ErrorKind::InvalidArgument.code())
}

/// Every encoder the crate knows; the table `encoder_from_name` reads.
const ENCODERS: [Encoder; 10] = [
    Encoder::AacLc,
    Encoder::AacEld,
    Encoder::AacHe,
    Encoder::AmrNb,
    Encoder::AmrWb,
    Encoder::Flac,
    Encoder::Opus,
    Encoder::Pcm16bits,
    Encoder::Vorbis,
    Encoder::Wav,
];

/// The encoder a TSX name stands for; `None` when the name is not one of `Encoder::name`'s.
fn encoder_from_name(name: &str) -> Option<Encoder> {
    ENCODERS.into_iter().find(|encoder| encoder.name() == name)
}

/// Turns the TSX config into the crate's. Unset fields keep `RecordConfig::default()`'s; a
/// missing `path` means a stream output.
fn to_record_config(config: NativeRecordConfig) -> Result<RecordConfig, JsError> {
    let defaults = RecordConfig::default();
    let encoder = match config.encoder.as_deref() {
        None => defaults.encoder,
        Some(name) => encoder_from_name(name).ok_or_else(|| {
            invalid("encoder must be one of the AudioEncoder names, such as \"aacLc\"")
        })?,
    };
    Ok(RecordConfig {
        sample_rate: config.sample_rate.unwrap_or(defaults.sample_rate),
        channels: config.channels.unwrap_or(defaults.channels),
        bit_rate: config.bit_rate.unwrap_or(defaults.bit_rate),
        encoder,
        output: config
            .path
            .map_or(Output::Stream, |path| Output::File { path }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_encoder_has_the_name_tsx_uses() {
        for encoder in ENCODERS {
            assert_eq!(encoder_from_name(encoder.name()), Some(encoder));
        }
        assert_eq!(encoder_from_name("aacLC"), None);
    }

    #[test]
    fn an_unknown_encoder_name_is_refused_and_a_known_one_is_kept() {
        let refused = to_record_config(NativeRecordConfig {
            encoder: Some("mp3".to_owned()),
            path: Some("recording.m4a".to_owned()),
            ..NativeRecordConfig::default()
        })
        .err();
        assert_eq!(
            refused.as_ref().and_then(|error| error.code()),
            Some("ERR_MICROPHONE_INVALID_ARGUMENT")
        );

        let kept = to_record_config(NativeRecordConfig {
            encoder: Some("opus".to_owned()),
            path: Some("recording.m4a".to_owned()),
            ..NativeRecordConfig::default()
        })
        .ok();
        assert_eq!(kept.map(|config| config.encoder), Some(Encoder::Opus));
    }

    #[test]
    fn a_config_without_a_path_streams() {
        let config = to_record_config(NativeRecordConfig::default()).ok();
        assert_eq!(config, Some(RecordConfig::default()));
        assert_eq!(config.map(|config| config.output), Some(Output::Stream));
    }

    fn chunk(first: i16) -> AudioChunk {
        AudioChunk {
            data: vec![first, first + 1],
            sample_rate: 16_000,
            channels: 1,
        }
    }

    #[test]
    fn the_sink_hands_chunks_to_the_listener_in_order_and_drops_them_without_one() {
        let chunks = ChunkSlot::default();
        let sink = chunk_sink(Arc::clone(&chunks));
        // No listener yet: the chunk is dropped, not queued for a later one.
        sink(chunk(0));

        let (sender, mut receiver) = mpsc::unbounded_channel();
        *lock(&chunks) = Some(sender);
        sink(chunk(1));
        sink(chunk(3));
        assert_eq!(receiver.try_recv().ok(), Some(chunk(1)));
        assert_eq!(receiver.try_recv().ok(), Some(chunk(3)));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn ending_a_listener_does_not_end_the_one_that_replaced_it() {
        let chunks = ChunkSlot::default();
        let (first, _first_receiver) = mpsc::unbounded_channel();
        let first_mine = first.clone();
        *lock(&chunks) = Some(first);
        let (second, mut second_receiver) = mpsc::unbounded_channel();
        let second_mine = second.clone();
        *lock(&chunks) = Some(second);

        clear_chunks(&chunks, &first_mine);
        chunk_sink(Arc::clone(&chunks))(chunk(5));
        assert_eq!(second_receiver.try_recv().ok(), Some(chunk(5)));

        clear_chunks(&chunks, &second_mine);
        assert!(lock(&chunks).is_none());
    }

    #[test]
    fn a_chunk_keeps_its_samples_rate_and_channels() {
        let native = to_native_chunk(chunk(7));
        assert_eq!(native.data, vec![7, 8]);
        assert_eq!(native.sample_rate, 16_000);
        assert_eq!(native.channels, 1);
    }

    #[test]
    fn unset_options_keep_the_defaults() {
        let config = to_record_config(NativeRecordConfig {
            path: Some("recording.m4a".to_owned()),
            ..NativeRecordConfig::default()
        })
        .ok();
        assert_eq!(
            config,
            Some(RecordConfig {
                output: Output::File {
                    path: "recording.m4a".to_owned()
                },
                ..RecordConfig::default()
            })
        );
    }
}
