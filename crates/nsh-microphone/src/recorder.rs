//! `Recorder`: the calls an app makes, with one recording at a time and one answer per call.

use std::{
    future::{Future, ready},
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
    task::{Context, Poll},
};

use tokio::sync::oneshot;

use crate::{
    backend::{Backend, BackendFuture, ChunkSink},
    error::{ErrorKind, RecorderError},
    options::{
        Amplitude, Encoder, InputDevice, Output, PermissionState, RecordConfig, StoppedRecording,
    },
};

/// Where a recording stands; clones of one `Recorder` read the same state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingState {
    /// No recording runs; a start may claim the gate.
    #[default]
    Idle,
    /// A recording delivers chunks.
    Recording,
    /// A recording holds its position and drops chunks.
    Paused,
}

/// The recording that owns the gate. `generation` names it, so a late platform answer for a
/// cancelled recording cannot touch a newer one.
struct Active {
    generation: u64,
    paused: bool,
    stopping: bool,
}

/// The recording gate: one `Active` at a time, and the settle channel of a stop still waiting
/// on the platform.
#[derive(Default)]
struct Gate {
    next_id: u64,
    recording: Option<Active>,
    stop_waiting: Option<oneshot::Sender<RecorderError>>,
    disposed: bool,
}

struct Shared {
    backend: Arc<dyn Backend>,
    gate: Mutex<Gate>,
}

impl Shared {
    fn gate(&self) -> MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `generation` still owns the gate.
    fn owns(&self, generation: u64) -> bool {
        matches!(&self.gate().recording, Some(active) if active.generation == generation)
    }

    /// Frees the gate when `generation` still owns it; a newer owner is never touched.
    fn release(&self, generation: u64) {
        let mut gate = self.gate();
        if matches!(&gate.recording, Some(active) if active.generation == generation) {
            gate.recording = None;
        }
    }

    /// Why a claim was lost: a dispose closed the handle, or a cancel took the recording.
    fn lost_claim(&self) -> RecorderError {
        RecorderError::new(if self.gate().disposed {
            ErrorKind::NotAvailable
        } else {
            ErrorKind::Cancelled
        })
    }
}

/// Recording to a file or a stream of chunks, as Flutter's record names the calls.
///
/// Clones share one gate. [`Recorder::new`] returns the platform's shared handle, so two calls
/// in one app share it too.
#[derive(Clone)]
pub struct Recorder {
    shared: Arc<Shared>,
}

impl Recorder {
    /// The platform's recorder, with one gate for the whole process.
    pub fn new() -> Self {
        static PLATFORM: OnceLock<Recorder> = OnceLock::new();
        PLATFORM
            .get_or_init(|| Self::from_arc(crate::platform::default_backend()))
            .clone()
    }

