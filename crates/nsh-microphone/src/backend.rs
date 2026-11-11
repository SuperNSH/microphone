//! The seam between `Recorder` and a platform: the queries, and one recording that answers once.

use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    error::RecorderError,
    options::{
        Amplitude, AudioChunk, Encoder, InputDevice, PermissionState, RecordConfig,
        StoppedRecording,
    },
};

/// A future a backend returns. Each call's future borrows the backend.
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Receives the recording's audio while it runs. A source drops chunks while the recording pauses.
pub type ChunkSink = Arc<dyn Fn(AudioChunk) + Send + Sync>;

/// What a platform provides. [`crate::Recorder`] adds the rules: one recording at a time, one
/// answer per call.
pub trait Backend: Send + Sync {
    /// Whether recording is allowed now.
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>>;

    /// Asks for recording to be allowed, prompting when the platform shows a prompt.
    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>>;

    /// The microphones the app could record from.
    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>>;

    /// Whether the platform can write `encoder`.
    fn is_encoder_supported(
        &self,
        encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>>;

    /// Starts delivering chunks to `sink` and returns at once. An `Err` means nothing started.
    fn start(&self, config: &RecordConfig, sink: ChunkSink) -> Result<(), RecorderError>;

    /// Stops delivering chunks; the recording continues.
    fn pause(&self) -> Result<(), RecorderError>;

    /// Delivers chunks again after a pause.
    fn resume(&self) -> Result<(), RecorderError>;

    /// The loudness of the running or paused recording.
    fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>>;

    /// Finishes the recording and resolves with what it left behind, exactly once.
    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>>;

    /// Ends the recording and discards its output, exactly once.
    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>>;

    /// Releases everything the backend holds; later calls answer `NotAvailable`.
    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>>;
}
