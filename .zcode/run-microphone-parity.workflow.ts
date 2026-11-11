// Dynamic workflow for /Users/hmziq/fo/nsh-microphone.
//
// Brief: build BOTH a Rust crate and a TypeScript package laid out like SuperNSH/biometrics
// (.refs/biometrics), same API names and behavior, every behavior tested in both. GLM-5.3 Max
// (the main-tier model) pinned on EVERY agent; audit up to 3 rounds per area, loop until zero
// issues or the cap, report honestly when an area never converges. No commits, no pushes, no
// other repos. STEP 0 (clone the five references into .refs/, read them) was done by the
// launcher pre-launch; this script only verifies it before any area starts.
//
// Gates reconciliation (recorded in docs/decisions.md by area ci-meta): AGENTS.md forbids
// building or testing on a developer machine ("GitHub Actions compiles and tests everything"),
// so the only local deterministic gates are pure-formatting and parse-only checks
// (`rustfmt --check`, `cargo fmt --all -- --check`, a YAML load of ci.yml). cargo clippy/test and
// the NSH TS gates (`nsh check`/`nsh build`/`nsh test`) are AUTHORED into
// .github/workflows/ci.yml for CI to run. The user forbade commits/pushes, so no CI run happens
// in this task: every status claim stays "Authored (awaiting first CI run)" and the final report
// says the gates were authored, not executed.

const MODELS = {
  // The user asked for GLM-5.3 Max on every area and every audit round. No vision work exists in
  // this run, so there is no flash pin anywhere.
  pro: model("account:zai-individual-coding-plan/GLM-5.3$high"),
};

const KIT = "/Users/hmziq/ZCodeProject/ai/harness/zcode/agents";
const PRO_FALLBACK =
  "You are a careful, judgment-heavy implementer and reviewer for one area of a parallel workflow. " +
  "Read before you write, match the surrounding code's style, and report honestly.";

const FENCE = "```";

const RESULT_FOOTER = `

When done, end your reply with a fenced json block exactly like this:
${FENCE}json
{"done": true, "files": ["every path you wrote or changed"], "summary": "one paragraph: what you implemented and any deviation from the frozen contract", "contractLine": "one sentence copied VERBATIM from your specialist contract (your system prompt) that most shaped the work", "blockers": []}
${FENCE}
The contractLine sentence must be an exact substring of your system prompt: the run verifies it.`;

const AUDIT_FOOTER = `

End your reply with a fenced json block exactly like this:
${FENCE}json
{"findings": [{"file": "path", "issue": "what is wrong, quoting the frozen contract or the reference file", "fix": "the exact change to make"}], "contractLine": "one sentence copied VERBATIM from your specialist contract (your system prompt)"}
${FENCE}
An empty findings array means clean. Return ONLY actionable findings: no PASS notes, no praise, no prose summary outside the block.`;

// ---------------------------------------------------------------------------
// Frozen contract. Every area implements exactly this slice; no decisions in flight.
// ---------------------------------------------------------------------------