    /// Recording through `backend`, with its own gate. Tests and fakes use this.
    pub fn with_backend(backend: impl Backend + 'static) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    fn from_arc(backend: Arc<dyn Backend>) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                gate: Mutex::new(Gate::default()),
            }),
        }
    }

    /// Where the recording stands.
    pub fn state(&self) -> RecordingState {
        match self.shared.gate().recording.as_ref() {
            None => RecordingState::Idle,
            Some(active) if active.paused => RecordingState::Paused,
            Some(_) => RecordingState::Recording,
        }
    }

    /// Whether a recording delivers chunks.
    pub fn is_recording(&self) -> bool {
        self.state() == RecordingState::Recording
    }

    /// Whether a recording holds its position and drops chunks.
    pub fn is_paused(&self) -> bool {
        self.state() == RecordingState::Paused
    }

    /// Whether recording is allowed now.
    pub fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        if self.shared.gate().disposed {
            return unavailable();
        }
        self.shared.backend.check_permissions()
    }

    /// Asks for recording to be allowed, prompting when the platform shows a prompt.
    pub fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        if self.shared.gate().disposed {
            return unavailable();
        }
        self.shared.backend.request_permissions()
    }

    /// The microphones the app could record from.
    pub fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        if self.shared.gate().disposed {
            return unavailable();
        }
        self.shared.backend.list_input_devices()
    }

    /// Whether the platform can write `encoder`.
    pub fn is_encoder_supported(
        &self,
        encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        if self.shared.gate().disposed {
            return unavailable();
        }
        self.shared.backend.is_encoder_supported(encoder)
    }

    /// Starts one recording. A bad config answers `InvalidArgument` before the gate is claimed;
    /// an undetermined permission is requested once; a busy gate answers `InProgress` and an
    /// unsupported encoder `FormatNotSupported`.
    pub fn start<'a>(
        &'a self,
        config: &'a RecordConfig,
        sink: ChunkSink,
    ) -> BackendFuture<'a, Result<(), RecorderError>> {
        Box::pin(async move {
            if let Some(error) = config_refusal(config) {
                return Err(error);
            }
            if self.shared.gate().disposed {
                return Err(RecorderError::new(ErrorKind::NotAvailable));
            }
            let permission = match self.shared.backend.check_permissions().await {
                Ok(PermissionState::Undetermined) => {
                    self.shared.backend.request_permissions().await
                }
                answer => answer,
            }?;
            if permission != PermissionState::Granted {
                return Err(RecorderError::new(ErrorKind::PermissionDenied));
            }
            let generation = {
                let mut gate = self.shared.gate();
                if gate.disposed {
                    return Err(RecorderError::new(ErrorKind::NotAvailable));
                }
                if gate.recording.is_some() {
                    return Err(RecorderError::new(ErrorKind::InProgress));
                }
                let generation = gate.next_id;
                gate.next_id += 1;
                gate.recording = Some(Active {
                    generation,
                    paused: false,
                    stopping: false,
                });
                generation
            };
            let supported = match self
                .shared
                .backend
                .is_encoder_supported(config.encoder)
                .await
            {
                Ok(supported) => supported,
                Err(error) => {
                    self.shared.release(generation);
                    return Err(error);
                }
            };
            if !supported {
                self.shared.release(generation);
                return Err(RecorderError::new(ErrorKind::FormatNotSupported));
            }
            if !self.shared.owns(generation) {
                // A cancel or a dispose ended this start while it asked the platform.
                return Err(self.shared.lost_claim());
            }
            if let Err(error) = self.shared.backend.start(config, sink) {
                self.shared.release(generation);
                return Err(error);
            }
            if !self.shared.owns(generation) {
                // The gate was lost after the platform started; the orphan must not survive it.
                let _ = self.shared.backend.cancel().await;
                return Err(self.shared.lost_claim());
            }
            Ok(())
        })
    }

    /// Pauses the running recording; chunks are dropped while it stays paused. Any other state
    /// answers `InvalidState`.
    pub fn pause(&self) -> Result<(), RecorderError> {
        let generation = {
            let mut gate = self.shared.gate();
            if gate.disposed {
                return Err(RecorderError::new(ErrorKind::NotAvailable));
            }
            let Some(active) = gate.recording.as_ref() else {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            };
            if active.paused {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            }
            active.generation
        };
        self.shared.backend.pause()?;
        let mut gate = self.shared.gate();
        if let Some(active) = gate.recording.as_mut() {
            // A stop, a cancel or a second pause may have moved on while the platform paused.
            if active.generation == generation && !active.paused {
                active.paused = true;
            }
        }
        Ok(())
    }

    /// Resumes a paused recording. Any other state answers `InvalidState`.
    pub fn resume(&self) -> Result<(), RecorderError> {
        let generation = {
            let mut gate = self.shared.gate();
            if gate.disposed {
                return Err(RecorderError::new(ErrorKind::NotAvailable));
            }
            let Some(active) = gate.recording.as_ref() else {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            };
            if !active.paused {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            }
            active.generation
        };
        self.shared.backend.resume()?;
        let mut gate = self.shared.gate();
        if let Some(active) = gate.recording.as_mut() {
            if active.generation == generation && active.paused {
                active.paused = false;
            }
        }
        Ok(())
    }

    /// The loudness of the running or paused recording. From idle it answers `InvalidState`.
    pub fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>> {
        Box::pin(async move {
            {
                let gate = self.shared.gate();
                if gate.disposed {
                    return Err(RecorderError::new(ErrorKind::NotAvailable));
                }
                if gate.recording.is_none() {
                    return Err(RecorderError::new(ErrorKind::InvalidState));
                }
            }
            self.shared.backend.amplitude().await
        })
    }

    /// Stops the recording and resolves with what it left behind. From idle, or while another
    /// stop ends the recording, it answers `InvalidState`. A cancel settles a pending stop as
    /// `Cancelled`, and the platform's late answer then settles nothing.
    pub fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        Box::pin(async move {
            let (generation, cancelled) = {
                let mut gate = self.shared.gate();
                if gate.disposed {
                    return Err(RecorderError::new(ErrorKind::NotAvailable));
                }
                let Some(active) = gate.recording.as_mut() else {
                    return Err(RecorderError::new(ErrorKind::InvalidState));
                };
                if active.stopping {
                    return Err(RecorderError::new(ErrorKind::InvalidState));
                }
                active.stopping = true;
                let generation = active.generation;
                let (sender, receiver) = oneshot::channel();
                gate.stop_waiting = Some(sender);
                (generation, receiver)
            };
            let race = StopRace {
                backend: self.shared.backend.stop(),
                cancelled,
            };
            match race.await {
                StopAnswer::Backend(answer) => {
                    let still_ours = {
                        let mut gate = self.shared.gate();
                        if matches!(&gate.recording, Some(active) if active.generation == generation)
                        {
                            gate.stop_waiting = None;
                            if answer.is_ok() {
                                gate.recording = None;
                            } else if let Some(active) = gate.recording.as_mut() {
                                // A failed stop leaves the recording running: stop again or cancel.
                                active.stopping = false;
                            }
                            true
                        } else {
                            false
                        }
                    };
                    if still_ours {
                        answer
                    } else {
                        Err(self.shared.lost_claim())
                    }
                }
                // Only a cancel or a dispose settles a waiting stop early.
                StopAnswer::Cancelled(error) => Err(error),
            }
        })
    }

    /// Ends the recording and discards its output. The gate frees whatever the platform's
    /// cancel answers, so nothing hangs; a pending stop settles `Cancelled`.
    pub fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        Box::pin(async move {
            let waiting = {
                let mut gate = self.shared.gate();
                if gate.disposed {
                    return Err(RecorderError::new(ErrorKind::NotAvailable));
                }
                if gate.recording.is_none() {
                    return Err(RecorderError::new(ErrorKind::InvalidState));
                }
                gate.recording = None;
                gate.stop_waiting.take()
            };
            if let Some(sender) = waiting {
                let _ = sender.send(RecorderError::new(ErrorKind::Cancelled));
            }
            self.shared.backend.cancel().await
        })
    }

    /// Ends the handle: a running recording is cancelled first, the backend's dispose runs once,
    /// and every later call answers `NotAvailable`.
    pub fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        Box::pin(async move {
            let recording = {
                let mut gate = self.shared.gate();
                if gate.disposed {
                    return Err(RecorderError::new(ErrorKind::NotAvailable));
                }
                gate.disposed = true;
                if let Some(sender) = gate.stop_waiting.take() {
                    let _ = sender.send(RecorderError::new(ErrorKind::Cancelled));
                }
                gate.recording.take().is_some()
            };
            // The backend's dispose must run once even when its cancel fails.
            let cancel_error = if recording {
                self.shared.backend.cancel().await.err()
            } else {
                None
            };
            match (cancel_error, self.shared.backend.dispose().await) {
                (Some(error), _) => Err(error),
                (None, answer) => answer,
            }
        })
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Which side of a stop's wait finished: the platform's answer, or the cancel that ended it.
enum StopAnswer {
    Backend(Result<StoppedRecording, RecorderError>),
    Cancelled(RecorderError),
}

