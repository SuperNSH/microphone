//! Apple (iOS and macOS): an `AVAudioEngine` input tap captures at the hardware rate and this
//! layer resamples; iOS runs a session that pauses on interruption and rebuilds on route change.

use std::{
    future::ready,
    ptr::NonNull,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
};

use block2::RcBlock;
use objc2::{msg_send, rc::Retained};
use objc2_avf_audio::{
    AVAudioEngine, AVAudioFormat, AVAudioFrameCount, AVAudioInputNode, AVAudioNodeBus,
    AVAudioPCMBuffer, AVAudioTime,
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

#[cfg(not(target_os = "macos"))]
use objc2::{
    RefEncode, class,
    rc::autoreleasepool,
    runtime::{AnyObject, Bool},
};

#[cfg(not(target_os = "macos"))]
use tokio::sync::oneshot;

#[cfg(not(target_os = "macos"))]
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryPlayAndRecord, AVAudioSessionInterruptionNotification,
    AVAudioSessionRecordPermission, AVAudioSessionRouteChangeNotification,
};

pub(crate) struct NativeBackend;

/// The input node's single bus; the tap's frame request, which the node rounds to its schedule.
const BUS: AVAudioNodeBus = 0;
const TAP_FRAMES: AVAudioFrameCount = 4096;

/// One recording's engine and input node. The bindings mark both `!Send` out of Apple thread
/// caution; the recording lock serializes every access, so the handles never run in parallel.
struct Graph {
    engine: Retained<AVAudioEngine>,
    node: Retained<AVAudioInputNode>,
}
// SAFETY: the graph only ever lives behind the recording lock, which serializes all uses.
unsafe impl Send for Graph {}

/// The notification token that unregisters an observer; the center accepts it from any thread.
#[cfg(not(target_os = "macos"))]
struct ObserverToken(Retained<AnyObject>);
// SAFETY: the token is an opaque registration handle used only for removeObserver:, never aliased.
#[cfg(not(target_os = "macos"))]
unsafe impl Send for ObserverToken {}

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

/// What the tap callback writes and the API reads: samples, meters, the drop flags.
struct TapData {
    sink: ChunkSink,
    file: bool,
    rate: u32,
    channels: u16,
    paused: bool,
    interrupted: bool,
    samples: Vec<i16>,
    frames: u64,
    average: f32,
    peak: f32,
    resample: Resampler,
    frame: Vec<f32>,
}