const CONTRACT = `
## Repo conventions (AGENTS.md, hard rules)
- Comments are 1-2 lines and state a non-obvious constraint. Docs read like a competent human: direct, concrete, no marketing filler.
- Dead code is deleted. No speculative abstractions.
- Every behaviour lands in BOTH the Rust crate and the TypeScript package, with a test in both.
- Every call resolves exactly once: success, a coded failure, or a cancel. Nothing hangs. No panics: no unwrap/expect on operation paths; lock with .unwrap_or_else(PoisonError::into_inner) like the reference does.
- Work only inside this repo. Never touch .refs/ contents (read-only references). NO git commands that change state: no commits, no pushes, no branches, no worktrees, no stashes. Do not run cargo build/check/clippy/test, nsh check/build/test, npm/yarn/pnpm install, or any emulator: AGENTS.md says GitHub Actions compiles and tests everything, and the CI workflow file is the authored gate. Allowed locally: rustfmt (check or format), reading files, \`git -C .refs/nsh show v0.4.0:<path>\` (read-only), \`cargo add --dry-run\` in a scratch dir under /tmp (metadata only), WebFetch of docs.rs pages.
- The deliverable of this task is the changed files. CI will compile and test them after a human commits and pushes.

## Layout (mirrors .refs/biometrics exactly)
- Root Cargo.toml becomes a workspace: members = ["crates/nsh-microphone"], resolver = "3", [workspace.package] version = "0.1.0", edition = "2024", license = "MIT", repository = "https://github.com/SuperNSH/microphone", authors = ["hmziqrs"].
- crates/nsh-microphone/: the Rust crate (lib name nsh_microphone).
- packages/nsh-microphone/: the TypeScript package.
- The old root src/lib.rs stub is deleted by area core (plain rm; the workspace replaces it).
- examples/demo and a package App.tsx demo are deliberately omitted: this repo's extra is the site/ status page. Recorded in docs/decisions.md.

## crates/nsh-microphone/Cargo.toml (exact dependency set; versions verified against crates.io on 2026-11-09)
[package] name = "nsh-microphone", description = "Recording to file or stream for Native Shell apps: voice notes and level meters. Not speech-to-text.", all fields inherited from workspace, [lib] name = "nsh_microphone", path = "src/lib.rs".
[dependencies]
  nsh-core = "0.4.0"
  thiserror = "2"
  tokio = { version = "1.54.1", default-features = false, features = ["sync"] }
  # cpal is only compiled where a desktop host uses it; macOS uses the Apple native layer.
[target.'cfg(any(target_os = "windows", target_os = "linux"))'.dependencies]
  cpal = "0.18.2"
[target.'cfg(target_vendor = "apple")'.dependencies]
  objc2 = { version = "0.6", features = ["std"] }
  block2 = { version = "0.6", features = ["std"] }
  objc2-avf-audio = { version = "0.3.2", default-features = false, features = ["std", "alloc", "AVAudioApplication", "AVAudioBuffer", "AVAudioEngine", "AVAudioFile", "AVAudioFormat", "AVAudioIONode", "AVAudioMixing", "AVAudioNode", "AVAudioSession", "AVAudioSessionRoute", "AVAudioSettings", "AVAudioTypes", "AVAudioTime"] }
[target.'cfg(target_os = "android")'.dependencies]
  jni = "0.22.4"
  ndk-context = "0.1.1"
[dev-dependencies]
  tokio = { version = "1.54.1", default-features = false, features = ["rt", "macros", "time"] }
  nsh-engine = { version = "0.4.0", default-features = false }
  nsh-testing = { version = "0.4.0", default-features = false }

## src/error.rs — the error model
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum ErrorKind { NotAvailable, PermissionDenied, Cancelled, InProgress, InvalidState, InvalidArgument, FormatNotSupported, Backend }
Stable codes (ErrorKind::code, &'static str):
  NotAvailable        -> "ERR_MICROPHONE_NOT_AVAILABLE"
  PermissionDenied    -> "ERR_MICROPHONE_PERMISSION_DENIED"
  Cancelled           -> "ERR_MICROPHONE_CANCELLED"
  InProgress          -> "ERR_MICROPHONE_IN_PROGRESS"
  InvalidState        -> "ERR_MICROPHONE_INVALID_STATE"
  InvalidArgument     -> "ERR_MICROPHONE_INVALID_ARGUMENT"
  FormatNotSupported  -> "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED"
  Backend             -> "ERR_MICROPHONE_BACKEND"
Messages (ErrorKind::message, one sentence, the SAME wording in the TS MESSAGES map):
  NotAvailable:       "Microphone recording is not available on this device."
  PermissionDenied:   "Recording was refused. Allow microphone access in the system settings."
  Cancelled:          "The operation was cancelled."
  InProgress:         "A recording is already running. Stop or cancel it first."
  InvalidState:       "The recorder is in a state that does not allow this call."
  InvalidArgument:    "An argument is empty or not allowed on this platform."
  FormatNotSupported: "The encoder or format is not supported on this platform."
  Backend:            "The platform's audio service failed."
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub struct RecorderError { kind: ErrorKind, status: Option<i64> } with Display = kind.message(); methods: new(kind), with_status(i64) -> Self, kind(), status() -> Option<i64>, code() -> &'static str.
impl fmt::Display for ErrorKind prints the code.
Reference mapping (documented in docs, mapped in platform layers): cpal BuildStreamError::DeviceNotAvailable -> NotAvailable; cpal StreamError mid-stream -> Backend (with status where the host gives one); cpal FormatNotSupported/StreamConfigInvalid -> FormatNotSupported/InvalidArgument; Flutter record's denied permission -> PermissionDenied; record's "already recording" -> InProgress; RNAA permission "denied"/"undetermined" -> PermissionDenied/Undetermined state; unknown platform errors -> Backend.

## src/options.rs — inputs and outputs
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum Encoder { AacLc, AacEld, AacHe, AmrNb, AmrWb, Flac, Opus, Pcm16bits, Vorbis, Wav }
  Encoder::name() -> &'static str returns the exact Flutter-record enum names: "aacLc", "aacEld", "aacHe", "amrNb", "amrWb", "flac", "opus", "pcm16bits", "vorbis", "wav". The TS union uses the same strings.
#[derive(Debug, Clone, PartialEq, Eq)] pub enum Output { File { path: String }, Stream } with impl Default = Output::Stream.
#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct RecordConfig { sample_rate: u32 (default 44_100), channels: u16 (default 1), bit_rate: u32 (default 128_000), encoder: Encoder (default AacLc), output: Output }
  Public fields. Options a platform cannot apply are accepted and documented in docs/parity.md, like biometrics does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum PermissionState { Granted, Denied, Undetermined } with PermissionState::name() -> &'static str: "granted", "denied", "undetermined" (the RNAA vocabulary).
#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct InputDevice { pub id: String, pub label: Option<String> }
#[derive(Debug, Clone, Copy, PartialEq, Default)] pub struct Amplitude { pub average: f32, pub peak: f32 } — normalized 0.0..=1.0, like record's Amplitude.
#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct StoppedRecording { pub path: Option<String>, pub duration_ms: Option<u64> } — path is None for a stream output (record's stop returns null there).
#[derive(Debug, Clone, PartialEq, Eq)] pub struct AudioChunk { pub data: Vec<i16>, pub sample_rate: u32, pub channels: u16 }

## src/backend.rs — the seam (mirror of biometrics' backend.rs)
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type ChunkSink = std::sync::Arc<dyn Fn(AudioChunk) + Send + Sync>;
pub trait Backend: Send + Sync {
  fn check_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>>;
  fn request_permissions(&self) -> BackendFuture<'_, Result<PermissionState, RecorderError>>;
  fn list_input_devices(&self) -> BackendFuture<'_, Result<Vec<InputDevice>, RecorderError>>;
  fn is_encoder_supported(&self, encoder: Encoder) -> BackendFuture<'_, Result<bool, RecorderError>>;
  fn start(&self, config: &RecordConfig, sink: ChunkSink) -> Result<(), RecorderError>;
  fn pause(&self) -> Result<(), RecorderError>;
  fn resume(&self) -> Result<(), RecorderError>;
  fn amplitude(&self) -> BackendFuture<'_, Result<Amplitude, RecorderError>>;
  fn stop(&self) -> BackendFuture<'_, Result<StoppedRecording, RecorderError>>;
  fn cancel(&self) -> BackendFuture<'_, Result<(), RecorderError>>;
  fn dispose(&self) -> BackendFuture<'_, Result<(), RecorderError>>;
}
Sync where the platform answers at once (start, pause, resume) like biometrics' start_prompt; async where the OS may take time (permissions, devices, amplitude, stop, cancel, dispose).

## src/recorder.rs — the front type (mirror of biometrics' auth.rs rules)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum RecordingState { Idle, Recording, Paused }
#[derive(Clone)] pub struct Recorder — Arc<Shared>; clones share one gate, like LocalAuth.
  Recorder::new() -> Self: the platform's shared handle via platform::default_backend() behind a OnceLock, exactly like biometrics' LocalAuth::new().
  Recorder::with_backend(backend: impl Backend + 'static) -> Self: its own gate. Tests and fakes use this.
State machine and rules (the gate is a std Mutex like biometrics' Gate; a generation counter drops late answers):
  state(&self) -> RecordingState; is_recording(), is_paused() -> bool.
  check_permissions / request_permissions -> BackendFuture<Result<PermissionState, RecorderError>>: pass-through.
  list_input_devices, is_encoder_supported(encoder): pass-through.
  start(&self, config: &RecordConfig, sink: ChunkSink) -> BackendFuture<Result<(), RecorderError>>:
    1. Refuse bad configs BEFORE claiming the gate, with InvalidArgument: sample_rate == 0; channels == 0 or > 2; bit_rate == 0 unless encoder is Pcm16bits or Wav; output is File with a blank path.
    2. check_permissions: Undetermined -> request_permissions once; still not Granted -> PermissionDenied; the backend answering Err(Cancelled) surfaces as Cancelled.
    3. Gate busy (a recording runs) -> InProgress.
    4. is_encoder_supported false -> FormatNotSupported.
    5. backend.start; on success the state is Recording and the gate holds until stop/cancel/dispose.
  pause(): only while Recording, else InvalidState. resume(): only while Paused, else InvalidState.
  amplitude(): only while Recording or Paused, else InvalidState.
  stop(): only while Recording or Paused, else InvalidState; resolves once with the backend's StoppedRecording, frees the gate, state Idle; a late backend answer after a cancel or a second stop is dropped (generation guard).
  cancel(): only while Recording or Paused, else InvalidState; discards the output, frees the gate, state Idle; resolves once with Ok(()).
  dispose(): if recording, cancel first; the backend's dispose runs once; afterwards every call on the handle answers NotAvailable.

## src/fake.rs — FakeBackend (mirror of biometrics' fake.rs, Clone over Arc<Mutex<FakeState>>)
Default: permissions Granted, devices one InputDevice { id: "built-in-mic", label: Some("Built-in microphone") }, encoders supported, every operation succeeds; stop answers StoppedRecording { path: Some("recording.m4a"), duration_ms: Some(0) }.
Setters (each takes the scripted answer): set_check_permissions(Result<PermissionState, RecorderError>), set_request_permissions(same), set_devices(Result<Vec<InputDevice>, RecorderError>), set_encoder_supported(Result<bool, RecorderError>), fail_next_start/pause/resume/amplitude/stop/cancel/dispose(RecorderError).
Drive helpers: hold_stop() makes the next stop future pend until finish_stop(Result<StoppedRecording, RecorderError>) -> bool (false when nothing held); push_chunk(AudioChunk) -> bool delivers to the sink only while Recording (true), dropped while Paused (false); recording() -> bool; last_config() -> Option<RecordConfig>.
Counters: starts(), pauses(), resumes(), stops(), cancels(), disposals(), chunks().

## src/platform/mod.rs and src/platform/unsupported.rs (area core)
mod.rs: cfg-selected platform modules exactly like biometrics' platform/mod.rs:
  apple on cfg(target_vendor = "apple"); android on cfg(target_os = "android"); windows on cfg(target_os = "windows"); linux on cfg(target_os = "linux"); web on cfg(target_arch = "wasm32"); unsupported on cfg(not(any of the above)).
  Each selected module exports a NativeBackend: crate::Backend. default_backend() -> Arc<dyn Backend> returns Arc::new(NativeBackend).
  mod.rs also carries pub(crate) fn write_wav(path: &str, sample_rate: u32, channels: u16, samples: &[i16]) -> Result<StoppedRecording, RecorderError>: a small WAV writer (44-byte canonical header) the platform layers use for Wav/Pcm16bits file output; it maps io::Error to Backend.
unsupported.rs and web.rs: the same shape — every Backend method answers Err(NotAvailable); web.rs's doc comment says "the web is an unsupported target" (the biometrics precedent).

## src/native.rs and src/availability.rs (area nsh-surface) — the NSH TSX layer
Verify every macro, type and method against the PUBLISHED 0.4.0 sources before using it:
  git -C .refs/nsh show v0.4.0:crates/nsh-core/src/prelude.rs  (and js/, platform.rs, reactive/)
The pattern files in .refs/biometrics compile against nsh-core 0.4.0, so their usage is the baseline; where the v0.4.0 source differs from the biometrics HEAD file, v0.4.0 wins.
native.rs: #[class] pub struct NativeRecorder { recorder: Recorder } with #[class] impl methods (NSH exposes snake_case members to TSX in camelCase, as biometrics' rust.ts proves):
  new() -> Self; check_permissions() -> Result<String, JsError> (PermissionState::name); request_permissions(); list_input_devices() -> Result<Vec<NativeInputDevice>, JsError>; is_encoder_supported(encoder: String) -> Result<bool, JsError> (unknown name -> InvalidArgument); start(config: Option<NativeRecordConfig>) -> Result<(), JsError>; pause(); resume(); amplitude() -> Result<NativeAmplitude, JsError>; stop() -> Result<NativeStoppedRecording, JsError>; cancel() -> Result<(), JsError>; dispose() -> Result<(), JsError>.
  #[derive(JsObject, Clone, Debug, Default)] NativeRecordConfig { sample_rate: Option<u32>, channels: Option<u16>, bit_rate: Option<u32>, encoder: Option<String>, path: Option<String> } — path absent means Output::Stream, which the native surface refuses with InvalidArgument (stream output over the Rust backend is not wired in NSH 0.4.0; parity documents it).
  #[derive(JsObject, Clone, Debug)] NativeInputDevice { id: String, label: Option<String> }; NativeAmplitude { average: f32, peak: f32 }; NativeStoppedRecording { path: Option<String>, duration_ms: Option<u64> }.
  Errors: into_js(error) = JsError::Error(error.kind().message().to_owned()).with_code(error.code()) — the exact biometrics convention (the TS asRecorderError parses "CODE: message" out of a host ERR_NSH_HOST_ERROR).
  #[cfg(test)] mod tests at the bottom, mirroring biometrics' native.rs tests: each encoder has the TSX name; a start with a stream output is refused; defaults keep RecordConfig::default().
availability.rs: mirror of biometrics' availability.rs with our types:
  #[non_exhaustive] #[derive(Debug, Clone, Default, PartialEq)] pub struct Availability { pub permission: PermissionState, pub has_input_device: bool }
  pub enum AvailabilityState { Checking, Ready(Availability), Failed(RecorderError) }
  pub struct MicrophoneAvailability { state: Signal<AvailabilityState>, refresh: Stored<Rc<dyn Fn()>> } with state() and refresh().
  pub fn use_microphone_availability() -> MicrophoneAvailability (Recorder::new) and use_microphone_availability_for(recorder: Recorder) — the re-check on Lifecycle::Active, the inflight task drop, the gone flag, cleanup: copy biometrics' structure exactly with nsh_core::platform::Lifecycle.

## src/lib.rs skeleton (area core writes it complete, so every other area's names resolve)
//! Crate doc: Recording to file or stream for Native Shell apps; each call resolves exactly once.
mod availability; mod backend; mod error; mod fake; mod native; mod options; mod platform; mod recorder;
pub use availability::{Availability, AvailabilityState, MicrophoneAvailability, use_microphone_availability, use_microphone_availability_for};
pub use backend::{Backend, BackendFuture, ChunkSink};
pub use error::{ErrorKind, RecorderError};
pub use fake::FakeBackend;
pub use native::NativeRecorder;
pub use options::{Amplitude, AudioChunk, Encoder, InputDevice, Output, PermissionState, RecordConfig, StoppedRecording};
pub use recorder::{Recorder, RecordingState};
#[native_module("nsh-microphone/native")] fn native(registry: &mut Registry) { registry.class::<NativeRecorder>(); }

## packages/nsh-microphone — the TypeScript package (area ts-package)
package.json: name "nsh-microphone", version "0.1.0", description "Recording to file or stream for Native Shell TSX apps, with record's names", license "MIT", "type": "module", "main": "./src/index.ts", "types": "./src/index.ts", exports { ".": "./src/index.ts", "./rust": "./src/rust.ts", "./memory": "./src/memory.ts" }, files ["src"], repository "https://github.com/SuperNSH/microphone". (Same shape as biometrics'; the NSH host provides "nsh" and "nsh/test" — no node_modules, no lockfile.)
tsconfig.json: { "extends": "./.nsh/tsconfig.json", "include": ["src/**/*.ts", "tests/**/*.ts", "tests/**/*.tsx"] } (.nsh/ is generated by the NSH toolchain in CI).
nsh.json: { "name": "nsh-microphone tests", "id": "dev.supernsh.microphone.tests", "version": "0.1.0", "build": 1, "rust": "../../crates/nsh-microphone", "capabilities": {} } (mirror biometrics').
src/errors.ts: type RecorderErrorCode = the 8 ERR_MICROPHONE_* strings; const MESSAGES: Record<RecorderErrorCode, string> with the same wording as the Rust messages; class RecorderError extends Error { readonly code } (name = "RecorderError"); recorderError(code); asRecorderError(error: unknown): RecorderError — parses a host rejection: error.code === "ERR_NSH_HOST_ERROR" with a message starting "ERR_MICROPHONE_<...>: " maps to the named code when known, else Backend; a direct known code passes through; anything else -> Backend. Copy biometrics' errors.ts structure.
src/options.ts: type Encoder = "aacLc" | "aacEld" | "aacHe" | "amrNb" | "amrWb" | "flac" | "opus" | "pcm16bits" | "vorbis" | "wav"; type PermissionState = "granted" | "denied" | "undetermined"; type Output = { kind: "file"; path: string } | { kind: "stream" }; interface RecordConfig { sampleRate: number (default 44100); channels: number (default 1); bitRate: number (default 128000); encoder: Encoder (default "aacLc"); output: Output (default { kind: "stream" }) }; interface InputDevice { id: string; label?: string }; interface Amplitude { average: number; peak: number }; interface StoppedRecording { path: string | null; durationMs: number | null }; interface AudioChunk { data: Int16Array; sampleRate: number; channels: number }.
src/backend.ts: interface ChunkSink { (chunk: AudioChunk): void }; interface RecorderBackend { checkPermissions(): Promise<PermissionState>; requestPermissions(): Promise<PermissionState>; listInputDevices(): Promise<InputDevice[]>; isEncoderSupported(encoder: Encoder): Promise<boolean>; start(config: RecordConfig, sink: ChunkSink): Promise<void>; pause(): Promise<void>; resume(): Promise<void>; amplitude(): Promise<Amplitude>; stop(): Promise<StoppedRecording>; cancel(): Promise<void>; dispose(): Promise<void>; }
src/memory.ts: memoryBackend(config: MemoryRecorderConfig = {}): MemoryRecorder — the TS twin of FakeBackend: MemoryRecorderConfig { permission?: PermissionState; devices?: InputDevice[]; encoderSupported?: boolean } plus MemoryRecorder extends RecorderBackend with the drive helpers: setPermission(state), failNextStart(code), holdStop(), finishStop(result: StoppedRecording | RecorderErrorCode): boolean, pushChunk(chunk: AudioChunk): boolean, recording(): boolean, lastConfig(): RecordConfig | undefined, counters starts()/pauses()/resumes()/stops()/cancels()/disposals()/chunks(). Semantics identical to FakeBackend (chunks drop while paused; a late finishStop after cancel is ignored).
src/rust.ts: import { NativeRecorder } from "nsh-microphone/native"; rustBackend(): RecorderBackend wrapping NativeRecorder with a settle() that wraps every rejection in asRecorderError. Config mapping: file output maps to NativeRecordConfig.path; stream output over the Rust backend rejects recorderError("ERR_MICROPHONE_INVALID_ARGUMENT") (documented in parity). Encoder strings pass through (same names both sides).
src/recorder.ts: class Recorder — the TS twin of the Rust Recorder: constructor(options: { backend?: RecorderBackend } = {}) with backend = rustBackend() default; private busy gate + state machine (Idle | Recording | Paused) with a generation counter so a late backend answer never settles a cancelled call; methods checkPermissions, requestPermissions, listInputDevices, isEncoderSupported, start(config: RecordConfig, onChunk?: ChunkSink) -> Promise<void> (same validation order and codes as Rust: InvalidArgument refusals before claiming the gate; Undetermined -> one request; Denied -> PERMISSION_DENIED; busy -> IN_PROGRESS; unsupported encoder -> FORMAT_NOT_SUPPORTED), pause, resume, amplitude, stop, cancel, dispose (after dispose every call rejects NOT_AVAILABLE); isRecording(), isPaused(), state(): RecordingState. Export type RecordingState = "idle" | "recording" | "paused".
src/availability.ts: mirror of biometrics' TS availability.ts: interface Availability { permission: PermissionState; hasInputDevice: boolean }; type AvailabilityState = { status: "checking" } | { status: "ready"; availability: Availability } | { status: "failed"; error: RecorderError }; interface MicrophoneAvailability { state(): AvailabilityState; refresh(): void }; useMicrophoneAvailability(options: UseMicrophoneAvailabilityOptions = {}) with recorder?: Recorder — imports { signal, effect, cleanup, lifecycle } from "nsh", keeps biometrics' NSH 0.4.0 web-runtime lifecycle guard comment and the describeKey dedupe.
src/index.ts: re-export everything above with biometrics' export style.

## Tests (areas rust-tests and ts-tests) — the canonical test list
Rust integration tests in crates/nsh-microphone/tests/{recorder,errors,edge_cases,availability}.rs, all through Recorder::with_backend(FakeBackend...) exactly like biometrics drives FakeBackend. Test function names (frozen; goals.md and parity.md reference them):
  recorder.rs: r_start_while_recording_is_refused_with_in_progress; r_pause_and_resume_track_the_state_machine; r_pause_when_idle_is_invalid_state; r_resume_when_recording_is_invalid_state; r_stop_when_idle_is_invalid_state; r_stop_twice_resolves_once_then_invalid_state; r_cancel_when_idle_is_invalid_state; r_amplitude_when_idle_is_invalid_state; r_clones_share_one_gate; r_dispose_while_recording_stops_and_later_calls_report_not_available
  errors.rs: r_every_kind_reports_its_stable_code; r_a_platform_status_travels_with_its_error
  edge_cases.rs: r_start_with_zero_sample_rate_is_invalid_argument; r_start_with_zero_or_three_channels_is_invalid_argument; r_start_with_an_empty_file_path_is_invalid_argument; r_start_with_zero_bit_rate_on_a_compressed_encoder_is_invalid_argument; r_an_unsupported_encoder_is_format_not_supported; r_undetermined_permission_is_requested_once_then_start_proceeds; r_denied_permission_is_permission_denied; r_a_cancelled_permission_prompt_reports_cancelled; r_the_config_reaches_the_backend; r_chunks_pause_and_resume_with_the_recording; r_a_late_stop_answer_after_cancel_is_ignored; r_a_backend_error_keeps_its_code_and_status; r_each_encoder_has_the_name_tsx_uses
  availability.rs: r_availability_wakes_only_on_change — adapt biometrics' tests/availability.rs (nsh_testing::Headless + a #[component] Probe that reads use_microphone_availability_for(Recorder::with_backend(fake))); verify the harness API names against \`git -C .refs/nsh show v0.4.0:crates/...\` and the biometrics file.
TypeScript tests in packages/nsh-microphone/tests/{recorder,errors,edge_cases,availability}.test.tsx with \`import { expect, test } from "nsh/test"\` and memoryBackend(). Test titles (frozen):
  recorder.test.tsx: "start while recording rejects with ERR_MICROPHONE_IN_PROGRESS"; "pause and resume track the state machine"; "pause when idle rejects ERR_MICROPHONE_INVALID_STATE"; "resume when recording rejects ERR_MICROPHONE_INVALID_STATE"; "stop when idle rejects ERR_MICROPHONE_INVALID_STATE"; "stop resolves once; a second stop rejects ERR_MICROPHONE_INVALID_STATE"; "cancel when idle rejects ERR_MICROPHONE_INVALID_STATE"; "amplitude when idle rejects ERR_MICROPHONE_INVALID_STATE"; "clones share one gate"; "dispose while recording cancels and later calls reject ERR_MICROPHONE_NOT_AVAILABLE"
  errors.test.tsx: "every code carries its message"; "asRecorderError maps a host rejection's code prefix"
  edge_cases.test.tsx: "zero sample rate rejects ERR_MICROPHONE_INVALID_ARGUMENT"; "zero or three channels reject ERR_MICROPHONE_INVALID_ARGUMENT"; "an empty file path rejects ERR_MICROPHONE_INVALID_ARGUMENT"; "zero bit rate on a compressed encoder rejects ERR_MICROPHONE_INVALID_ARGUMENT"; "an unsupported encoder rejects ERR_MICROPHONE_FORMAT_NOT_SUPPORTED"; "undetermined permission is requested once, then start proceeds"; "denied permission rejects ERR_MICROPHONE_PERMISSION_DENIED"; "a cancelled permission prompt rejects ERR_MICROPHONE_CANCELLED"; "the config reaches the backend"; "chunks pause and resume with the recording"; "a late stop answer after cancel is ignored"; "a backend error keeps its code"; "each encoder has the name the Rust crate uses"
  availability.test.tsx: mirror biometrics' tests/availability.test.tsx + tests/fixtures/ pattern (a TSX fixture component driving useMicrophoneAvailability with a memory backend): "availability checks at start and refreshes on demand", "availability reports a failed check with its code".
The biometrics rule holds: a test is only authoritative once CI runs it; until then docs call tests Authored.

## Docs
docs/goals.md (area goals): Goals; Acceptance criteria (each maps to a CI job and a Rust+TS test pair); the error model table (8 kinds x code x message x reference mapping); the edge-case table (every case from docs/brief.md plus the contract list, each mapped to the named Rust and TS tests above and to how record / react-native-audio-api / cpal handle it); Out of scope: speech-to-text, on-device test runs in CI (device tests stay manual for maintainers), demo app.
docs/parity.md (areas parity then parity-rn, chained): replaces the scaffold table. Status vocabulary at the top: Authored (code and a test exist; the repo's CI has not run yet because this task may not commit or push), Partial (works, limit named), Accepted (name kept, platform does nothing), Platform only (device behavior no CI runner shows; the named test covers what the library passes on). Done is reserved for after the first green CI run. Sections: record's methods, RecordConfig fields, AudioEncoder values; react-native-audio-api's AudioRecorder methods, options and events; cpal's surface as used by the desktop layers. Columns: | reference API | Rust | TypeScript | Status | Covered by |. Rows name the canonical tests above.
`;

