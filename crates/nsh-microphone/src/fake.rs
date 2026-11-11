//! A backend with scripted answers. Tests use it, and so can an app that runs without a microphone.

use std::{
    future::ready,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use tokio::sync::oneshot;

use crate::{
    backend::{Backend, BackendFuture, ChunkSink},
    error::{ErrorKind, RecorderError},
    options::{
        Amplitude, AudioChunk, Encoder, InputDevice, PermissionState, RecordConfig,
        StoppedRecording,
    },
};

/// A backend whose answers the test sets, and whose stop the test holds and finishes.
///
/// Like a platform, a cancel does not finish a held stop: the test answers it, as the system
/// would. Clones share one state.
#[derive(Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    check_answer: Result<PermissionState, RecorderError>,
    request_answer: Result<PermissionState, RecorderError>,
    devices: Result<Vec<InputDevice>, RecorderError>,
    encoder_supported: Result<bool, RecorderError>,
    next_start_error: Option<RecorderError>,
    next_pause_error: Option<RecorderError>,
    next_resume_error: Option<RecorderError>,
    next_amplitude_error: Option<RecorderError>,
    next_stop_error: Option<RecorderError>,
    next_cancel_error: Option<RecorderError>,
    next_dispose_error: Option<RecorderError>,
    hold_armed: bool,
    held: Option<oneshot::Sender<Result<StoppedRecording, RecorderError>>>,
    recording: bool,
    paused: bool,
    sink: Option<ChunkSink>,
    last_config: Option<RecordConfig>,
    starts: usize,
    pauses: usize,
    resumes: usize,
    stops: usize,
    cancels: usize,
    disposals: usize,
    chunks: usize,
}

impl Default for FakeBackend {
    /// A device with a microphone, permission granted and every encoder supported.
    fn default() -> Self {
        Self::new()
    }
}

impl FakeBackend {
    /// A device with permission granted, one microphone, and every encoder supported.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                check_answer: Ok(PermissionState::Granted),
                request_answer: Ok(PermissionState::Granted),
                devices: Ok(vec![InputDevice {
                    id: "built-in-mic".to_owned(),
                    label: Some("Built-in microphone".to_owned()),
                }]),
                encoder_supported: Ok(true),
                next_start_error: None,
                next_pause_error: None,
                next_resume_error: None,
                next_amplitude_error: None,
                next_stop_error: None,
                next_cancel_error: None,
                next_dispose_error: None,
                hold_armed: false,
                held: None,
                recording: false,
                paused: false,
                sink: None,
                last_config: None,
                starts: 0,
                pauses: 0,
                resumes: 0,
                stops: 0,
                cancels: 0,
                disposals: 0,
                chunks: 0,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets what `check_permissions` answers.
    pub fn set_check_permissions(&self, answer: Result<PermissionState, RecorderError>) {
        self.lock().check_answer = answer;
    }

    /// Sets what `request_permissions` answers.
    pub fn set_request_permissions(&self, answer: Result<PermissionState, RecorderError>) {
        self.lock().request_answer = answer;
    }

    /// Sets what `list_input_devices` answers.
    pub fn set_devices(&self, answer: Result<Vec<InputDevice>, RecorderError>) {
        self.lock().devices = answer;
    }

    /// Sets what `is_encoder_supported` answers.
    pub fn set_encoder_supported(&self, answer: Result<bool, RecorderError>) {
        self.lock().encoder_supported = answer;
    }

    /// Makes the next start fail with `error`, as a platform does when it cannot record.
    pub fn fail_next_start(&self, error: RecorderError) {
        self.lock().next_start_error = Some(error);
    }

    /// Makes the next pause fail with `error`.
    pub fn fail_next_pause(&self, error: RecorderError) {
        self.lock().next_pause_error = Some(error);
    }

    /// Makes the next resume fail with `error`.
    pub fn fail_next_resume(&self, error: RecorderError) {
        self.lock().next_resume_error = Some(error);
    }

    /// Makes the next amplitude fail with `error`.
    pub fn fail_next_amplitude(&self, error: RecorderError) {
        self.lock().next_amplitude_error = Some(error);
    }

    /// Makes the next stop fail with `error`, as a platform does when it cannot flush the file.
    pub fn fail_next_stop(&self, error: RecorderError) {
        self.lock().next_stop_error = Some(error);
    }

    /// Makes the next cancel fail with `error`.
    pub fn fail_next_cancel(&self, error: RecorderError) {
        self.lock().next_cancel_error = Some(error);
    }

    /// Makes the next dispose fail with `error`.
    pub fn fail_next_dispose(&self, error: RecorderError) {
        self.lock().next_dispose_error = Some(error);
    }

    /// Makes the next stop wait for `finish_stop`, as a platform does while it flushes.
    pub fn hold_stop(&self) {
        self.lock().hold_armed = true;
    }

