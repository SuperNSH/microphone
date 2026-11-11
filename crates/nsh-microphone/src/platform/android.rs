//! Android: `AudioRecord` reads PCM16 over JNI, through the VM and app context `ndk-context`
//! carries.
//!
//! The app's manifest must declare `android.permission.RECORD_AUDIO`, and API 34+ additionally
//! wants the `microphone` foreground-service type on any service that records while backgrounded;
//! both are app requirements this library cannot grant. Runtime permissions need API 23+ — below
//! that RECORD_AUDIO is granted at install.
//!
//! v1 records PCM16 only: `Wav` and `Pcm16bits` answer supported, compressed encoders do not.
//! The configured rate is requested from the hardware and a carried linear resample delivers
//! chunks at that rate when the hardware chose another. A file output accumulates in memory and
//! `write_wav` writes it on stop.

use std::{
    ffi::{c_char, c_int, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use jni::{
    Env, JValue, jni_sig, jni_str,
    objects::{JObject, JObjectArray, JShortArray, JString},
    sys::{jint, jshort, jsize},
    vm::JavaVM,
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

/// `MediaRecorder.AudioSource.MIC`; the contract pins it — record itself defaults to DEFAULT.
const AUDIO_SOURCE_MIC: jint = 1;
/// `AudioFormat.CHANNEL_IN_MONO` / `CHANNEL_IN_STEREO`.
const CHANNEL_IN_MONO: jint = 16;
const CHANNEL_IN_STEREO: jint = 12;
/// `AudioFormat.ENCODING_PCM_16BIT`.
const ENCODING_PCM_16BIT: jint = 2;
/// `AudioRecord.STATE_INITIALIZED`.
const STATE_INITIALIZED: jint = 1;
/// `AudioRecord.RECORDSTATE_RECORDING`.
const RECORDSTATE_RECORDING: jint = 3;
/// `AudioRecord.ERROR_BAD_VALUE`; the other negatives are generic framework refusals.
const ERROR_BAD_VALUE: jint = -2;
/// `PackageManager.PERMISSION_GRANTED`.
const PERMISSION_GRANTED: jint = 0;
/// `AudioManager.GET_DEVICES_INPUTS`.
const GET_DEVICES_INPUTS: jint = 1;
/// Runtime permissions exist from API 23; below it RECORD_AUDIO is granted at install.
const SDK_WITH_RUNTIME_PERMISSIONS: jint = 23;
/// record's own request code for RECORD_AUDIO.
const REQUEST_CODE: jint = 1001;
const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

/// How long `start` waits for the reader to build the `AudioRecord` before failing.
const START_WINDOW: Duration = Duration::from_secs(5);
/// How long a stop waits for the reader to finish before it detaches and fails.
const JOIN_WINDOW: Duration = Duration::from_secs(3);
/// How long a permission request watches the framework for the dialog's answer.
const REQUEST_WINDOW: Duration = Duration::from_secs(30);
/// Every wait between reads is this bounded poll; no call blocks forever.
const POLL: Duration = Duration::from_millis(20);
const REQUEST_POLL: Duration = Duration::from_millis(250);

/// `JNI_GetCreatedJavaVMs`, found by name in `libnativehelper`.
type GetCreatedVms = unsafe extern "C" fn(*mut *mut jni::sys::JavaVM, jsize, *mut jsize) -> jint;

// bionic's libdl: the two calls `libc` would make, so the crate needs no `libc` dependency.
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_NOW` on bionic: resolve everything up front so a failure surfaces here, not later.
const RTLD_NOW: c_int = 2;

/// Set once `ndk-context` holds the VM and the application; it can be set only once.
static CONTEXT: Mutex<bool> = Mutex::new(false);

/// The one recording this backend runs, process-wide; `Recorder` hands out a single shared handle.
static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);

/// Locks `mutex`; a poisoned lock is taken back rather than propagated, so no call path panics.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl From<jni::errors::Error> for RecorderError {
    fn from(error: jni::errors::Error) -> Self {
        // A caught Java exception carries its text but no numeric code; ThrowFailed is the one
        // that does. The audio layer's own statuses are attached where they occur.
        match error {
            jni::errors::Error::ThrowFailed(status) => {
                RecorderError::new(ErrorKind::Backend).with_status(i64::from(status))
            }
            _ => RecorderError::new(ErrorKind::Backend),
        }
    }
}

/// The VM of this process. An NSH app has no JVM of its own, so it is found through the runtime's
/// library, as the biometrics crate finds it.
fn find_vm() -> Result<*mut jni::sys::JavaVM, RecorderError> {
    // SAFETY: the names are valid C strings, and the symbol is called with the signature the JNI
    // specification gives it.
    unsafe {
        let library = dlopen(c"libnativehelper.so".as_ptr(), RTLD_NOW);
        if library.is_null() {
            return Err(RecorderError::new(ErrorKind::Backend));
        }
        let symbol = dlsym(library, c"JNI_GetCreatedJavaVMs".as_ptr());
        if symbol.is_null() {
            return Err(RecorderError::new(ErrorKind::Backend));
        }
        let get: GetCreatedVms = std::mem::transmute::<*mut c_void, GetCreatedVms>(symbol);
        let mut vm: *mut jni::sys::JavaVM = std::ptr::null_mut();
        let mut count: jsize = 0;
        if get(&mut vm, 1, &mut count) != 0 || count < 1 || vm.is_null() {
            return Err(RecorderError::new(ErrorKind::Backend));
        }
        Ok(vm)
    }
}

/// Runs `work` on this thread, attached to the VM the process already runs.
fn with_vm<R>(
    vm: *mut jni::sys::JavaVM,
    work: impl FnOnce(&mut Env<'_>) -> Result<R, RecorderError>,
) -> Result<R, RecorderError> {
    if vm.is_null() {
        return Err(RecorderError::new(ErrorKind::Backend));
    }
    // SAFETY: the pointer came from the runtime's own `JNI_GetCreatedJavaVMs` or from
    // `ndk-context`, and the VM lives as long as the process.
    let vm = unsafe { JavaVM::from_raw(vm) };
    vm.attach_current_thread(work)
}

/// Gives `ndk-context` the VM and the application, once — NSH does not start `ndk-context`
/// itself. A context another runtime installed already is adopted untouched.
fn ensure_context() -> Result<(), RecorderError> {
    let mut ready = lock(&CONTEXT);
    if *ready {
        return Ok(());
    }
    // `ndk-context` has no try-get and asserts on a second initialization, and its getter panics
    // while nothing is installed, so the probe is the only way to tell an existing context apart.
    // The probe panics once on a fresh process; a silent hook keeps that out of the log.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let probe = catch_unwind(AssertUnwindSafe(ndk_context::android_context));
    std::panic::set_hook(previous);
    if probe.is_ok() {
        *ready = true;
        return Ok(());
    }
    let vm = find_vm()?;
    let context = with_vm(vm, |env| {
        let application = env
            .call_static_method(
                jni_str!("android/app/ActivityThread"),
                jni_str!("currentApplication"),
                jni_sig!("()Landroid/app/Application;"),
                &[],
            )?
            .l()?;
        if application.is_null() {
            return Err(RecorderError::new(ErrorKind::Backend));
        }
        // The global reference lives as long as the process, which is what `ndk-context` assumes.
        let global = env.new_global_ref(application)?;
        Ok::<_, RecorderError>(global.into_raw().cast::<c_void>())
    })?;
    // SAFETY: the VM and the application are the process's own, and the probe showed nothing is
    // installed.
    unsafe { ndk_context::initialize_android_context(vm.cast(), context) };
    *ready = true;
    Ok(())
}

/// Runs `work` on this thread with the context installed and attached.
fn attached<R>(
    work: impl FnOnce(&mut Env<'_>) -> Result<R, RecorderError>,
) -> Result<R, RecorderError> {
    ensure_context()?;
    let vm = ndk_context::android_context().vm();
    with_vm(vm.cast(), work)
}

/// Runs `work` on a fresh attached thread and answers through a oneshot, so the caller's executor
/// never blocks on JNI or a poll window. A spawn failure or a lost thread answers Backend.
fn dispatched<R: Send + 'static>(
    work: impl FnOnce(&mut Env<'_>) -> Result<R, RecorderError> + Send + 'static,
) -> BackendFuture<'static, Result<R, RecorderError>> {
    let (answered, listening) = tokio::sync::oneshot::channel();
    let spawned = thread::Builder::new()
        .name("nsh-microphone-android-query".to_owned())
        .spawn(move || {
            let answer = attached(work);
            let _ = answered.send(answer);
        });
    Box::pin(async move {
        if spawned.is_err() {
            return Err(RecorderError::new(ErrorKind::Backend));
        }
        listening
            .await
            .map_err(|_| RecorderError::new(ErrorKind::Backend))
    })
}

/// The application `Context` `ndk-context` carries; the pointer is a global ref it owns.
fn app_context(env: &mut Env<'_>) -> Result<JObject<'_>, RecorderError> {
    let context = ndk_context::android_context().context();
    if context.is_null() {
        return Err(RecorderError::new(ErrorKind::Backend));
    }
    // SAFETY: a live global ref held by `ndk-context` for the process's lifetime; `JObject` never
    // frees a reference, so nothing deletes it here.
    Ok(unsafe { JObject::from_raw(env, context.cast()) })
}

/// `Build.VERSION.SDK_INT`.
fn sdk_int(env: &mut Env<'_>) -> Result<jint, RecorderError> {
    Ok(env
        .get_static_field(
            jni_str!("android/os/Build$VERSION"),
            jni_str!("SDK_INT"),
            jni_sig!("I"),
        )?
        .i()?)
}

/// A system service by name, such as `audio` for `AudioManager`.
fn system_service<'local>(
    env: &mut Env<'local>,
    context: &JObject<'_>,
    name: &str,
) -> Result<JObject<'local>, RecorderError> {
    let name: JObject<'_> = JString::from_str(env, name)?.into();
    Ok(env
        .call_method(
            context,
            jni_str!("getSystemService"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&name)],
        )?
        .l()?)
}

/// The resumed activity, walked out of `ActivityThread`'s private record of them — the same
/// hidden API robius walks for biometric prompts. `None` on any failure; callers degrade, not fail.
fn current_activity(env: &mut Env<'_>) -> Option<JObject<'_>> {
    let walked = (|| {
        let thread = env
            .call_static_method(
                jni_str!("android/app/ActivityThread"),
                jni_str!("currentActivityThread"),
                jni_sig!("()Landroid/app/ActivityThread;"),
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        let records = env
            .get_field(
                &thread,
                jni_str!("mActivities"),
                jni_sig!("Landroid/util/ArrayMap;"),
            )
            .ok()?
            .l()
            .ok()?;
        let values = env
            .call_method(
                &records,
                jni_str!("values"),
                jni_sig!("()Ljava/util/Collection;"),
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        let iterator = env
            .call_method(
                &values,
                jni_str!("iterator"),
                jni_sig!("()Ljava/util/Iterator;"),
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        let mut paused = None;
        while env
            .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])
            .ok()?
            .z()
            .ok()?
        {
            let record = env
                .call_method(
                    &iterator,
                    jni_str!("next"),
                    jni_sig!("()Ljava/lang/Object;"),
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            // One record failing its field reads fails the walk; the hidden API works for all
            // records or none.
            let activity = env
                .get_field(
                    &record,
                    jni_str!("activity"),
                    jni_sig!("Landroid/app/Activity;"),
                )
                .and_then(|value| value.l())
                .ok()?;
            if activity.is_null() {
                continue;
            }
            let paused_now = env
                .get_field(&record, jni_str!("paused"), jni_sig!("Z"))
                .and_then(|value| value.z())
                .ok()?;
            if !paused_now {
                return Some(activity);
            }
            paused.get_or_insert(activity);
        }
        paused
    })();
    if walked.is_none() {
        clear_pending(env);
    }
    walked
}

/// Whether the framework reports `permission` granted on `context`.
fn permission_granted(
    env: &mut Env<'_>,
    context: &JObject<'_>,
    permission: &JObject<'_>,
) -> Result<bool, RecorderError> {
    Ok(env
        .call_method(
            context,
            jni_str!("checkSelfPermission"),
            jni_sig!("(Ljava/lang/String;)I"),
            &[JValue::Object(permission)],
        )?
        .i()?
        == PERMISSION_GRANTED)
}

/// RNAA's vocabulary over the framework's two answers: a shown rationale means denied, and
/// never-asked cannot be told from don't-ask-again, so both are undetermined.
fn permission_now(env: &mut Env<'_>) -> Result<PermissionState, RecorderError> {
    if sdk_int(env)? < SDK_WITH_RUNTIME_PERMISSIONS {
        return Ok(PermissionState::Granted);
    }
    let context = app_context(env)?;
    let permission: JObject<'_> = JString::from_str(env, RECORD_AUDIO)?.into();
    if permission_granted(env, &context, &permission)? {
        return Ok(PermissionState::Granted);
    }
    let state = match current_activity(env) {
        Some(activity) => {
            let rationale = env
                .call_method(
                    &activity,
                    jni_str!("shouldShowRequestPermissionRationale"),
                    jni_sig!("(Ljava/lang/String;)Z"),
                    &[JValue::Object(&permission)],
                )?
                .z()?;
            if rationale {
                PermissionState::Denied
            } else {
                PermissionState::Undetermined
            }
        }
        None => PermissionState::Undetermined,
    };
    Ok(state)
}

/// Shows the system dialog through the resumed activity and watches the permission state for its
/// answer. No observable answer inside the window resolves as cancelled.
fn ask_permission(env: &mut Env<'_>) -> Result<PermissionState, RecorderError> {
    if sdk_int(env)? < SDK_WITH_RUNTIME_PERMISSIONS {
        return Ok(PermissionState::Granted);
    }
    let context = app_context(env)?;
    let permission: JObject<'_> = JString::from_str(env, RECORD_AUDIO)?.into();
    if permission_granted(env, &context, &permission)? {
        return Ok(PermissionState::Granted);
    }
    let Some(activity) = current_activity(env) else {
        // No activity is reachable: the dialog cannot be shown and the refusal stands.
        return Ok(PermissionState::Denied);
    };
    let element = JString::from_str(env, RECORD_AUDIO)?;
    let requested = JObjectArray::<JString>::new(env, 1, &element)?;
    env.call_method(
        &activity,
        jni_str!("requestPermissions"),
        jni_sig!("([Ljava/lang/String;I)V"),
        &[
            JValue::Object(requested.as_ref()),
            JValue::Int(REQUEST_CODE),
        ],
    )?;
    let began = Instant::now();
    loop {
        if permission_granted(env, &context, &permission)? {
            return Ok(PermissionState::Granted);
        }
        let rationale = env
            .call_method(
                &activity,
                jni_str!("shouldShowRequestPermissionRationale"),
                jni_sig!("(Ljava/lang/String;)Z"),
                &[JValue::Object(&permission)],
            )?
            .z()?;
        if rationale {
            return Ok(PermissionState::Denied);
        }
        if began.elapsed() >= REQUEST_WINDOW {
            // Dismissed or don't-ask-again: the framework answers those only to the activity.
            return Err(RecorderError::new(ErrorKind::Cancelled));
        }
        thread::sleep(REQUEST_POLL);
    }
}

/// The label of an input device, as record names one: the product name, when there is one.
fn device_label(env: &mut Env<'_>, device: &JObject<'_>) -> Result<Option<String>, RecorderError> {
    let name = env
        .call_method(
            device,
            jni_str!("getProductName"),
            jni_sig!("()Ljava/lang/CharSequence;"),
            &[],
        )?
        .l()?;
    if name.is_null() {
        return Ok(None);
    }
    let text = env
        .call_method(
            &name,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    let text: JString = env.cast_local(text)?;
    Ok(Some(text.try_to_string(env)?))
}

/// The microphones `AudioManager` reports, record's own listing route.
fn input_devices(env: &mut Env<'_>) -> Result<Vec<InputDevice>, RecorderError> {
    let context = app_context(env)?;
    let manager = system_service(env, &context, "audio")?;
    let devices = env
        .call_method(
            &manager,
            jni_str!("getDevices"),
            jni_sig!("(I)[Landroid/media/AudioDeviceInfo;"),
            &[JValue::Int(GET_DEVICES_INPUTS)],
        )?
        .l()?;
    let list = env
        .call_static_method(
            jni_str!("java/util/Arrays"),
            jni_str!("asList"),
            jni_sig!("([Ljava/lang/Object;)Ljava/util/List;"),
            &[JValue::Object(&devices)],
        )?
        .l()?;
    let iterator = env
        .call_method(
            &list,
            jni_str!("iterator"),
            jni_sig!("()Ljava/util/Iterator;"),
            &[],
        )?
        .l()?;
    let mut found = Vec::new();
    while env
        .call_method(&iterator, jni_str!("hasNext"), jni_sig!("()Z"), &[])?
        .z()?
    {
        let device = env
            .call_method(
                &iterator,
                jni_str!("next"),
                jni_sig!("()Ljava/lang/Object;"),
                &[],
            )?
            .l()?;
        if device.is_null() {
            continue;
        }
        let id = env
            .call_method(&device, jni_str!("getId"), jni_sig!("()I"), &[])?
            .i()?;
        let label = device_label(env, &device)?;
        found.push(InputDevice {
            id: id.to_string(),
            label,
        });
    }
    Ok(found)
}

/// What the reader thread and the control calls share while one recording runs.
struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    done: AtomicBool,
    /// Frames delivered at the configured rate; the stream output's duration.
    frames: AtomicU64,
    level: Mutex<Amplitude>,
    samples: Mutex<Vec<i16>>,
    failure: Mutex<Option<RecorderError>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            done: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            level: Mutex::new(Amplitude::default()),
            samples: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
        }
    }
}

/// One running recording: the config it runs with and its reader thread.
struct Recording {
    config: RecordConfig,
    shared: Arc<Shared>,
    handle: JoinHandle<()>,
}

/// Keeps the reader's first coded failure; it is what `stop` reports.
fn fail(shared: &Shared, failure: RecorderError) {
    let mut slot = lock(&shared.failure);
    if slot.is_none() {
        *slot = Some(failure);
    }
}

/// A carried linear resampler over interleaved frames; the interpolation phase survives reads so
/// chunk boundaries do not click.
struct Resampler {
    channels: usize,
    /// Input frames consumed per output frame.
    step: f64,
    position: f64,
    tail: Vec<i16>,
}

impl Resampler {
    fn new(source: u32, target: u32, channels: usize) -> Self {
        Self {
            channels,
            // A zero target cannot come through Recorder; the guard keeps the loop total.
            step: if target == 0 {
                1.0
            } else {
                f64::from(source) / f64::from(target)
            },
            position: 0.0,
            tail: Vec::new(),
        }
    }

    /// Appends the frames of `input`, resampled to the target rate, onto `output`.
    fn run(&mut self, input: &[i16], output: &mut Vec<i16>) {
        let channels = self.channels;
        let frames = input.len() / channels;
        while self.position < frames as f64 {
            let whole = self.position.floor() as usize;
            let part = self.position - whole as f64;
            let after = &input[whole * channels..(whole + 1) * channels];
            let before = if whole > 0 {
                &input[(whole - 1) * channels..whole * channels]
            } else if self.tail.len() == channels {
                &self.tail
            } else {
                after
            };
            for channel in 0..channels {
                let start = f64::from(before[channel]);
                let end = f64::from(after[channel]);
                output.push((start + (end - start) * part).round() as i16);
            }
            self.position += self.step;
        }
        self.position -= frames as f64;
        if frames > 0 {
            self.tail = input[input.len() - channels..].to_vec();
        }
    }
}

/// Peak and a smoothed average of one read, normalized like record's `Amplitude`.
fn note_level(shared: &Shared, samples: &[i16]) {
    let mut peak = 0.0f32;
    let mut sum = 0.0f32;
    for sample in samples {
        let level = (f32::from(*sample) / 32767.0).abs();
        peak = peak.max(level);
        sum += level;
    }
    if samples.is_empty() {
        return;
    }
    let average = (sum / samples.len() as f32).clamp(0.0, 1.0);
    let mut level = lock(&shared.level);
    level.average += (average - level.average) * 0.25;
    level.peak = peak.min(1.0);
}

/// A failed call may leave a Java exception pending, and JNI is undefined while one is; drop it.
fn clear_pending(env: &Env<'_>) {
    if env.exception_check() {
        env.exception_clear();
    }
}

/// `AudioRecord`'s teardown, record's rule: stop only while RECORDING, then release. Best-effort —
/// the recording's own failure is what `stop` reports, not this.
fn release(env: &mut Env<'_>, recorder: &JObject<'_>) {
    clear_pending(env);
    let state = env
        .call_method(
            recorder,
            jni_str!("getRecordingState"),
            jni_sig!("()I"),
            &[],
        )
        .and_then(|value| value.i())
        .unwrap_or(0);
    // A throw from one teardown call must not ride into the next; each step starts clear.
    clear_pending(env);
    if state == RECORDSTATE_RECORDING {
        let _ = env.call_method(recorder, jni_str!("stop"), jni_sig!("()V"), &[]);
    }
    clear_pending(env);
    let _ = env.call_method(recorder, jni_str!("release"), jni_sig!("()V"), &[]);
}

/// Builds a started `AudioRecord` with record's defaults: MIC source, PCM16, double the minimum
/// buffer. Every failure releases the half-built recorder before it returns.
fn open_recorder<'local>(
    env: &mut Env<'local>,
    config: &RecordConfig,
) -> Result<(JObject<'local>, u32, JShortArray<'local>, usize), RecorderError> {
    let channel_config = if config.channels == 2 {
        CHANNEL_IN_STEREO
    } else {
        CHANNEL_IN_MONO
    };
    let min = env
        .call_static_method(
            jni_str!("android/media/AudioRecord"),
            jni_str!("getMinBufferSize"),
            jni_sig!("(III)I"),
            &[
                JValue::Int(config.sample_rate as jint),
                JValue::Int(channel_config),
                JValue::Int(ENCODING_PCM_16BIT),
            ],
        )?
        .i()?;
    if min < 0 {
        // ERROR_BAD_VALUE is a bad argument; the other negatives refuse the configuration.
        return Err(if min == ERROR_BAD_VALUE {
            RecorderError::new(ErrorKind::InvalidArgument)
        } else {
            RecorderError::new(ErrorKind::FormatNotSupported)
        }
        .with_status(i64::from(min)));
    }
    // record doubles the minimum for a safety margin; the reads run one minimum at a time.
    let bytes = min.saturating_mul(2);
    let buffer_shorts = min as usize;
    let recorder = env.new_object(
        jni_str!("android/media/AudioRecord"),
        jni_sig!("(IIIII)V"),
        &[
            JValue::Int(AUDIO_SOURCE_MIC),
            JValue::Int(config.sample_rate as jint),
            JValue::Int(channel_config),
            JValue::Int(ENCODING_PCM_16BIT),
            JValue::Int(bytes),
        ],
    )?;
    let state = env
        .call_method(&recorder, jni_str!("getState"), jni_sig!("()I"), &[])
        .and_then(|value| value.i());
    let state = match state {
        Ok(state) => state,
        Err(error) => {
            release(env, &recorder);
            return Err(RecorderError::from(error));
        }
    };
    if state != STATE_INITIALIZED {
        release(env, &recorder);
        return Err(RecorderError::new(ErrorKind::NotAvailable));
    }
    if let Err(error) = env.call_method(&recorder, jni_str!("startRecording"), jni_sig!("()V"), &[])
    {
        release(env, &recorder);
        return Err(RecorderError::from(error));
    }
    let actual = env
        .call_method(&recorder, jni_str!("getSampleRate"), jni_sig!("()I"), &[])
        .and_then(|value| value.i());
    let actual = match actual {
        Ok(actual) => actual,
        Err(error) => {
            release(env, &recorder);
            return Err(RecorderError::from(error));
        }
    };
    let buffer = match JShortArray::new(env, buffer_shorts) {
        Ok(buffer) => buffer,
        Err(error) => {
            release(env, &recorder);
            return Err(RecorderError::from(error));
        }
    };
    Ok((recorder, actual as u32, buffer, buffer_shorts))
}

/// Streams the configured recording: reads the `AudioRecord` on this thread, resamples to the
/// configured rate, feeds the level meter and the sink, and releases on the way out. `answered`
/// carries the construction result to `start` before the loop begins.
fn record(
    env: &mut Env<'_>,
    config: &RecordConfig,
    sink: &ChunkSink,
    shared: &Shared,
    answered: &mpsc::Sender<Result<(), RecorderError>>,
) -> Result<(), RecorderError> {
    let (recorder, actual_rate, buffer, buffer_shorts) = match open_recorder(env, config) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = answered.send(Err(error.clone()));
            return Err(error);
        }
    };
    let _ = answered.send(Ok(()));
    let channels = usize::from(config.channels.max(1));
    let mut samples: Vec<jshort> = vec![0; buffer_shorts];
    let mut resampler = Resampler::new(actual_rate, config.sample_rate, channels);
    let file_output = matches!(config.output, Output::File { .. });
    let mut outcome = Ok(());
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        while shared.paused.load(Ordering::Relaxed) && !shared.stop.load(Ordering::Relaxed) {
            thread::sleep(POLL);
        }
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let read = env
            .call_method(
                &recorder,
                jni_str!("read"),
                jni_sig!("([SII)I"),
                &[
                    JValue::Object(buffer.as_ref()),
                    JValue::Int(0),
                    JValue::Int(buffer_shorts as jint),
                ],
            )
            .and_then(|value| value.i());
        let read = match read {
            Ok(read) => read,
            Err(error) => {
                outcome = Err(RecorderError::from(error));
                break;
            }
        };
        if read < 0 {
            outcome = Err(RecorderError::new(ErrorKind::Backend).with_status(i64::from(read)));
            break;
        }
        if read == 0 {
            thread::sleep(POLL);
            continue;
        }
        let count = read as usize;
        if let Err(error) = buffer.get_region(env, 0, &mut samples[..count]) {
            outcome = Err(RecorderError::from(error));
            break;
        }
        let block = &samples[..count];
        let mut delivered = Vec::new();
        resampler.run(block, &mut delivered);
        note_level(shared, block);
        let frames = (delivered.len() / channels) as u64;
        shared.frames.fetch_add(frames, Ordering::Relaxed);
        if file_output {
            lock(&shared.samples).extend_from_slice(&delivered);
        }
        sink(AudioChunk {
            data: delivered,
            sample_rate: config.sample_rate,
            channels: config.channels,
        });
    }
    release(env, &recorder);
    outcome
}

/// The reader thread: one attached run that builds the recorder, streams it, and releases it.
fn reader_thread(
    config: RecordConfig,
    sink: ChunkSink,
    shared: Arc<Shared>,
    answered: mpsc::Sender<Result<(), RecorderError>>,
) {
    let answer = attached(move |env| record(env, &config, &sink, &shared, &answered));
    if let Err(failure) = answer {
        fail(&shared, failure);
    }
    shared.done.store(true, Ordering::Relaxed);
}

/// Signals the reader, waits bounded for it, and joins it; `Err` means it never finished and stays
/// detached to release the `AudioRecord` whenever it does.
fn end(handle: &JoinHandle<()>, shared: &Shared) -> Result<(), RecorderError> {
    shared.stop.store(true, Ordering::Relaxed);
    let began = Instant::now();
    while !shared.done.load(Ordering::Relaxed) {
        let waited = began.elapsed();
        if waited >= JOIN_WINDOW {
            // The status carries how long the reader was given before it was left detached.
            return Err(
                RecorderError::new(ErrorKind::Backend).with_status(waited.as_millis() as i64)
            );
        }
        thread::sleep(POLL);
    }
    let _ = handle.join();
    Ok(())
}

/// Frames to milliseconds, rounded. A zero rate cannot come through `Recorder`; the guard keeps
/// the helper total for a direct backend caller.
fn duration_ms(frames: u64, rate: u32) -> u64 {
    if rate == 0 {
        0
    } else {
        (frames * 1000 + u64::from(rate) / 2) / u64::from(rate)
    }
}

/// The reader's first coded failure, when it had one.
fn failure_of(shared: &Shared) -> Option<RecorderError> {
    lock(&shared.failure).clone()
}

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        dispatched(permission_now)
    }

    fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>> {
        dispatched(ask_permission)
    }

    fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>> {
        dispatched(input_devices)
    }

    fn is_encoder_supported(
        &self,
        encoder: Encoder,
    ) -> BackendFuture<'_, Result<bool, RecorderError>> {
        Box::pin(async move {
            // Android v1 records PCM16 and this layer writes the WAV container itself.
            Ok(matches!(encoder, Encoder::Wav | Encoder::Pcm16bits))
        })
    }

    fn start(&self, config: &RecordConfig, sink: ChunkSink) -> Result<(), RecorderError> {
        let mut slot = lock(&RECORDING);
        if slot.is_some() {
            return Err(RecorderError::new(ErrorKind::InProgress));
        }
        let shared = Arc::new(Shared::new());
        let (answered, listening) = mpsc::channel();
        let reader = Arc::clone(&shared);
        let thread_config = config.clone();
        let handle = thread::Builder::new()
            .name("nsh-microphone-android".to_owned())
            .spawn(move || reader_thread(thread_config, sink, reader, answered))
            .map_err(|_| RecorderError::new(ErrorKind::Backend))?;
        match listening.recv_timeout(START_WINDOW) {
            Ok(Ok(())) => {
                *slot = Some(Recording {
                    config: config.clone(),
                    shared,
                    handle,
                });
                Ok(())
            }
            Ok(Err(error)) => {
                let _ = end(&handle, &shared);
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Construction never answered; the flag retires the thread whenever it does.
                shared.stop.store(true, Ordering::Relaxed);
                let _ = end(&handle, &shared);
                Err(RecorderError::new(ErrorKind::Backend))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The reader died before answering; only a panic in the JNI layer lands here.
                let _ = handle.join();
                Err(RecorderError::new(ErrorKind::Backend))
            }
        }
    }

    fn pause(&self) -> Result<(), RecorderError> {
        let slot = lock(&RECORDING);
        let Some(recording) = slot.as_ref() else {
            return Err(RecorderError::new(ErrorKind::InvalidState));
        };
        if let Some(failure) = failure_of(&recording.shared) {
            return Err(failure);
        }
        recording.shared.paused.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn resume(&self) -> Result<(), RecorderError> {
        let slot = lock(&RECORDING);
        let Some(recording) = slot.as_ref() else {
            return Err(RecorderError::new(ErrorKind::InvalidState));
        };
        if let Some(failure) = failure_of(&recording.shared) {
            return Err(failure);
        }
        recording.shared.paused.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>> {
        Box::pin(async move {
            let slot = lock(&RECORDING);
            let Some(recording) = slot.as_ref() else {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            };
            if let Some(failure) = failure_of(&recording.shared) {
                return Err(failure);
            }
            Ok(*lock(&recording.shared.level))
        })
    }

    fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>> {
        dispatched(|_env| {
            let recording = lock(&RECORDING).take();
            let Some(recording) = recording else {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            };
            end(&recording.handle, &recording.shared)?;
            if let Some(failure) = failure_of(&recording.shared) {
                return Err(failure);
            }
            match recording.config.output {
                Output::File { path } => {
                    let samples = std::mem::take(&mut *lock(&recording.shared.samples));
                    write_wav(
                        &path,
                        recording.config.sample_rate,
                        recording.config.channels,
                        &samples,
                    )
                }
                Output::Stream => {
                    let frames = recording.shared.frames.load(Ordering::Relaxed);
                    Ok(StoppedRecording {
                        path: None,
                        duration_ms: Some(duration_ms(frames, recording.config.sample_rate)),
                    })
                }
            }
        })
    }

    fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        dispatched(|_env| {
            let recording = lock(&RECORDING).take();
            let Some(recording) = recording else {
                return Err(RecorderError::new(ErrorKind::InvalidState));
            };
            end(&recording.handle, &recording.shared)?;
            // The output is discarded, so a reader failure ends it and still resolves clean.
            Ok(())
        })
    }

    fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>> {
        dispatched(|_env| {
            if let Some(recording) = lock(&RECORDING).take() {
                end(&recording.handle, &recording.shared)?;
            }
            Ok(())
        })
    }
}
