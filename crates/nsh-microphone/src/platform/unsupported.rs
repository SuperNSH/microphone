//! A target with no recording backend: nothing is available, and no recording starts.

use std::future::ready;

use crate::{
    backend::{Backend, BackendFuture, ChunkSink},
    error::{ErrorKind, RecorderError},
    options::{Amplitude, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording},
};

pub(crate) struct NativeBackend;

/// Every call answers `NotAvailable`: this target has no microphone service to ask.
fn unavailable<T>() -> BackendFuture<'static, Result<T, RecorderError>> {
    Box::pin(ready(Err(RecorderError::new(ErrorKind::NotAvailable))))
}

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        unavailable()
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        unavailable()
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        unavailable()
    }

    fn is_encoder_supported(
        &self,
        _encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        unavailable()
    }

    fn start(&self, _config: &RecordConfig, _sink: ChunkSink) -> Result<(), RecorderError> {
        Err(RecorderError::new(ErrorKind::NotAvailable))
    }

    fn pause(&self) -> Result<(), RecorderError> {
        Err(RecorderError::new(ErrorKind::NotAvailable))
    }

    fn resume(&self) -> Result<(), RecorderError> {
        Err(RecorderError::new(ErrorKind::NotAvailable))
    }

    fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>> {
        unavailable()
    }

    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        unavailable()
    }

    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        unavailable()
    }

    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        unavailable()
    }
}