    /// Answers the held stop. Returns `false` when nothing waits; a cancelled caller still
    /// consumes the answer.
    pub fn finish_stop(&self, answer: Result<StoppedRecording, RecorderError>) -> bool {
        match self.lock().held.take() {
            Some(sender) => {
                let _ = sender.send(answer);
                true
            }
            None => false,
        }
    }

    /// Delivers a chunk to the sink while the recording runs. A paused or stopped recording drops
    /// it and answers `false`.
    pub fn push_chunk(&self, chunk: AudioChunk) -> bool {
        let sink = {
            let mut state = self.lock();
            if !state.recording || state.paused {
                return false;
            }
            state.chunks += 1;
            state.sink.clone()
        };
        match sink {
            // The sink runs outside the lock: it may read the fake back.
            Some(sink) => {
                sink(chunk);
                true
            }
            None => false,
        }
    }

    /// Whether a recording is running.
    pub fn recording(&self) -> bool {
        self.lock().recording
    }

    /// The config of the last start the backend saw, failed starts included.
    pub fn last_config(&self) -> Option<RecordConfig> {
        self.lock().last_config.clone()
    }

    /// How many starts succeeded.
    pub fn starts(&self) -> usize {
        self.lock().starts
    }

    /// How many pauses succeeded.
    pub fn pauses(&self) -> usize {
        self.lock().pauses
    }

    /// How many resumes succeeded.
    pub fn resumes(&self) -> usize {
        self.lock().resumes
    }

    /// How many stops were asked of the backend.
    pub fn stops(&self) -> usize {
        self.lock().stops
    }

    /// How many cancels were asked of the backend.
    pub fn cancels(&self) -> usize {
        self.lock().cancels
    }

    /// How many disposes were asked of the backend.
    pub fn disposals(&self) -> usize {
        self.lock().disposals
    }

    /// How many chunks reached the sink.
    pub fn chunks(&self) -> usize {
        self.lock().chunks
    }
}

impl Backend for FakeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        let answer = self.lock().check_answer.clone();
        Box::pin(ready(answer))
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        let answer = self.lock().request_answer.clone();
        Box::pin(ready(answer))
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        let devices = self.lock().devices.clone();
        Box::pin(ready(devices))
    }

    fn is_encoder_supported(
        &self,
        _encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        let answer = self.lock().encoder_supported.clone();
        Box::pin(ready(answer))
    }

    fn start(&self, config: &RecordConfig, sink: ChunkSink) -> Result<(), RecorderError> {
        let mut state = self.lock();
        state.last_config = Some(config.clone());
        if let Some(error) = state.next_start_error.take() {
            return Err(error);
        }
        state.starts += 1;
        state.recording = true;
        state.paused = false;
        state.sink = Some(sink);
        Ok(())
    }

    fn pause(&self) -> Result<(), RecorderError> {
        let mut state = self.lock();
        if let Some(error) = state.next_pause_error.take() {
            return Err(error);
        }
        state.pauses += 1;
        state.paused = true;
        Ok(())
    }

    fn resume(&self) -> Result<(), RecorderError> {
        let mut state = self.lock();
        if let Some(error) = state.next_resume_error.take() {
            return Err(error);
        }
        state.resumes += 1;
        state.paused = false;
        Ok(())
    }

    fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>> {
        let answer = {
            let mut state = self.lock();
            state
                .next_amplitude_error
                .take()
                .map_or(Ok(Amplitude::default()), Err)
        };
        Box::pin(ready(answer))
    }

    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        let mut state = self.lock();
        state.stops += 1;
        state.recording = false;
        state.paused = false;
        state.sink = None;
        if let Some(error) = state.next_stop_error.take() {
            return Box::pin(ready(Err(error)));
        }
        if state.hold_armed {
            state.hold_armed = false;
            let (sender, receiver) = oneshot::channel();
            state.held = Some(sender);
            // A dropped sender counts as a cancel: the held stop must still settle once.
            return Box::pin(async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err(RecorderError::new(ErrorKind::Cancelled)))
            });
        }
        Box::pin(ready(Ok(StoppedRecording {
            path: Some("recording.m4a".to_owned()),
            duration_ms: Some(0),
        })))
    }

    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        let answer = {
            let mut state = self.lock();
            state.cancels += 1;
            state.recording = false;
            state.paused = false;
            state.sink = None;
            state.next_cancel_error.take().map_or(Ok(()), Err)
        };
        Box::pin(ready(answer))
    }

    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        let answer = {
            let mut state = self.lock();
            state.disposals += 1;
            state.recording = false;
            state.paused = false;
            state.sink = None;
            state.next_dispose_error.take().map_or(Ok(()), Err)
        };
        Box::pin(ready(answer))
    }
}
