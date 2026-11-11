//! The inputs and outputs of a recording: encoders, configs, permissions, chunks.

/// An encoder, named as Flutter record's `AudioEncoder` names it; see [`Encoder::name`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoder {
    /// AAC low complexity, the default everywhere it exists.
    AacLc,
    /// AAC enhanced low delay, for low-latency capture.
    AacEld,
    /// AAC high efficiency, for narrow-band voice.
    AacHe,
    /// AMR narrow band, telephony voice.
    AmrNb,
    /// AMR wide band.
    AmrWb,
    /// Free Lossless Audio Codec.
    Flac,
    /// Opus, for network voice.
    Opus,
    /// Raw 16-bit samples, no container.
    Pcm16bits,
    /// Vorbis in an Ogg container.
    Vorbis,
    /// 16-bit PCM in a WAV container.
    Wav,
}

impl Encoder {
    /// The name the TSX side uses, the exact `AudioEncoder` string, so both ends map both ways.
    pub fn name(self) -> &'static str {
        match self {
            Encoder::AacLc => "aacLc",
            Encoder::AacEld => "aacEld",
            Encoder::AacHe => "aacHe",
            Encoder::AmrNb => "amrNb",
            Encoder::AmrWb => "amrWb",
            Encoder::Flac => "flac",
            Encoder::Opus => "opus",
            Encoder::Pcm16bits => "pcm16bits",
            Encoder::Vorbis => "vorbis",
            Encoder::Wav => "wav",
        }
    }
}

/// Where a recording goes: a file by `path`, or chunks to the sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Write to this path; a blank path refuses the start.
    File {
        /// The file to write, as the platform names paths.
        path: String,
    },
    /// Deliver chunks only; `StoppedRecording::path` is `None` there.
    Stream,
}

impl Default for Output {
    /// Chunks to the sink: the output that needs no path.
    fn default() -> Self {
        Self::Stream
    }
}

/// The options of one recording. Start from [`RecordConfig::default`] and set what you need.
///
/// Options a platform cannot apply are accepted and listed in `docs/parity.md`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordConfig {
    /// Samples per second. Zero refuses the start.
    pub sample_rate: u32,
    /// One or two; zero or more refuses the start.
    pub channels: u16,
    /// Target bits per second of the encoded stream. Zero is allowed on `Pcm16bits` and `Wav` only.
    pub bit_rate: u32,
    /// The encoder to write with.
    pub encoder: Encoder,
    /// Where the recording goes.
    pub output: Output,
}

impl Default for RecordConfig {
    /// Record's own defaults, so a default start succeeds: 44_100 Hz, one channel, 128 kbit/s
    /// AAC, chunks to the sink.
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            channels: 1,
            bit_rate: 128_000,
            encoder: Encoder::AacLc,
            output: Output::Stream,
        }
    }
}

/// Whether recording is allowed, as react-native-audio-api names the states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionState {
    /// Recording may start now.
    Granted,
    /// The user or the system refused; asking again will not help until the settings change.
    Denied,
    /// No answer yet; a request may prompt.
    Undetermined,
}

impl PermissionState {
    /// The state's name, the exact string the TSX side uses: granted, denied, undetermined.
    pub fn name(self) -> &'static str {
        match self {
            PermissionState::Granted => "granted",
            PermissionState::Denied => "denied",
            PermissionState::Undetermined => "undetermined",
        }
    }
}

/// One input device, as `list_input_devices` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputDevice {
    /// The platform's identifier, opaque to the app.
    pub id: String,
    /// A human-readable name, when the platform gives one.
    pub label: Option<String>,
}

/// The loudness of the running recording, normalized to `0.0..=1.0` as record's `Amplitude` is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Amplitude {
    /// The recent average level.
    pub average: f32,
    /// The recent peak level.
    pub peak: f32,
}

/// What a stopped recording left behind. `path` is `None` for a stream output, as record's stop is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoppedRecording {
    /// The file written, for a file output.
    pub path: Option<String>,
    /// How long the recording ran, when the platform measured it.
    pub duration_ms: Option<u64>,
}

/// One buffer of 16-bit samples from the microphone, interleaved by channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioChunk {
    /// The interleaved samples.
    pub data: Vec<i16>,
    /// The sample rate this buffer was captured at.
    pub sample_rate: u32,
    /// How many interleaved channels this buffer carries.
    pub channels: u16,
}