impl TapData {
    fn new(config: &RecordConfig, sink: ChunkSink) -> Self {
        Self {
            sink,
            file: matches!(config.output, Output::File { .. }),
            rate: config.sample_rate,
            channels: config.channels,
            paused: false,
            interrupted: false,
            samples: Vec::new(),
            frames: 0,
            average: 0.0,
            peak: 0.0,
            resample: Resampler::new(0.0, config.sample_rate),
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

/// One recording in flight.
struct Active {
    graph: Graph,
    tap: Arc<Mutex<TapData>>,
    config: RecordConfig,
    path: Option<String>,
    #[cfg(not(target_os = "macos"))]
    observers: Vec<ObserverToken>,
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

/// Whether a microphone is wired up; a Mac with no input presents a zero-channel format. The
/// Mac has no session permission API in scope, so this probe is its whole permission story.
#[cfg(target_os = "macos")]
fn has_input() -> bool {
    let engine = unsafe { AVAudioEngine::new() };
    let node = unsafe { engine.inputNode() };
    (unsafe { node.outputFormatForBus(BUS).channelCount() }) > 0
}

/// The current record permission. The session API is deprecated in favor of AVAudioApplication
/// but answers on every iOS version this crate supports.
#[cfg(not(target_os = "macos"))]
#[allow(deprecated)]
fn session_permission() -> PermissionState {
    let value = unsafe { AVAudioSession::sharedInstance().recordPermission() };
    match value {
        AVAudioSessionRecordPermission::Granted => PermissionState::Granted,
        AVAudioSessionRecordPermission::Denied => PermissionState::Denied,
        _ => PermissionState::Undetermined,
    }
}

/// Category PlayAndRecord, then activation; Apple requires an active session before the engine
/// starts.
#[cfg(not(target_os = "macos"))]
fn activate_session() -> Result<(), RecorderError> {
    let session = unsafe { AVAudioSession::sharedInstance() };
    let category = match unsafe { AVAudioSessionCategoryPlayAndRecord } {
        Some(category) => category,
        // None means the symbol never linked, so no session work can happen on this build.
        None => return Err(RecorderError::new(ErrorKind::Backend)),
    };
    unsafe { session.setCategory_error(category) }.map_err(|error| {
        RecorderError::new(ErrorKind::Backend).with_status(i64::from(error.code()))
    })?;
    unsafe { session.setActive_error(true) }.map_err(|error| {
        RecorderError::new(ErrorKind::Backend).with_status(i64::from(error.code()))
    })?;
    Ok(())
}

/// Best-effort deactivation: another audio user can hold the session on, and the recording's
/// result is already decided by the time this runs.
#[cfg(not(target_os = "macos"))]
fn deactivate_session() {
    let session = unsafe { AVAudioSession::sharedInstance() };
    let _ = unsafe { session.setActive_error(false) };
}

/// The session's input ports as devices.
#[cfg(not(target_os = "macos"))]
fn input_devices() -> Vec<InputDevice> {
    autoreleasepool(|pool| {
        let session = unsafe { AVAudioSession::sharedInstance() };
        let Some(ports) = (unsafe { session.availableInputs() }) else {
            return Vec::new();
        };
        let mut devices = Vec::with_capacity(ports.count());
        for index in 0..ports.count() {
            let port = ports.objectAtIndex(index);
            let uid = unsafe { port.UID() };
            let name = unsafe { port.portName() };
            devices.push(InputDevice {
                id: unsafe { uid.to_str(pool) }.to_owned(),
                label: Some(unsafe { name.to_str(pool) }.to_owned()),
            });
        }
        devices
    })
}

/// Builds the engine graph and installs the tap. Fails with `NotAvailable` when the machine
/// has no input, and with the OS status when the engine refuses to start.
fn build_graph(tap: &Arc<Mutex<TapData>>) -> Result<Graph, RecorderError> {
    let engine = unsafe { AVAudioEngine::new() };
    let node = unsafe { engine.inputNode() };
    // The node's own processing format is the only one an input tap accepts without raising.
    let format = unsafe { node.outputFormatForBus(BUS) };
    let channels = unsafe { format.channelCount() };
    if channels == 0 {
        return Err(RecorderError::new(ErrorKind::NotAvailable));
    }
    {
        let mut data = tap.lock().unwrap_or_else(PoisonError::into_inner);
        // Only the resampler follows the current hardware rate; the buffer layout travels with
        // the tap block, so a rebuild never re-describes the old tap's buffers.
        data.resample = Resampler::new(unsafe { format.sampleRate() }, data.rate);
    }
    install_tap(&node, &format, tap.clone());
    // The OS status (an NSError code) rides along; NSError's type is never named because
    // this crate does not depend on objc2-foundation directly.
    unsafe { engine.startAndReturnError() }.map_err(|error| {
        RecorderError::new(ErrorKind::Backend).with_status(i64::from(error.code()))
    })?;
    Ok(Graph { engine, node })
}

/// Installs the input tap: the block is heap-allocated because the node keeps its own copy.
fn install_tap(node: &AVAudioInputNode, format: &AVAudioFormat, tap: Arc<Mutex<TapData>>) {
    let hw_channels = u16::try_from(unsafe { format.channelCount() }).unwrap_or(u16::MAX);
    let interleaved = unsafe { format.isInterleaved() };
    let block = RcBlock::new(
        move |buffer: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
            deliver(unsafe { buffer.as_ref() }, &tap, hw_channels, interleaved);
        },
    );
    // SAFETY: the selector and block signature match AVAudioNodeTapBlock; the format is the
    // node's own, which the engine accepts for an input tap.
    unsafe {
        let _: () = msg_send![
            node,
            installTapOnBus: BUS,
            bufferSize: TAP_FRAMES,
            format: format,
            usingBlock: &*block
        ];
    }
}

/// Reads one tap buffer into the shared tap state; the sink runs after the lock is gone so a
/// callback that re-enters the recorder cannot deadlock the audio thread.
fn deliver(buffer: &AVAudioPCMBuffer, tap: &Mutex<TapData>, hw_channels: u16, interleaved: bool) {
    let (sink, chunk) = {
        let mut data = tap.lock().unwrap_or_else(PoisonError::into_inner);
        if data.paused || data.interrupted {
            return;
        }
        let frames = unsafe { buffer.frameLength() } as usize;
        let channels = unsafe { buffer.floatChannelData() };
        // A non-float tap format would leave the channel data null; nothing to read then.
        if frames == 0 || channels.is_null() {
            return;
        }
        let stride = unsafe { buffer.stride() };
        let hw = usize::from(hw_channels);
        let mut out = Vec::with_capacity(frames * usize::from(data.channels.max(1)));
        let mut hw_frame = Vec::with_capacity(hw);
        for frame in 0..frames {
            hw_frame.clear();
            for channel in 0..hw {
                // Interleaved data sits behind channel pointer 0; planar data behind its own.
                let (slot, offset) = if interleaved {
                    (0, channel)
                } else {
                    (channel, 0)
                };
                // SAFETY: the tap format fixes the channel count and stride, so both indexes
                // stay inside the buffer for every frame the callback hands over.
                let sample =
                    unsafe { *(*channels.add(slot)).as_ptr().add(frame * stride + offset) };
                hw_frame.push(sample);
            }
            data.push_frame(&hw_frame, &mut out);
        }
        let chunk = AudioChunk {
            data: out,
            sample_rate: data.rate,
            channels: data.channels,
        };
        data.frames += u64::try_from(chunk.data.len() / usize::from(chunk.channels.max(1)))
            .unwrap_or(u64::MAX);
        if data.file {
            data.samples.extend_from_slice(&chunk.data);
        }
        (data.sink.clone(), chunk)
    };
    sink(chunk);
}

/// Registers the session observers; every observer is removed when the recording ends.
#[cfg(not(target_os = "macos"))]
fn observe() -> Vec<ObserverToken> {
    let mut observers = Vec::new();
    if let Some(name) = unsafe { AVAudioSessionInterruptionNotification } {
        observers.extend(add_observer(name, on_interruption));
    }
    if let Some(name) = unsafe { AVAudioSessionRouteChangeNotification } {
        observers.extend(add_observer(name, on_route_change));
    }
    observers
}

/// One block-based observer on the default notification center.
#[cfg(not(target_os = "macos"))]
fn add_observer<T: RefEncode + ?Sized>(name: &T, act: fn()) -> Option<ObserverToken> {
    let block = RcBlock::new(move |_note: NonNull<AnyObject>| {
        act();
    });
    // SAFETY: the center copies the block and hands back a token removeObserver: accepts.
    let center: Retained<AnyObject> =
        unsafe { msg_send![class!(NSNotificationCenter), defaultCenter] };
    let token: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            &*center,
            addObserverForName: name,
            object: Option::<&AnyObject>::None,
            queue: Option::<&AnyObject>::None,
            usingBlock: &*block
        ]
    };
    token.map(ObserverToken)
}

#[cfg(not(target_os = "macos"))]
fn remove_observers(tokens: &[ObserverToken]) {
    if tokens.is_empty() {
        return;
    }
    // SAFETY: defaultCenter is the same singleton the tokens came from.
    let center: Retained<AnyObject> =
        unsafe { msg_send![class!(NSNotificationCenter), defaultCenter] };
    for token in tokens {
        // SAFETY: the token only ever names this observer.
        unsafe {
            let _: () = msg_send![&*center, removeObserver: &*token.0];
        }
    }
}

/// A call interruption ends the session's audio; delivery stops and stays stopped until the
/// app starts a new recording, per the brief's restart-explicitly rule.
#[cfg(not(target_os = "macos"))]
fn on_interruption() {
    let mut guard = lock_shared();
    if let Phase::Active(active) = &mut guard.phase {
        active
            .tap
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .interrupted = true;
    }
}

/// A route change (headphones, Bluetooth) invalidates the graph; rebuild it around the same
/// tap. A failed rebuild stops delivery rather than losing what was captured.
#[cfg(not(target_os = "macos"))]
fn on_route_change() {
    let mut guard = lock_shared();
    if let Phase::Active(active) = &mut guard.phase {
        // One graph delivers at a time: the old engine stops before the replacement builds.
        unsafe { active.graph.engine.stop() };
        let tap = active.tap.clone();
        match build_graph(&tap) {
            Ok(graph) => {
                std::mem::replace(&mut active.graph, graph);
            }
            Err(_) => {
                active
                    .tap
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .interrupted = true;
            }
        }
    }
}

/// Ends a recording: tap off, observers off, engine stopped, session released. The lock order
/// is always recording state first, tap second; the tap callback never takes the first lock.
fn teardown(active: &mut Active) {
    #[cfg(not(target_os = "macos"))]
    remove_observers(&active.observers);
    unsafe { active.graph.node.removeTapOnBus(BUS) };
    unsafe { active.graph.engine.stop() };
    #[cfg(not(target_os = "macos"))]
    deactivate_session();
}

/// Tears the recording down and answers what it left behind. The phase swap happens first, so
/// the call resolves exactly once however the file write goes.
fn finish(discard: bool) -> Result<StoppedRecording, RecorderError> {
    let mut guard = lock_shared();
    match std::mem::replace(&mut guard.phase, Phase::Idle) {
        Phase::Active(mut active) => {
            teardown(&mut active);
            if discard {
                // Cancel keeps nothing: the samples live only in memory until stop writes them.
                return Ok(StoppedRecording::default());
            }
            let data = active.tap.lock().unwrap_or_else(PoisonError::into_inner);
            match active.path {
                Some(path) => write_wav(
                    &path,
                    active.config.sample_rate,
                    active.config.channels,
                    &data.samples,
                ),
                None => Ok(StoppedRecording {
                    path: None,
                    duration_ms: Some(millis(data.frames, active.config.sample_rate)),
                }),
            }
        }
        // Disposed stays disposed: the swap above was only a peek at the phase.
        Phase::Disposed => {
            guard.phase = Phase::Disposed;
            Err(RecorderError::new(ErrorKind::NotAvailable))
        }
        Phase::Idle => Err(RecorderError::new(ErrorKind::InvalidState)),
    }
}

/// Pause and resume flip the tap's flag; the tap drops its buffers while it is set.
fn set_paused(paused: bool) -> Result<(), RecorderError> {
    let mut guard = lock_shared();
    match &mut guard.phase {
        Phase::Active(active) => {
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

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        #[cfg(not(target_os = "macos"))]
        let answer = Ok(session_permission());
        #[cfg(target_os = "macos")]
        let answer = if has_input() {
            Ok(PermissionState::Granted)
        } else {
            Err(RecorderError::new(ErrorKind::NotAvailable))
        };
        Box::pin(ready(answer))
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        Box::pin(async move {
            #[cfg(not(target_os = "macos"))]
            {
                let (sender, receiver) = oneshot::channel::<PermissionState>();
                let block = RcBlock::new(move |granted: Bool| {
                    let state = if bool::from(granted) {
                        PermissionState::Granted
                    } else {
                        PermissionState::Denied
                    };
                    // oneshot send never blocks, so the handler returns fast on any queue.
                    let _ = sender.send(state);
                });
                let session = unsafe { AVAudioSession::sharedInstance() };
                // iOS calls the handler exactly once, immediately when permission is decided.
                // SAFETY: the block matches the requestRecordPermission: handler signature.
                unsafe {
                    let _: () = msg_send![&*session, requestRecordPermission: &*block];
                }
                // Awaiting the oneshot pends this future without wedging its polling thread.
                receiver
                    .await
                    .map_err(|_| RecorderError::new(ErrorKind::Backend))
            }
            #[cfg(target_os = "macos")]
            {
                // The Mac has no prompt to show; TCC asks on first use of the hardware.
                if has_input() {
                    Ok(PermissionState::Granted)
                } else {
                    Err(RecorderError::new(ErrorKind::NotAvailable))
                }
            }
        })
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        #[cfg(not(target_os = "macos"))]
        let devices = input_devices();
        // The Mac lists the single default input; picking among inputs is v2 work.
        #[cfg(target_os = "macos")]
        let devices = if has_input() {
            vec![InputDevice {
                id: "default-input".to_owned(),
                label: Some("Default input".to_owned()),
            }]
        } else {
            Vec::new()
        };
        Box::pin(ready(Ok(devices)))
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
        #[cfg(not(target_os = "macos"))]
        activate_session()?;
        let tap = Arc::new(Mutex::new(TapData::new(config, sink)));
        let graph = match build_graph(&tap) {
            Ok(graph) => graph,
            Err(error) => {
                // The session was activated above; a failed start leaves nothing to keep it on.
                #[cfg(not(target_os = "macos"))]
                deactivate_session();
                return Err(error);
            }
        };
        #[cfg(not(target_os = "macos"))]
        let observers = observe();
        guard.phase = Phase::Active(Active {
            graph,
            tap,
            config: config.clone(),
            path,
            #[cfg(not(target_os = "macos"))]
            observers,
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
                // A revoked permission can deliver silence instead of an error; these meters
                // are how the app sees it.
                Ok(Amplitude {
                    average: data.average,
                    peak: data.peak,
                })
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
        if let Phase::Active(mut active) = std::mem::replace(&mut guard.phase, Phase::Disposed) {
            teardown(&mut active);
        }
        drop(guard);
        Box::pin(ready(Ok(())))
    }
}
