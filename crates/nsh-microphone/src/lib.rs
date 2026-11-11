//! Recording to file or stream for Native Shell apps: voice notes and level meters. Not
//! speech-to-text. Each call resolves exactly once: success, a coded failure, or a cancel.

mod availability;
mod backend;
mod error;
mod fake;
mod native;
mod options;
mod platform;
mod recorder;

use nsh_core::prelude::*;

pub use availability::{
    Availability, AvailabilityState, MicrophoneAvailability, use_microphone_availability,
    use_microphone_availability_for,
};
pub use backend::{Backend, BackendFuture, ChunkSink};
pub use error::{ErrorKind, RecorderError};
pub use fake::FakeBackend;
pub use native::NativeRecorder;
pub use options::{
    Amplitude, AudioChunk, Encoder, InputDevice, Output, PermissionState, RecordConfig,
    StoppedRecording,
};
pub use recorder::{Recorder, RecordingState};

/// The crate's UI set: what TSX imports from `nsh-microphone/native`.
#[native_module("nsh-microphone/native")]
fn native(registry: &mut Registry) {
    registry.class::<NativeRecorder>();
}