const REFS_NOTE = `
## References on disk (read them; never modify .refs/)
- Layout and API-pattern template: .refs/biometrics/ (the finished twin of this task; its ci.yml and docs set the style).
- Flutter record plugin: .refs/record/ (record_platform_interface/lib/src/ has the API and types: record_platform_interface.dart, types/record_config.dart, types/audio_encoder.dart, types/amplitude.dart, types/input_device.dart, types/ios_audio_session.dart, types/android_record_config.dart).
- react-native-audio-api: .refs/react-native-audio-api/packages/react-native-audio-api/src/core/AudioRecorder.ts and its types (AudioRecorderOptions, AudioRecorderFileOptions, FileFormat, FileInfo, Result).
- cpal: .refs/cpal/src/{lib,platform,error,stream}.rs (device enumeration, StreamConfig, error kinds).
- nsh at the pinned version: \`git -C .refs/nsh show v0.4.0:crates/nsh-core/src/<path>\` (HEAD is NOT 0.4.0; always read v0.4.0).
- docs/brief.md in this repo is the original research brief; its edge cases are all covered by the contract above.`;

// ---------------------------------------------------------------------------
// Areas. Each owns a disjoint file set and runs implement -> gate -> audit(<=3, fix between).
// ---------------------------------------------------------------------------

interface AreaSpec {
  name: string;
  title: string;
  persona: string;
  auditPersona: string;
  owns: string[];
  spec: string;
  gate?: "rustfmt" | "yaml";
  rsFiles?: string[];
}