/// A stop's wait, raced by hand: tokio's `select!` sits behind the `macros` feature, which the
/// lib does not take. Both fields are `Unpin`, so polling needs no pinning gymnastics.
struct StopRace<'a> {
    backend: BackendFuture<'a, Result<StoppedRecording, RecorderError>>,
    cancelled: oneshot::Receiver<RecorderError>,
}

impl Future for StopRace<'_> {
    type Output = StopAnswer;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<StopAnswer> {
        let this = self.get_mut();
        if let Poll::Ready(answer) = this.backend.as_mut().poll(cx) {
            return Poll::Ready(StopAnswer::Backend(answer));
        }
        // A dropped sender counts as a cancel, as a settled-once stop leaves nothing to answer.
        match Pin::new(&mut this.cancelled).poll(cx) {
            Poll::Ready(Ok(error)) => Poll::Ready(StopAnswer::Cancelled(error)),
            Poll::Ready(Err(_)) => Poll::Ready(StopAnswer::Cancelled(RecorderError::new(
                ErrorKind::Cancelled,
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The start's refusals, in the contract's order: rate, channels, bit rate, then the file path.
fn config_refusal(config: &RecordConfig) -> Option<RecorderError> {
    let pcm = matches!(config.encoder, Encoder::Pcm16bits | Encoder::Wav);
    let invalid = config.sample_rate == 0
        || config.channels == 0
        || config.channels > 2
        || (config.bit_rate == 0 && !pcm)
        || matches!(&config.output, Output::File { path } if path.trim().is_empty());
    invalid.then(|| RecorderError::new(ErrorKind::InvalidArgument))
}

/// The answer every call on a disposed handle gets.
fn unavailable<T>() -> BackendFuture<'static, Result<T, RecorderError>> {
    Box::pin(ready(Err(RecorderError::new(ErrorKind::NotAvailable))))
}
