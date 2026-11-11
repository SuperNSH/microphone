//! The web is an unsupported target: every method answers `NotAvailable` and resolves at once.

use crate::{
    backend::{Backend, BackendFuture, ChunkSink},
    error::{ErrorKind, RecorderError},
    options::{Amplitude, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording},
};

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn is_encoder_supported(
        &self,
        _encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
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
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }

    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        Box::pin(std::future::ready(Err(RecorderError::new(
            ErrorKind::NotAvailable,
        ))))
    }
}
