//! The recorder as TSX sees it: `NativeRecorder`, a class whose async members reject with a
//! `code` on failure. Results are plain values; the gate and the platform stay on the Rust side.

use std::sync::Arc;

use nsh_core::prelude::*;

use crate::{
    backend::ChunkSink,
    error::{ErrorKind, RecorderError},
    options::{Encoder, Output, RecordConfig},
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
    /// The file to write. Absent means a stream output, which this surface refuses.
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

/// What a stopped recording left behind.
#[derive(JsObject, Clone, Debug)]
pub struct NativeStoppedRecording {
    /// The file written, for a file output.
    pub path: Option<String>,
    /// How long the recording ran, when the platform measured it.
    pub duration_ms: Option<u64>,
}

/// A recording handle. Create one and keep it; every handle shares the platform's recording gate.
#[class]
pub struct NativeRecorder {
    recorder: Recorder,
}

#[class]
impl NativeRecorder {
    /// The platform's recorder.
    pub fn new() -> NativeRecorder {
        NativeRecorder {
            recorder: Recorder::new(),
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

    /// Starts one recording to `config.path`'s file; a config without a path (a stream output)
    /// answers `ERR_MICROPHONE_INVALID_ARGUMENT`: chunks are not wired in NSH 0.4.0.
    pub async fn start(
        this: Instance<Self>,
        config: Option<NativeRecordConfig>,
    ) -> Result<(), JsError> {
        let recorder = this.with(|native| native.recorder.clone());
        let config = to_record_config(config.unwrap_or_default())?;
        // The platform layer writes the file; a stream consumer is what this surface lacks.
        let sink: ChunkSink = Arc::new(|_| {});
        recorder.start(&config, sink).await.map_err(into_js)
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
/// missing `path` means a stream output, which this surface refuses (see `docs/parity.md`).
fn to_record_config(config: NativeRecordConfig) -> Result<RecordConfig, JsError> {
    let Some(path) = config.path else {
        return Err(invalid(
            "The native surface records to a file: pass a path (see docs/parity.md).",
        ));
    };
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
        output: Output::File { path },
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
    fn a_start_with_a_stream_output_is_refused() {
        let refused = to_record_config(NativeRecordConfig::default()).err();
        assert_eq!(
            refused.as_ref().and_then(|error| error.code()),
            Some("ERR_MICROPHONE_INVALID_ARGUMENT")
        );
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