const AREAS: AreaSpec[] = [
  {
    name: "goals",
    title: "docs/goals.md: goals, acceptance criteria, error model, edge cases",
    persona: "z-flutter-engineer",
    auditPersona: "z-flutter-engineer",
    owns: ["docs/goals.md"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: docs/goals.md (write this one file; create it new)
A complete goals document in the direct, concrete style of .refs/biometrics/docs/parity.md. Sections, all grounded in the frozen contract above:
1. **Goals** — what this library is (recording to file or stream for voice notes and level meters; NOT speech-to-text), the two deliverables (crates/nsh-microphone + packages/nsh-microphone, same API names and behavior), and the layout rule (SuperNSH/biometrics twin).
2. **Acceptance criteria** — a table where each criterion names its evidence: the CI job that runs it (format, rust matrix clippy+test, cross-check targets, typescript nsh check/build/test) and the Rust+TS test pair from the canonical list. Include the AGENTS.md invariants as criteria: every call resolves exactly once; nothing hangs; no panics on operation paths; every behaviour tested in both languages; status claims match CI.
3. **Error model** — the 8-kind table: kind, stable code, message, and which reference failure each kind covers (cpal BuildStreamError/StreamError kinds, Flutter record's PlatformException behaviors, react-native-audio-api's onError + permission states, unsupported targets). State that codes are stable and apps match on them.
4. **Edge cases** — the table mapping every case to a named test. Use every row of the canonical test list (all 13 edge_cases.rs names and their TS twins) plus the state-machine rows from recorder.rs. Columns: | Edge case | How the references handle it (record / react-native-audio-api / cpal) | Rust test | TypeScript test |. Cover at minimum: double start; pause/resume in every wrong state; stop twice; cancel when idle; amplitude when idle; zero sample rate; zero or three channels; empty path; zero bit rate on compressed encoder; unsupported encoder; undetermined -> request once; denied; cancelled permission prompt; config pass-through; chunks pause with the recording; late stop answer after cancel; dispose while recording; platform status codes preserved; encoder name parity. Then the brief's own cases as rows marked Platform only with the named library rule: route changes rebuild the audio graph (Apple: AVAudioEngine rebuild; record and RNAA both reconnect), call interruptions stop the tap and restart explicitly (AVAudioSession interruption), sample-rate mismatch records at the hardware rate and resamples, Android 14+ needs the microphone foreground-service type declared, revoked permission can yield silence instead of an error and the layers detect and report it.
5. **Out of scope** — speech-to-text; device/emulator runs in CI (AGENTS.md forbids them on dev machines and CI has no devices); demo app; streaming over the native Rust-TSX bridge in NSH 0.4.0.
Status wording rule: this repo has not run CI yet (no commits allowed in this task), so nothing is claimed Done; criteria say what CI will prove.${RESULT_FOOTER}`,
  },
  {
    name: "parity",
    title: "docs/parity.md part 1: every public API of Flutter record mapped to ours",
    persona: "z-flutter-engineer",
    auditPersona: "z-flutter-engineer",
    owns: ["docs/parity.md"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: docs/parity.md part 1 (replace the scaffold table; this one file)
A second area appends the react-native-audio-api + cpal sections to this SAME file right after you finish, so your part must come first and be complete on its own. Structure: top matter, then your Flutter sections.
Top matter: title "Parity with Flutter record, react-native-audio-api and cpal"; the status vocabulary paragraph from the contract (Authored / Partial / Accepted / Platform only; Done reserved for after the first green CI run); the line "Rust test names are in crates/nsh-microphone/tests/. TypeScript tests are in packages/nsh-microphone/tests/." (biometrics' exact convention).
Your sections (read the real APIs in .refs/record/record_platform_interface/lib/src/ first — do not trust memory):
1. **Methods of record's RecordPlatform interface**: one row per method — create, start, stop, pause, resume, isRecording, isPaused, hasPermission, dispose, getAmplitude, isEncoderSupported, cancel, and the audio-session methods (manageAudioSession, setAudioSessionActive, setAudioSessionCategory). Columns: | record API | Rust | TypeScript | Status | Covered by |. Rust and TypeScript cells use OUR names from the contract (Recorder::start, Recorder.stop, ...). Covered by names the canonical test.
2. **RecordConfig fields and types**: encoder, bitRate, numChannels, sampleRate, path vs stream, device, android/ios sub-configs. Mark what we fold into RecordConfig and what is Accepted (kept, platform does nothing) or Platform only.
3. **AudioEncoder values**: all ten, our Encoder enum, same string names, per-platform support notes (is_encoder_supported answers it at runtime; the fake and memory backends script it).
4. **Failure mapping**: how record reports failures (PlatformException / RecordException) and which ERR_MICROPHONE_* code each maps to (the contract's reference mapping).
Status honesty: rows where the behavior exists in code and a named test exists but CI has not run are Authored, not Done. Device-only rows are Platform only.${RESULT_FOOTER}`,
  },
  {
    name: "parity-rn",
    title: "docs/parity.md part 2: react-native-audio-api and cpal rows",
    persona: "z-react-native-engineer",
    auditPersona: "z-react-native-engineer",
    owns: ["docs/parity.md"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: docs/parity.md part 2 (append only)
This area is strictly chained after the "parity" area, which owns the same file and writes the top matter + Flutter sections first. When you start, docs/parity.md already exists. APPEND your sections; never rewrite theirs.
Read the real API first: .refs/react-native-audio-api/packages/react-native-audio-api/src/core/AudioRecorder.ts, its types (src/types/*.ts: AudioRecorderOptions, AudioRecorderFileOptions, FileFormat, FileInfo, Result), and .refs/cpal/src/{platform,error,stream}.rs.
1. **AudioRecorder methods**: start, stop, pause, resume, isRecording, isPaused, enableFileOutput, disableFileOutput, connect/disconnect (adapter node), onAudioReady/clearOnAudioReady, onError/clearOnError, getCurrentDuration, inputLatency, static isRecordingOngoing, consumeLastRecordingResult. Columns | react-native-audio-api | Rust | TypeScript | Status | Covered by |. Ours: chunk delivery is the ChunkSink passed to start (Rust) and the onChunk callback (TS); getCurrentDuration maps to StoppedRecording.duration_ms only at stop (Partial; say exactly what we do); inputLatency not in the API (Absent — state the omission plainly).
2. **AudioRecorderOptions and file options**: sampleRate, bitRate, channels, encoding, isStreaming, fileDirectory/fileName/fileFormat. Map to RecordConfig fields; fileFormat -> our Encoder subset (which FileFormat values map to which Encoder names); directory/flat path differences named.
3. **Permission surface**: requestPermissions/checkPermissions and their "granted"/"denied"/"undetermined" strings — we adopt these exact names (PermissionState::name).
4. **Errors and events**: their onError event and thrown errors -> our coded RecorderError (the reference mapping).
5. **cpal as used by the desktop layers** (windows.rs/linux.rs): default_input_device/build_input_stream/StreamConfig and pause/play as 0.18 names them, DeviceNotAvailable and FormatNotSupported mapped to our codes with status kept where the host gives one. Covered by: the config pass-through and unsupported-encoder tests (the cpal layer itself is Platform only on CI runners).
Honesty rule: Authored until CI runs; Platform only for device behavior; Partial with the limit named; Absent when we deliberately do not have it (say why).${RESULT_FOOTER}`,
  },
  {
    name: "core",
    title: "Rust core: workspace, error model, backend trait, Recorder state machine, FakeBackend",
    persona: "z-rust-engineer",
    auditPersona: "z-rust-engineer",
    owns: [
      "Cargo.toml",
      "crates/nsh-microphone/Cargo.toml",
      "crates/nsh-microphone/src/lib.rs",
      "crates/nsh-microphone/src/error.rs",
      "crates/nsh-microphone/src/options.rs",
      "crates/nsh-microphone/src/backend.rs",
      "crates/nsh-microphone/src/recorder.rs",
      "crates/nsh-microphone/src/fake.rs",
      "crates/nsh-microphone/src/platform/mod.rs",
      "crates/nsh-microphone/src/platform/unsupported.rs",
    ],
    gate: "rustfmt",
    rsFiles: [
      "crates/nsh-microphone/src/lib.rs",
      "crates/nsh-microphone/src/error.rs",
      "crates/nsh-microphone/src/options.rs",
      "crates/nsh-microphone/src/backend.rs",
      "crates/nsh-microphone/src/recorder.rs",
      "crates/nsh-microphone/src/fake.rs",
      "crates/nsh-microphone/src/platform/mod.rs",
      "crates/nsh-microphone/src/platform/unsupported.rs",
    ],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: the Rust core (the contract sections "src/error.rs", "src/options.rs", "src/backend.rs", "src/recorder.rs", "src/fake.rs", "src/platform/mod.rs and unsupported.rs", "src/lib.rs skeleton", and both manifests)
Files you own (create; every other area depends on these names resolving):
- Cargo.toml (root): convert to the workspace manifest per the contract. The crate stub at src/lib.rs (root) is DELETED: \`rm src/lib.rs\` and \`rmdir src\` (plain filesystem moves; no git commands). Your other files make the workspace complete without it.
- crates/nsh-microphone/Cargo.toml: exactly the dependency set in the contract (versions verified against crates.io: cpal 0.18.2, objc2 0.6 + block2 0.6 + objc2-avf-audio 0.3.2, jni 0.22.4 + ndk-context 0.1.1, nsh-core 0.4.0, thiserror 2, tokio 1.54.1; dev-deps tokio rt/macros/time + nsh-engine 0.4.0 + nsh-testing 0.4.0 default-features=false). Do NOT add features you were not given: objc2-avf-audio's list was verified against its real 0.3.2 feature set.
- The six core source files + platform/mod.rs + platform/unsupported.rs + lib.rs, exactly per contract.
Pattern files to imitate (read them first): .refs/biometrics/crates/nsh-biometrics/src/{error.rs, backend.rs, fake.rs, auth.rs, lib.rs} and its platform/mod.rs for the cfg-selection shape. Recorder's gate is biometrics' auth.rs Gate translated from prompts to recordings: busy while a recording runs, generation counter so a late stop answer after a cancel resolves nothing, tokio oneshot for the async settles (stop/cancel/dispose/permission calls), std Mutex for state.
The write_wav helper in platform/mod.rs is plain Rust (no cpal import — cpal is target-gated): a 44-byte canonical PCM16 WAV header, io errors map to RecorderError Backend.
Rules that will be audited: no unwrap/expect on operation paths; every future settles exactly once; no dead code; doc comments on every public item (1-2 lines, non-obvious constraints). Docs/README claims are NOT yours to write.${RESULT_FOOTER}`,
  },
  {
    name: "apple",
    title: "Apple platform bridge: AVAudioEngine input tap with session handling",
    persona: "z-rust-engineer",
    auditPersona: "z-unsafe-ffi-auditor",
    owns: ["crates/nsh-microphone/src/platform/apple.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/platform/apple.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/platform/apple.rs (this one file; nothing else)
cfg(target_vendor = "apple"): ONE NativeBackend implementing the Backend trait from the contract, serving iOS AND macOS (the brief decides: the Apple handling rules — AVAudioEngine input tap, session category, interruption and route-change handling — are ported into this native layer, which is also why macOS uses it instead of cpal: cpal has no session or interruption story, and docs/brief.md's edge cases demand them; decisions.md records this).
Deps already declared for your target by area core — use only these: objc2 0.6, block2 0.6, objc2-avf-audio 0.3.2 (features: std, alloc, AVAudioApplication, AVAudioBuffer, AVAudioEngine, AVAudioFile, AVAudioFormat, AVAudioIONode, AVAudioMixing, AVAudioNode, AVAudioSession, AVAudioSessionRoute, AVAudioSettings, AVAudioTypes, AVAudioTime). You may NOT edit Cargo.toml.
Behavior (each maps to a coded error; never panic; never hang; every call resolves exactly once):
- check/request permissions: iOS through AVAudioSession's permission API (recordPermission / requestRecordPermission); macOS: no AVCaptureDevice dep is declared, so macOS checkPermissions answers Granted when an input device exists (documented; Partial in parity). Undetermined -> request. Map denials to PermissionDenied.
- list_input_devices: AVAudioSession availableInputs on iOS; on macOS answer the single default input (id "default-input") — documented Partial.
- is_encoder_supported: Wav and Pcm16bits true (the layer writes PCM WAV itself via platform::write_wav); compressed encoders false in v1 (parity: Partial).
- start: build the AVAudioEngine graph (input node tap via installInputTap with the block2 closure), capture at the hardware rate (the brief's sample-rate mismatch rule: if config.sample_rate differs, convert to i16 chunks at config rate — a plain linear resample is acceptable and must be documented), convert Float32 samples to i16 AudioChunks and push through the ChunkSink. File output: write with AVAudioFile (or write_wav for PCM16) on stop. Session: on iOS set category PlayAndRecord, activate; observe interruption notifications -> pause the tap per the brief's rule and route-change notifications -> rebuild the engine graph. Register all observers on start; remove them on stop/cancel/dispose.
- pause/resume: engine pause or tap-hold; amplitude: the tap keeps running peak/average (EMA) fed to Amplitude; stop: remove the tap, deactivate the session (iOS), flush and close the file, answer StoppedRecording; cancel: remove the tap, discard the partial file; dispose: release the engine.
Error mapping: any objc2 call failing where the contract has a code -> that code; the platform's own numeric codes (AVAudioSession error codes, osstatus) go into RecorderError::with_status. Unavailable input hardware -> NotAvailable.
VERIFY EVERY API NAME before use: WebFetch https://docs.rs/crate/objc2-avf-audio/0.3.2 (and item pages like its AVAudioEngine struct page) for the exact generated Rust names (they are snake_case, e.g. input_node(), install_input_tap_on_output(), set_category_with_options(...)). block2 closures: Block::new with the exact signature docs.rs shows. Never guess a symbol.
This file is compiled only by the CI cross-check job (aarch64-apple-ios / aarch64-apple-ios-sim) — write it as if it will be type-checked. Read .refs/biometrics/crates/nsh-biometrics/src/platform/apple.rs first for the file's shape and doc-comment style.${RESULT_FOOTER}`,
  },
  {
    name: "android",
    title: "Android platform bridge: AudioRecord over JNI with foreground-service rules",
    persona: "z-rust-engineer",
    auditPersona: "z-unsafe-ffi-auditor",
    owns: ["crates/nsh-microphone/src/platform/android.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/platform/android.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/platform/android.rs (this one file; nothing else)
cfg(target_os = "android"): ONE NativeBackend implementing the Backend trait. Deps already declared by area core — use only: jni 0.22.4, ndk-context 0.1.1. You may NOT edit Cargo.toml.
Behavior (coded errors; never panic — every JNI result is checked and mapped; never hang; resolves exactly once):
- ndk_context::android_context() gives the JavaVM and the app Context; attach the current thread per jni 0.22.4's API.
- check/request permissions: Context.checkSelfPermission / requestPermissions against android.Manifest.permission.RECORD_AUDIO (API 23+; document the min API). Denied -> PermissionDenied; the framework's cancelled prompt -> Cancelled.
- list_input_devices: AudioManager.getDevices(ALL_INPUTS) -> InputDevice list (id, label from the device name).
- is_encoder_supported: AudioRecord's native PCM path -> Wav/Pcm16bits true; compressed formats are NOT wired in v1 -> false (parity: Partial; Android records PCM and file output goes through platform::write_wav).
- start: AudioRecord with MediaRecorder.AudioSource.MIC (record's default audio source), buffer size from AudioRecord.getMinBufferSize, read loop on a dedicated thread pushing AudioChunk i16 data through the ChunkSink; hardware rate is requested and a linear resample to config.sample_rate when they differ (document). File output via write_wav on stop. The Android 14+ microphone foreground-service type (RECORD_AUDIO foregroundServiceType in the manifest) is an APP manifest requirement: state it in the file's doc comment and note it for parity (Platform only; the library cannot grant it).
- stop: signal the read loop, join it with a bounded wait (never hang: if the thread does not exit, map to Backend with status), release the AudioRecord; cancel: same but discard; amplitude: peak/EMA over recent reads; dispose: release everything.
Error mapping: JNI exceptions -> RecorderError Backend with the exception's code in with_status where available. VERIFY every jni 0.22.4 method name against docs.rs before use (env.call_method, JNIEnv/JObject/GlobalRef lifetimes). Read .refs/biometrics/crates/nsh-biometrics/src/platform/android.rs for file shape (same jni + ndk-context pair) and .refs/record/record_android/ for the Android recording rules.
This file is compiled only by the CI cross-check job (aarch64-linux-android) — write it as if it will be type-checked.${RESULT_FOOTER}`,
  },
  {
    name: "windows",
    title: "Windows platform bridge: cpal WASAPI host",
    persona: "z-rust-engineer",
    auditPersona: "z-unsafe-ffi-auditor",
    owns: ["crates/nsh-microphone/src/platform/windows.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/platform/windows.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/platform/windows.rs (this one file; nothing else)
cfg(target_os = "windows"): ONE NativeBackend implementing the Backend trait on cpal 0.18.2 (declared by area core for windows+linux; you may NOT edit Cargo.toml). Verify every cpal 0.18 API name against docs.rs 0.18.2 before use — cpal reworked its API in the 0.18 line (devices(), default_input_device(), build_input_stream(...), and how pause/play are reached).
Behavior (coded errors; never panic; never hang; resolves exactly once):
- check/request permissions: WASAPI capture needs no runtime consent call in v1 -> Granted (documented; parity: Partial with the Windows privacy-settings caveat named).
- list_input_devices: cpal devices() filtered to input-capable devices, id from the device name (stable ids need extra crates — document).
- is_encoder_supported: Wav/Pcm16bits true (write_wav); compressed false (parity: Partial).
- start: pick the default input device (None -> NotAvailable), validate config against the device's supported range (FormatNotSupported / InvalidArgument per the mapping), build an f32 input stream, convert to i16 chunks at config.sample_rate (linear resample when the hardware rate differs — the brief's mismatch rule; document), push through the sink; keep peak/EMA for amplitude. File output via platform::write_wav on stop. Stream errors surface as Backend with the cpal error text in status.
- pause/resume: cpal's stream pause/play as 0.18 exposes them (verify); stop: drop the stream, flush the file; cancel: drop and discard; dispose: release.
Read .refs/cpal/src/{platform.rs, host.rs} and .refs/biometrics/crates/nsh-biometrics/src/platform/windows.rs for shape. Compiled by the CI rust job on windows-latest — write it as if type-checked.${RESULT_FOOTER}`,
  },
  {
    name: "linux",
    title: "Linux platform bridge: cpal ALSA host",
    persona: "z-rust-engineer",
    auditPersona: "z-unsafe-ffi-auditor",
    owns: ["crates/nsh-microphone/src/platform/linux.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/platform/linux.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/platform/linux.rs (this one file; nothing else)
cfg(target_os = "linux"): ONE NativeBackend implementing the Backend trait on cpal 0.18.2 (declared by area core; you may NOT edit Cargo.toml). Same rules as the windows area, with Linux specifics:
- The ALSA host needs libasound2-dev at build time: CI's LINUX_PACKAGES installs it (the ci-meta area writes that). State the requirement in the file's doc comment.
- list_input_devices via cpal devices(); PulseAudio/PipeWire expose the same ALSA interface (document that the names come through ALSA's plugin stack).
- Everything else mirrors the windows area's contract: permissions Granted in v1 (documented Partial), encoder support Wav/Pcm16bits only, hardware-rate capture with documented linear resample, write_wav file output, amplitude EMA, coded errors with cpal error text as status, never panic, never hang, resolves exactly once.
Verify cpal 0.18 API names against docs.rs. Read .refs/cpal/src/{platform.rs, host.rs}; note the scaffold README's "cpal on desktop" is realized by this file and windows.rs (decisions.md records why macOS uses the Apple layer instead). Compiled by the CI rust job on ubuntu-latest — write it as if type-checked.${RESULT_FOOTER}`,
  },
  {
    name: "web",
    title: "Web bridge: unsupported target",
    persona: "z-rust-engineer",
    auditPersona: "z-rust-engineer",
    owns: ["crates/nsh-microphone/src/platform/web.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/platform/web.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/platform/web.rs (this one file; nothing else)
cfg(target_arch = "wasm32"): NativeBackend where every Backend method answers Err(RecorderError::new(ErrorKind::NotAvailable)) — the exact biometrics precedent (their final commit: "report the web as an unsupported target"). Doc comment: "The web is an unsupported target." A one-line comment per method block is enough; no dead code, no TODO. Compiled by the CI cross-check wasm32 job.${RESULT_FOOTER}`,
  },
  {
    name: "nsh-surface",
    title: "NSH TSX layer: NativeRecorder class and the availability hook",
    persona: "z-nsh-engineer",
    auditPersona: "z-nsh-engineer",
    owns: ["crates/nsh-microphone/src/native.rs", "crates/nsh-microphone/src/availability.rs"],
    gate: "rustfmt",
    rsFiles: ["crates/nsh-microphone/src/native.rs", "crates/nsh-microphone/src/availability.rs"],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/src/{native.rs, availability.rs} (these two files; nothing else)
The contract section "src/native.rs and src/availability.rs" is your whole spec. lib.rs (area core) already declares mod native; mod availability; and re-exports + registers NativeRecorder, so your names must match the contract exactly.
Verify EVERY nsh-core macro and type you use against the published sources before using it: \`git -C .refs/nsh show v0.4.0:crates/nsh-core/src/prelude.rs\`, then follow the module files (js/ for JsError/JsObject/class macros, platform.rs for Lifecycle, reactive/ for Signal/stored/effect/cleanup/spawn/Owner). The baseline is what .refs/biometrics/crates/nsh-biometrics/src/{native.rs, availability.rs} use (they compile against nsh-core 0.4.0 in biometrics' CI); where biometrics' file and the v0.4.0 source disagree, v0.4.0 wins. If a v0.4.0 API you need genuinely does not exist, do NOT invent one: adapt to the closest v0.4.0-real form and name the adaptation in your result summary and blockers.
Inline #[cfg(test)] tests at the bottom of native.rs per the contract (encoder names, stream-output refusal, defaults) — they run in CI only.
The availability hook's reactivity contract (wakes only on change; re-checks on Lifecycle::Active; dropping the owner cancels the inflight read) is biometrics' exact structure with our Availability type.${RESULT_FOOTER}`,
  },
  {
    name: "ts-package",
    title: "TypeScript package: Recorder, coded errors, memory backend, rust bridge, hook",
    persona: "z-nsh-engineer",
    auditPersona: "z-nsh-engineer",
    owns: [
      "packages/nsh-microphone/package.json",
      "packages/nsh-microphone/tsconfig.json",
      "packages/nsh-microphone/nsh.json",
      "packages/nsh-microphone/src/index.ts",
      "packages/nsh-microphone/src/errors.ts",
      "packages/nsh-microphone/src/options.ts",
      "packages/nsh-microphone/src/backend.ts",
      "packages/nsh-microphone/src/recorder.ts",
      "packages/nsh-microphone/src/memory.ts",
      "packages/nsh-microphone/src/rust.ts",
      "packages/nsh-microphone/src/availability.ts",
    ],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: packages/nsh-microphone (the manifest trio + the eight src/*.ts files; nothing else — tests/ belongs to another area)
The contract section "packages/nsh-microphone" is your whole spec. Pattern files to imitate: .refs/biometrics/packages/nsh-biometrics/{package.json, tsconfig.json, nsh.json, src/*.ts}. The Recorder class mirrors the Rust Recorder's rules exactly: same validation order (InvalidArgument refusals before the gate is claimed), Undetermined -> one requestPermissions, busy -> ERR_MICROPHONE_IN_PROGRESS, encoder check -> FORMAT_NOT_SUPPORTED, generation counter so a late backend answer never settles a cancelled or superseded call, dispose making every later call reject ERR_MICROPHONE_NOT_AVAILABLE. The errors module mirrors the Rust codes and message wording EXACTLY (apps match on codes in both languages).
No node_modules and no lockfile: "nsh" and "nsh/test" come from the NSH host (see how biometrics' src/availability.ts imports from "nsh"). Do not add devDependencies, do not add App.tsx, do not create tests/ (another area owns it). TypeScript style: match biometrics' TS (2-space indent, double quotes, semicolons, readonly where immutable).${RESULT_FOOTER}`,
  },
  {
    name: "rust-tests",
    title: "Rust tests: every goal, edge case and error path on FakeBackend",
    persona: "z-rust-engineer",
    auditPersona: "z-rust-engineer",
    owns: [
      "crates/nsh-microphone/tests/recorder.rs",
      "crates/nsh-microphone/tests/errors.rs",
      "crates/nsh-microphone/tests/edge_cases.rs",
      "crates/nsh-microphone/tests/availability.rs",
    ],
    gate: "rustfmt",
    rsFiles: [
      "crates/nsh-microphone/tests/recorder.rs",
      "crates/nsh-microphone/tests/errors.rs",
      "crates/nsh-microphone/tests/edge_cases.rs",
      "crates/nsh-microphone/tests/availability.rs",
    ],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: crates/nsh-microphone/tests/ (the four test files; nothing else)
The contract section "Tests" is your spec: the exact frozen test function names, all driven through Recorder::with_backend(FakeBackend...) plus direct FakeBackend scripting (hold_stop/finish_stop/fail_next_*/push_chunk/counters). Pattern files: .refs/biometrics/crates/nsh-biometrics/tests/{auth.rs, availability.rs} (note their microtask-flush trick and the Headless availability harness).
- The tests assume the core API exists exactly as the contract states (another area writes it in parallel — missing files are NOT findings for you; write against the contract).
- You may NOT edit src/ or Cargo.toml. If the contract forces a test the API cannot express, write the closest honest test and name the gap in blockers.
- availability.rs adapts biometrics' Headless component harness with nsh-testing 0.4.0 / nsh-engine 0.4.0 (dev-deps already declared by core). Verify harness names against \`git -C .refs/nsh show v0.4.0:crates/nsh-testing/src/<path>\` and the biometrics file; if v0.4.0's harness differs, follow v0.4.0.
- tokio: use #[tokio::test] where the call needs a runtime (dev-dep features rt+macros+time are declared).
Tests are Authored (CI runs them after the human pushes); nothing runs locally.${RESULT_FOOTER}`,
  },
  {
    name: "ts-tests",
    title: "TypeScript tests: every goal, edge case and error path on memoryBackend",
    persona: "z-nsh-engineer",
    auditPersona: "z-nsh-engineer",
    owns: [
      "packages/nsh-microphone/tests/recorder.test.tsx",
      "packages/nsh-microphone/tests/errors.test.tsx",
      "packages/nsh-microphone/tests/edge_cases.test.tsx",
      "packages/nsh-microphone/tests/availability.test.tsx",
      "packages/nsh-microphone/tests/fixtures/availability.tsx",
    ],
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: packages/nsh-microphone/tests/ (the four test files + the fixtures dir; nothing else)
The contract section "Tests" is your spec: the exact frozen test titles, \`import { expect, test } from "nsh/test"\`, everything driven by memoryBackend() (drive helpers: finishStop, pushChunk, counters, setPermission, failNextStart, holdStop). Pattern files: .refs/biometrics/packages/nsh-biometrics/tests/{auth.test.tsx, availability.test.tsx, fixtures/availability.tsx} — copy their conventions (the microtask flush helper, the TSX fixture component pattern for the hook, toMatchObject({ code: "..." }) rejections).
- The tests assume the TS package API exists exactly as the contract states (another area writes it in parallel — missing files are NOT findings for you).
- You may NOT edit src/ or the manifests. If a frozen title cannot be honestly tested with the contract API, write the closest honest test and name the gap in blockers.
- availability.test.tsx uses the fixture component + useMicrophoneAvailability with a memory-backend Recorder.
Tests are Authored (CI's \`nsh test\` runs them after the human pushes); nothing runs locally.${RESULT_FOOTER}`,
  },
  {
    name: "ci-meta",
    title: "CI workflow, README, decisions, gitignore, status page, phase plan",
    persona: "general-pro",
    auditPersona: "general-pro",
    owns: [
      ".github/workflows/ci.yml",
      "README.md",
      "docs/decisions.md",
      ".gitignore",
      "site/index.html",
      ".zcode/workflow.ts",
    ],
    gate: "yaml",
    spec: `${CONTRACT}
${REFS_NOTE}

## Your area: the shared meta files (six files; nothing else)
1. **.github/workflows/ci.yml** (rewrite; the authored gate — AGENTS.md: every compile and test runs here, never on a developer machine). Keep the scaffold's header comment, concurrency, permissions and pinned action SHAs (actions/checkout@3d3c42e5..., dtolnay/rust-toolchain@683b588c...). Jobs, mirroring .refs/biometrics/.github/workflows/ci.yml but package-scoped:
   - format: ubuntu, rustfmt only, cargo fmt --all -- --check.
   - rust (matrix ubuntu/macos/windows): dtolnay stable + clippy component; a Linux step installs libasound2-dev pkg-config (env LINUX_PACKAGES like biometrics'); cargo clippy -p nsh-microphone --all-targets -- -D warnings; cargo test -p nsh-microphone --all-targets.
   - cross-check (matrix: macos+aarch64-apple-ios, macos+aarch64-apple-ios-sim, ubuntu+aarch64-linux-android with the NDK env lines biometrics uses, ubuntu+wasm32-unknown-unknown): cargo clippy -p nsh-microphone --target <t> --all-targets -- -D warnings.
   - typescript: ubuntu; install NSH v0.4.0 from SuperNSH/nsh-builds (the install.sh block biometrics uses, verbatim style); working-directory packages/nsh-microphone: nsh check; nsh build 2>&1 | tee build.log; ! grep -i warn build.log; nsh test.
2. **docs/decisions.md** (rewrite; keep a Decided section, drop stale Open items by resolving them). Decided entries: NSH version 0.4.0 (what biometrics pins; crates.io-verified; .refs/nsh carries the v0.4.0 tag for source reading); library choice (cpal 0.18.2 for Windows/Linux desktop; objc2-avf-audio 0.3.2 AVAudioEngine tap for ALL Apple targets including macOS, because the brief's edge cases demand interruption/route handling which cpal lacks — name the deviation from "cpal on desktop"); platforms in scope (iOS, macOS, Windows, Linux real; web = unsupported target answering ERR_MICROPHONE_NOT_AVAILABLE, the biometrics precedent). Plus the decisions that shape this build: the error-code set with stable codes; stream chunks delivered through ChunkSink with paused chunks dropped; the native TSX surface is file-output-only until a verified NSH 0.4.0 chunk callback exists (parity: Partial); examples/demo omitted (site/ is this repo's extra); AND THE GATES RECONCILIATION as its own entry: AGENTS.md forbids local builds/tests, so this task ran only pure-formatting/parse checks (rustfmt, cargo fmt --all -- --check, a YAML load); clippy, cargo test, nsh check/build/test are authored into ci.yml; the user forbade commits/pushes so no CI run happened yet — all status claims stay Authored until the first green run. Record the STEP 0 clone results: all five URLs resolved to their canonical repos (.refs/record = https://github.com/llfbandit/record, .refs/react-native-audio-api = https://github.com/software-mansion/react-native-audio-api, .refs/cpal = https://github.com/RustAudio/cpal, .refs/nsh = https://github.com/longbridge/nsh, .refs/biometrics = https://github.com/SuperNSH/biometrics); none failed.
3. **README.md** (update in place): Status section -> "**Authored: implemented; not yet verified by CI.** The crate and package are implemented and the CI workflow compiles and tests them; the first green run on master flips docs/parity.md rows to Done." (AGENTS.md: a claim with no passing run is unverified). Layout section: workspace + crates/nsh-microphone + packages/nsh-microphone + docs + site + .zcode/workflow.ts. Checks section: name the CI gates and keep "Every compile and test runs in GitHub Actions. Nothing is built on a developer machine."
4. **.gitignore**: keep target/, .DS_Store, Cargo.lock, .refs/; add .nsh/ and node_modules/ (biometrics' ignores).
5. **site/index.html**: status line -> "Status: authored. Implemented; CI verifies on the first run." Keep the page's plain style.
6. **.zcode/workflow.ts**: replace the scaffold phase plan with the real one: per-area phases implement -> audit (up to 3 rounds, fix between) -> scoped formatting gate; a join check over the whole tree; compile/test gates are CI-authored. This file is data; keep it small and honest.
References: .refs/biometrics/.github/workflows/ci.yml (the style to mirror), AGENTS.md, the existing scaffold files.${RESULT_FOOTER}`,
  },
];

// ---------------------------------------------------------------------------

interface AreaResult {
  name: string;
  status: "done" | "done-with-findings" | "failed";
  summary: string;
  auditRounds: number;
  converged: boolean;
  personaDelivery: string;
  gate: string;
  blockers: string[];
}

function jsonBlock(reply: unknown): Record<string, unknown> | null {
  if (reply !== null && typeof reply === "object") return reply as Record<string, unknown>;
  const raw = String(reply ?? "");
  const blocks = raw.match(/```json\s*([\s\S]*?)```/g);
  if (!blocks || blocks.length === 0) return null;
  const last = blocks[blocks.length - 1].replace(/^```json\s*/, "").replace(/```\s*$/, "");
  try {
    const parsed = JSON.parse(last);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

async function specialist(name: string, fallback: string): Promise<{ persona: string; source: string }> {
  const adapter =
    `You are the ${name} specialist for one area of a parallel workflow. ` +
    `Your full specialist contract follows verbatim — apply it to this area's work; ` +
    `where it conflicts with your ask, the ask wins.` + "\n\n";
  for (const p of [`.zcode/agents/${name}.md`, `${KIT}/${name}.md`]) {
    try {
      const content = await files.read(p);
      return { persona: adapter + String(content).replace(/^---\n[\s\S]*?\n---\n/, "").trim(), source: p };
    } catch {
      // try the next source
    }
  }
  return { persona: fallback, source: "fallback" };
}

function runOk(res: unknown): { ok: boolean; output: string } {
  const r = (res ?? {}) as Record<string, unknown>;
  const ok =
    r.exitCode === 0 || r.exit_code === 0 || r.code === 0 || r.success === true || r.ok === true;
  const output = String(r.output ?? r.stdout ?? r.stderr ?? JSON.stringify(r)).slice(0, 4000);
  return { ok, output };
}

function lineIn(persona: string, parsed: Record<string, unknown> | null): boolean {
  const line = parsed && typeof parsed.contractLine === "string" ? parsed.contractLine.trim() : "";
  return line.length > 0 && persona.includes(line);
}

async function recheckContractLine(handle: ReturnType<typeof agent>, persona: string): Promise<boolean> {
  const retry = jsonBlock(
    await ask(
      handle,
      `Reply once more, ending with the same json block but with contractLine holding ONE sentence copied VERBATIM from your specialist contract (your system prompt) — it must be an exact substring of it.`,
    ),
  );
  return lineIn(persona, retry);
}

async function rustfmtGate(rsFiles: string[], handle: ReturnType<typeof agent>): Promise<string> {
  const res = runOk(await world.run("rustfmt", ["--check", "--edition", "2024", ...rsFiles], { timeoutMs: 120000 }));
  if (res.ok) return "rustfmt clean";
  // Formatting is mechanical: one fix round, then re-check.
  await ask(
    handle,
    `rustfmt --check reported formatting differences in your files. Fix them (running \`rustfmt --edition 2024 <file>\` on your own files is allowed — it only rewrites formatting, no build). Differences:\n${res.output}\n${RESULT_FOOTER}`,
  );
  const res2 = runOk(await world.run("rustfmt", ["--check", "--edition", "2024", ...rsFiles], { timeoutMs: 120000 }));
  return res2.ok ? "rustfmt clean after one fix" : `rustfmt STILL FAILING: ${res2.output.slice(0, 600)}`;
}

async function yamlGate(): Promise<string> {
  const res = runOk(
    await world.run("ruby", ["-ryaml", "-e", "YAML.load_file('.github/workflows/ci.yml'); puts 'yaml-ok'"], {
      timeoutMs: 60000,
    }),
  );
  return res.ok && res.output.includes("yaml-ok")
    ? "ci.yml parses as YAML"
    : `ci.yml YAML check FAILED: ${res.output.slice(0, 400)}`;
}

function auditAsk(area: AreaSpec): string {
  const owned = area.owns.map((f) => `- ${f}`).join("\n");
  return `Audit (read-only; do NOT edit anything) one area of the nsh-microphone parity build in the current checkout.

Area: ${area.name} — ${area.title}
Files owned by this area (audit ONLY these):
${owned}

Everything else is another area's work: a file owned by a different area that is missing, empty or different is NOT a finding for this audit (the run's join phase checks cross-area consistency). All areas implement in parallel, so other files may not exist yet.

Audit against the frozen contract below and the reference files in .refs/ (read them; they are the ground truth). Find ONLY real, actionable defects in the owned files: contract violations (wrong names, wrong codes, missing resolve-exactly-once guarantees, unwrap/expect on operation paths, dead code, comments longer than two lines or restating the obvious, missing docs on public Rust items), divergence from the biometrics pattern where the contract demands it, and any claim that states unverified success. An owned file that is missing or empty IS a finding.

${CONTRACT}

${AUDIT_FOOTER}`;
}

async function runArea(area: AreaSpec): Promise<AreaResult> {
  const out: AreaResult = {
    name: area.name,
    status: "failed",
    summary: "",
    auditRounds: 0,
    converged: false,
    personaDelivery: "unverified",
    gate: "none",
    blockers: [],
  };
  try {
    report({ name: area.name, phase: "implementing", summary: area.title }, "areas");

    const implPersona = await specialist(area.persona, PRO_FALLBACK);
    const impl = agent(`Implement·${area.name}`, { system: implPersona.persona, model: MODELS.pro });

    let first = jsonBlock(await ask(impl, area.spec + RESULT_FOOTER));
    if (!first || first.done !== true) {
      first = jsonBlock(
        await ask(
          impl,
          `Your previous reply did not carry a parsable json result block with "done": true. Finish the work if incomplete and reply again, ending with the exact json block from the instructions.`,
        ),
      );
    }
    if (!first) throw new Error("implementer returned no parsable result block");
    out.personaDelivery = lineIn(implPersona.persona, first) || (await recheckContractLine(impl, implPersona.persona))
      ? "verified"
      : "unverified";
    if (Array.isArray(first.blockers)) out.blockers = (first.blockers as unknown[]).map(String).slice(0, 10);

    // Scoped deterministic gate (pure formatting / parse only — AGENTS.md forbids builds locally).
    report({ name: area.name, phase: "gating", summary: "scoped formatting/parse gate" }, "areas");
    if (area.gate === "rustfmt" && area.rsFiles) out.gate = await rustfmtGate(area.rsFiles, impl);
    else if (area.gate === "yaml") out.gate = await yamlGate();

    // Audit loop: up to 3 audit rounds, fix between, until zero issues or the cap.
    const auditPersona = await specialist(area.auditPersona, PRO_FALLBACK);
    let rounds = 0;
    let converged = false;
    let leftover: string[] = [];
    while (true) {
      report({ name: area.name, phase: "auditing", summary: `audit round ${rounds + 1} of at most 3` }, "areas");
      const auditor = agent(`Audit·${area.name}·${rounds + 1}`, {
        system: auditPersona.persona + "\n\nThis assignment is READ-ONLY: never edit, create or delete anything.",
        model: MODELS.pro,
      });
      let audit = jsonBlock(await ask(auditor, auditAsk(area)));
      if (!audit || !Array.isArray(audit.findings)) {
        audit = jsonBlock(await ask(auditor, `Reply again, ending with the exact json findings block described.`));
      }
      if (!audit || !Array.isArray(audit.findings)) {
        out.blockers.push("audit reply was never parsable; audit stopped inconclusive");
        break;
      }
      if (!lineIn(auditPersona.persona, audit)) out.blockers.push(`audit round ${rounds + 1}: auditor contract line unverified`);
      rounds += 1;
      const findings = (audit.findings as unknown[]).map((f) => (typeof f === "string" ? f : JSON.stringify(f)));
      if (findings.length === 0) {
        converged = true;
        break;
      }
      if (rounds >= 3) {
        leftover = findings;
        break;
      }
      report({ name: area.name, phase: "fixing", summary: `${findings.length} findings -> fix round ${rounds}` }, "areas");
      await ask(
        impl,
        `Your area (${area.name}) was audited against the frozen contract. Apply EXACTLY these findings to your owned files; change nothing outside them:\n\n` +
          findings.join("\n\n") +
          RESULT_FOOTER,
      );
    }
    out.auditRounds = rounds;
    out.converged = converged;
    out.status = converged ? "done" : "done-with-findings";
    const files = Array.isArray(first.files) ? (first.files as unknown[]).map(String).join(", ") : "";
    out.summary =
      String(first.summary ?? "") +
      ` | gate: ${out.gate} | audits: ${rounds}${converged ? " (clean)" : leftover.length ? ` (cap hit; ${leftover.length} findings left: ${leftover.slice(0, 3).join(" | ")})` : " (inconclusive)"}` +
      (files ? ` | files: ${files}` : "");
    report(
      { name: area.name, phase: converged ? "done" : "failed", summary: out.summary.slice(0, 300) },
      "areas",
    );
    return out;
  } catch (error) {
    out.status = "failed";
    out.summary = `failed: ${String(error).slice(0, 400)}`;
    report({ name: area.name, phase: "failed", summary: out.summary }, "areas");
    return out;
  }
}

// ---------------------------------------------------------------------------

artifact.board("areas", {
  key: "name",
  status: "phase",
  columns: ["implementing", "gating", "auditing", "fixing", "done", "failed"],
});

phase("STEP 0 guard: the five reference clones from the launcher are present before any area starts");
try {
  for (const ref of ["record", "react-native-audio-api", "cpal", "nsh", "biometrics"]) {
    await files.read(`.refs/${ref}/README.md`);
  }
  const tag = runOk(
    await world.run("git", ["-C", ".refs/nsh", "show", "v0.4.0:crates/nsh-core/src/lib.rs"], { timeoutMs: 30000 }),
  );
  if (!tag.ok) throw new Error("the .refs/nsh v0.4.0 tag is not readable: " + tag.output);
} catch (error) {
  return {
    conclusion: `Blocked before any area started: STEP 0 references are incomplete (${String(error).slice(0, 300)}). Nothing was implemented.`,
    areas: [],
    notes: ["STEP 0 was performed pre-launch; if this guard fails, the checkout changed underneath the run."],
  };
}

phase("Run every area's own chain in parallel: implement, gate, audit up to 3 rounds with fixes between");

// docs/parity.md has two writers (Flutter part, then RN/cpal part) chained on one file.
const parityFirst = runArea(AREAS.find((a) => a.name === "parity")!);
const paritySecond = (async (): Promise<AreaResult> => {
  await parityFirst;
  return runArea(AREAS.find((a) => a.name === "parity-rn")!);
})();
const others = AREAS.filter((a) => a.name !== "parity" && a.name !== "parity-rn").map((a) => runArea(a));
const settled = await Promise.all([parityFirst, paritySecond, ...others]);
const ordered: AreaResult[] = AREAS.map((a) => settled.find((r) => r.name === a.name)).filter(
  (r): r is AreaResult => Boolean(r),
);

phase("Join: one whole-tree consistency check, route findings to owners once, re-check once");
const joinPersona = await specialist("general-pro", PRO_FALLBACK);
const joinAuditor = agent("Join audit", { system: joinPersona.persona, model: MODELS.pro });

function ownerOf(file: string): string {
  const exact = AREAS.find((a) => a.owns.includes(file));
  if (exact) return exact.name;
  let best = "";
  let bestLen = 0;
  for (const a of AREAS) {
    for (const f of a.owns) {
      const dir = f.includes("/") ? f.slice(0, f.lastIndexOf("/") + 1) : "";
      if (dir && file.startsWith(dir) && dir.length > bestLen) {
        best = a.name;
        bestLen = dir.length;
      }
    }
  }
  return best;
}

const joinFindings: { file: string; issue: string; fix: string }[] = [];
try {
  const canonicalTests = [
    "r_start_while_recording_is_refused_with_in_progress",
    "r_pause_and_resume_track_the_state_machine",
    "r_pause_when_idle_is_invalid_state",
    "r_resume_when_recording_is_invalid_state",
    "r_stop_when_idle_is_invalid_state",
    "r_stop_twice_resolves_once_then_invalid_state",
    "r_cancel_when_idle_is_invalid_state",
    "r_amplitude_when_idle_is_invalid_state",
    "r_clones_share_one_gate",
    "r_dispose_while_recording_stops_and_later_calls_report_not_available",
    "r_every_kind_reports_its_stable_code",
    "r_a_platform_status_travels_with_its_error",
    "r_start_with_zero_sample_rate_is_invalid_argument",
    "r_start_with_zero_or_three_channels_is_invalid_argument",
    "r_start_with_an_empty_file_path_is_invalid_argument",
    "r_start_with_zero_bit_rate_on_a_compressed_encoder_is_invalid_argument",
    "r_an_unsupported_encoder_is_format_not_supported",
    "r_undetermined_permission_is_requested_once_then_start_proceeds",
    "r_denied_permission_is_permission_denied",
    "r_a_cancelled_permission_prompt_reports_cancelled",
    "r_the_config_reaches_the_backend",
    "r_chunks_pause_and_resume_with_the_recording",
    "r_a_late_stop_answer_after_cancel_is_ignored",
    "r_a_backend_error_keeps_its_code_and_status",
    "r_each_encoder_has_the_name_tsx_uses",
    "r_availability_wakes_only_on_change",
  ].join(", ");
  const joinAsk = `You are the join auditor of a parallel build of the nsh-microphone Rust crate + TypeScript package in the current checkout. All areas have finished; now check CROSS-AREA consistency (each area's own audit already ran). Read files as needed; do NOT edit anything.

Check exactly these, and nothing else:
1. Contract file inventory: every file of the layout exists (crates/nsh-microphone/src/{lib,error,options,backend,recorder,fake,native,availability}.rs, crates/nsh-microphone/src/platform/{mod,unsupported,apple,android,windows,linux,web}.rs, crates/nsh-microphone/tests/{recorder,errors,edge_cases,availability}.rs, packages/nsh-microphone/{package.json,nsh.json,tsconfig.json} + src/{index,errors,options,backend,recorder,memory,rust,availability}.ts + tests/{recorder,errors,edge_cases,availability}.test.tsx, docs/{goals,parity,decisions}.md, .github/workflows/ci.yml). Name the missing ones.
2. The 8 ERR_MICROPHONE_* codes and their messages are IDENTICAL across crates/nsh-microphone/src/error.rs, packages/nsh-microphone/src/errors.ts, and docs/goals.md's error table.
3. The 10 encoder names are identical across crates/nsh-microphone/src/options.rs (Encoder::name), packages/nsh-microphone/src/options.ts, and docs/goals.md.
4. Every Rust test name in docs/goals.md's edge-case table and docs/parity.md's Covered-by column appears in the canonical list, and the canonical list's names exist verbatim in crates/nsh-microphone/tests/*.rs (TS titles exist in packages/nsh-microphone/tests/*.test.tsx). Canonical Rust names: ${canonicalTests}
5. Status-claim honesty (AGENTS.md): README.md, docs/parity.md, docs/goals.md, site/index.html and .zcode/workflow.ts claim nothing Done/working — implemented work is Authored (CI never ran: no commits or pushes were allowed). Any "Done"/"works"/"passing" claim without a green run is a finding.
6. .github/workflows/ci.yml runs the four gates the brief names: cargo fmt --all -- --check, cargo clippy (package-scoped, --all-targets, -D warnings), cargo test (package-scoped, --all-targets), and the TypeScript typecheck+tests (nsh check / nsh build with a warning grep / nsh test).
7. Whole-tree formatting: cargo fmt --all -- --check runs mechanically outside this audit; skip re-checking formatting.

Report ONLY actionable cross-area findings.
${AUDIT_FOOTER}`;
  let join = jsonBlock(await ask(joinAuditor, joinAsk));
  if (!join || !Array.isArray(join.findings)) {
    join = jsonBlock(await ask(joinAuditor, `Reply again, ending with the exact json findings block.`));
  }
  if (join && Array.isArray(join.findings)) {
    for (const f of join.findings as Record<string, unknown>[]) {
      joinFindings.push({ file: String(f.file ?? ""), issue: String(f.issue ?? ""), fix: String(f.fix ?? "") });
    }
  } else {
    ordered.push({
      name: "join",
      status: "done-with-findings",
      summary: "join audit reply was never parsable; cross-area consistency is unverified",
      auditRounds: 0,
      converged: false,
      personaDelivery: "unverified",
      gate: "none",
      blockers: ["join audit unparseable"],
    });
  }

  // cargo fmt --all -- --check at the join: it needs every area's files (pure formatting).
  const fmt = runOk(await world.run("cargo", ["fmt", "--all", "--", "--check"], { timeoutMs: 120000 }));

  if (joinFindings.length > 0 || !fmt.ok) {
    // Route each finding to the area that owns the file; the fmt diffs are parsed per file.
    const byArea = new Map<string, string[]>();
    const add = (areaName: string, finding: string) => {
      const list = byArea.get(areaName) ?? [];
      list.push(finding);
      byArea.set(areaName, list);
    };
    for (const f of joinFindings) add(ownerOf(f.file) || "core", `${f.file}: ${f.issue} -> ${f.fix}`);
    if (!fmt.ok) {
      const diffFiles = [...fmt.output.matchAll(/Diff in (\S+) at line/g)].map((m) => m[1]);
      if (diffFiles.length > 0) {
        for (const file of [...new Set(diffFiles)]) add(ownerOf(file) || "core", `cargo fmt --check differences in ${file} — apply rustfmt formatting to it`);
      } else {
        add("core", `cargo fmt --all -- --check reports differences that could not be attributed to a file — find and format them:\n${fmt.output.slice(0, 1200)}`);
      }
    }

    for (const [areaName, findings] of byArea) {
      const spec = AREAS.find((a) => a.name === areaName);
      if (!spec) continue;
      report({ name: areaName, phase: "fixing", summary: "join findings -> one routed fix round" }, "areas");
      const fixPersona = await specialist(spec.persona, PRO_FALLBACK);
      const fixer = agent(`Join fix·${areaName}`, { system: fixPersona.persona, model: MODELS.pro });
      await ask(
        fixer,
        `The join audit found cross-area defects in files you own. Apply EXACTLY these; change nothing outside your owned files (${spec.owns.join(", ")}):\n\n` +
          findings.join("\n\n") +
          RESULT_FOOTER,
      );
    }
  }

  // One re-check by the same auditor, plus the whole-tree fmt gate again.
  let still = -1;
  if (joinFindings.length > 0) {
    const recheck = jsonBlock(
      await ask(
        joinAuditor,
        `Fix rounds ran for the earlier findings. Re-check the same points against the current tree. Report ONLY findings that still stand (old, already-fixed findings without standing defects are not findings).\n${AUDIT_FOOTER}`,
      ),
    );
    still = recheck && Array.isArray(recheck.findings) ? (recheck.findings as unknown[]).length : -1;
  }
  const fmtFinal = runOk(await world.run("cargo", ["fmt", "--all", "--", "--check"], { timeoutMs: 120000 }));
  const joinClean = (joinFindings.length === 0 || still === 0) && fmt.ok && fmtFinal.ok;
  ordered.push({
    name: "join",
    status: joinClean ? "done" : "done-with-findings",
    summary:
      `initial cross-area findings: ${joinFindings.length}; ` +
      (joinFindings.length === 0
        ? "consistency clean on first pass"
        : still === 0
          ? "0 standing after one routed fix round"
          : still < 0
            ? "re-check unparseable (unverified)"
            : `${still} still standing after one routed fix round`) +
      `; cargo fmt --all -- --check: ${fmtFinal.ok ? "clean" : "STILL FAILING: " + fmtFinal.output.slice(0, 300)}`,
    auditRounds: joinFindings.length > 0 ? 2 : 1,
    converged: joinClean,
    personaDelivery: "unverified",
    gate: fmtFinal.ok ? "cargo fmt --all -- --check clean" : "cargo fmt failing",
    blockers: [],
  });
} catch (error) {
  ordered.push({
    name: "join",
    status: "failed",
    summary: `join phase failed: ${String(error).slice(0, 300)}`,
    auditRounds: 0,
    converged: false,
    personaDelivery: "unverified",
    gate: "none",
    blockers: [],
  });
}

phase("Final report");
const finalJoin = ordered.find((r) => r.name === "join");
const areaOutcomes = ordered.filter((r) => r.name !== "join");
const failedAreas = areaOutcomes.filter((r) => r.status === "failed");
const unconverged = areaOutcomes.filter((r) => r.status === "done-with-findings");
const lines = areaOutcomes.map(
  (r) =>
    `- ${r.name}: ${r.status.toUpperCase()} after ${r.auditRounds} audit round(s)${r.converged ? " (converged)" : " (NOT converged)"}, gate: ${r.gate}, persona ${r.personaDelivery}. ${r.summary}`,
);
const conclusion = [
  `nsh-microphone parity build: ${areaOutcomes.length} areas ran, ${areaOutcomes.length - failedAreas.length} produced their files, ${failedAreas.length} failed.`,
  failedAreas.length ? `Failed areas: ${failedAreas.map((r) => r.name).join(", ")} (their summaries name the error).` : "",
  unconverged.length
    ? `Did NOT converge within the 3-audit cap: ${unconverged.map((r) => r.name).join(", ")} — leftover findings are quoted in each area's summary.`
    : `Every area's audit loop reached zero findings within the cap.`,
  `Join check: ${finalJoin ? finalJoin.summary : "not run"}.`,
  `GATES ARE AUTHORED, NOT EXECUTED: per AGENTS.md nothing is built or tested on a developer machine, and the brief forbade commits and pushes, so .github/workflows/ci.yml carries the cargo fmt/clippy/test and nsh check/build/test gates for CI to run after a human pushes. Every status claim in docs and README stays "Authored (awaiting first CI run)".`,
  ...lines,
]
  .filter(Boolean)
  .join("\n");

return {
  conclusion,
  areas: ordered,
  notes: [
    "Model pin: every agent ran on account:zai-individual-coding-plan/GLM-5.3$high (the user asked for GLM-5.3 Max everywhere); no flash anywhere — no vision work exists in this run.",
    "Audit budget: 3 audit rounds per area, fix between rounds, until zero issues or the cap; capped areas are reported as done-with-findings with the leftover findings quoted.",
    "STEP 0 ran pre-launch: all five references cloned into .refs/ (record, react-native-audio-api, cpal, nsh, biometrics), .gitignore already lists .refs/, and the nsh v0.4.0 tag was fetched so areas read the exact published crate sources. This script verified all of it before any area started.",
    "Dependency versions were verified live against the crates.io sparse index before the run: cpal 0.18.2, objc2 0.6 / block2 0.6 / objc2-avf-audio 0.3.2 (feature list verified), jni 0.22.4, ndk-context 0.1.1, nsh-core/nsh-engine/nsh-testing 0.4.0, thiserror 2, tokio 1.54.1.",
    "The two parity areas chain on one shared file (docs/parity.md): parity-rn starts only after parity finishes. That is the only sequencing in the run besides the join; every other area ran fully in parallel with disjoint files.",
    "personaDelivery 'unverified' means the specialist contract line could not be confirmed verbatim; it is reported, never faked.",
  ],
};
