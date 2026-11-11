import type { ChunkSink, RecorderBackend } from "./backend";
import { asRecorderError, recorderError, type RecorderError, type RecorderErrorCode } from "./errors";
import type { Amplitude, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording } from "./options";
import { rustBackend } from "./rust";

export type RecordingState = "idle" | "recording" | "paused";

type StopOutcome = { ok: true; recording: StoppedRecording } | { ok: false; error: RecorderError };

/** Fills the defaults of a partial config, the twin of the Rust `RecordConfig::default()`. */
function resolveConfig(config: RecordConfig): RecordConfig {
  return {
    sampleRate: config.sampleRate ?? 44100,
    channels: config.channels ?? 1,
    bitRate: config.bitRate ?? 128000,
    encoder: config.encoder ?? "aacLc",
    output: config.output ?? { kind: "stream" },
  };
}

/** The Rust crate's refusals, in its order: rate, channels, bit rate, then the file path. */
function configRefusal(config: RecordConfig): RecorderErrorCode | null {
  if (config.sampleRate === 0) return "ERR_MICROPHONE_INVALID_ARGUMENT";
  if (config.channels === 0 || config.channels > 2) return "ERR_MICROPHONE_INVALID_ARGUMENT";
  if (config.bitRate === 0 && config.encoder !== "pcm16bits" && config.encoder !== "wav") {
    return "ERR_MICROPHONE_INVALID_ARGUMENT";
  }
  if (config.output.kind === "file" && config.output.path.trim() === "") {
    return "ERR_MICROPHONE_INVALID_ARGUMENT";
  }
  return null;
}

/**
 * Recording to a file or a stream of chunks, as Flutter's record names the calls. It runs on the
 * Rust crate unless it is given another backend. One recording runs at a time, and each call
 * resolves exactly once.
 */
export class Recorder {
  private readonly backend: RecorderBackend;
  private current: RecordingState = "idle";
  private busy = false;
  private stopping = false;
  private disposed = false;
  private generation = 0;
  private stopSettler: ((error: RecorderError) => void) | null = null;

  constructor(options: { backend?: RecorderBackend } = {}) {
    this.backend = options.backend ?? rustBackend();
  }

  state(): RecordingState {
    return this.current;
  }

  isRecording(): boolean {
    return this.current === "recording";
  }

  isPaused(): boolean {
    return this.current === "paused";
  }

  async checkPermissions(): Promise<PermissionState> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    try {
      return await this.backend.checkPermissions();
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  async requestPermissions(): Promise<PermissionState> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    try {
      return await this.backend.requestPermissions();
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  async listInputDevices(): Promise<InputDevice[]> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    try {
      return await this.backend.listInputDevices();
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  async isEncoderSupported(encoder: Encoder): Promise<boolean> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    try {
      return await this.backend.isEncoderSupported(encoder);
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  /**
   * Starts one recording. A bad config rejects with `ERR_MICROPHONE_INVALID_ARGUMENT` before the
   * gate is claimed; an undetermined permission is requested once and anything but granted
   * rejects with `ERR_MICROPHONE_PERMISSION_DENIED`; a busy recorder rejects with
   * `ERR_MICROPHONE_IN_PROGRESS` and an unsupported encoder with `ERR_MICROPHONE_FORMAT_NOT_SUPPORTED`.
   */
  async start(config: RecordConfig, onChunk?: ChunkSink): Promise<void> {
    const resolved = resolveConfig(config);
    const refusal = configRefusal(resolved);
    if (refusal !== null) throw recorderError(refusal);
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    await this.ensurePermission();
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    if (this.busy) throw recorderError("ERR_MICROPHONE_IN_PROGRESS");
    this.busy = true;
    this.current = "recording";
    let supported: boolean;
    try {
      supported = await this.backend.isEncoderSupported(resolved.encoder);
    } catch (error) {
      this.releaseGate();
      throw asRecorderError(error);
    }
    if (!supported) {
      this.releaseGate();
      throw recorderError("ERR_MICROPHONE_FORMAT_NOT_SUPPORTED");
    }
    if (this.disposed) {
      this.releaseGate();
      throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    }
    const sink: ChunkSink = onChunk ?? (() => {});
    try {
      await this.backend.start(resolved, sink);
    } catch (error) {
      this.releaseGate();
      throw asRecorderError(error);
    }
    if (this.disposed) {
      // The gate was lost after the platform started; the orphan must not survive it. Best
      // effort, like the Rust twin's `let _ =`: the refusal below is the answer either way.
      try {
        await this.backend.cancel();
      } catch {
        // Ignored on purpose.
      }
      this.releaseGate();
      throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    }
  }

  /** Pauses the running recording. Chunks are dropped while it stays paused. */
  async pause(): Promise<void> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    if (this.current !== "recording") throw recorderError("ERR_MICROPHONE_INVALID_STATE");
    const attempt = this.generation;
    try {
      await this.backend.pause();
    } catch (error) {
      throw asRecorderError(error);
    }
    // A cancel or a finished stop may have ended the recording while the backend paused it.
    if (this.generation === attempt && this.current === "recording") this.current = "paused";
  }

  /** Resumes a paused recording. */
  async resume(): Promise<void> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    if (this.current !== "paused") throw recorderError("ERR_MICROPHONE_INVALID_STATE");
    const attempt = this.generation;
    try {
      await this.backend.resume();
    } catch (error) {
      throw asRecorderError(error);
    }
    if (this.generation === attempt && this.current === "paused") this.current = "recording";
  }

  /** The loudness of the running or paused recording. */
  async amplitude(): Promise<Amplitude> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    if (this.current === "idle") throw recorderError("ERR_MICROPHONE_INVALID_STATE");
    try {
      return await this.backend.amplitude();
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  /**
   * Stops the recording and resolves with what it left behind. A stop from idle, or a second
   * stop while one ends, rejects with `ERR_MICROPHONE_INVALID_STATE`. A cancel settles a pending
   * stop with `ERR_MICROPHONE_CANCELLED`, and the backend's late answer then settles nothing.
   */
  stop(): Promise<StoppedRecording> {
    if (this.disposed) return Promise.reject(recorderError("ERR_MICROPHONE_NOT_AVAILABLE"));
    if (this.current === "idle" || this.stopping) {
      return Promise.reject(recorderError("ERR_MICROPHONE_INVALID_STATE"));
    }
    this.stopping = true;
    const attempt = this.generation;
    return new Promise<StoppedRecording>((resolve, reject) => {
      let settled = false;
      const settle = (outcome: StopOutcome) => {
        if (settled) return;
        settled = true;
        if (outcome.ok) resolve(outcome.recording);
        else reject(outcome.error);
      };
      const cancelledHere = (error: RecorderError) => settle({ ok: false, error });
      this.stopSettler = cancelledHere;
      this.backend.stop().then(
        (recording) => {
          if (this.stopSettler === cancelledHere) this.stopSettler = null;
          // A cancel or dispose already ended this recording; its late answer settles nothing.
          if (this.generation !== attempt) return;
          this.stopping = false;
          this.current = "idle";
          this.busy = false;
          settle({ ok: true, recording });
        },
        (error: unknown) => {
          if (this.stopSettler === cancelledHere) this.stopSettler = null;
          if (this.generation !== attempt) return;
          // The recording lives on: stop it again or cancel it.
          this.stopping = false;
          settle({ ok: false, error: asRecorderError(error) });
        },
      );
    });
  }

  /** Cancels the recording: the output is discarded, and a pending stop rejects cancelled. */
  async cancel(): Promise<void> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    if (this.current === "idle") throw recorderError("ERR_MICROPHONE_INVALID_STATE");
    this.abort();
    try {
      await this.backend.cancel();
    } catch (error) {
      throw asRecorderError(error);
    }
  }

  /**
   * Ends the handle. A running recording is cancelled first, and every later call rejects with
   * `ERR_MICROPHONE_NOT_AVAILABLE`.
   */
  async dispose(): Promise<void> {
    if (this.disposed) throw recorderError("ERR_MICROPHONE_NOT_AVAILABLE");
    // Claimed before any await: a start in flight then fails its disposed checks, not a silent Ok.
    this.disposed = true;
    let cancelError: unknown = null;
    if (this.current !== "idle") {
      this.abort();
      try {
        await this.backend.cancel();
      } catch (error) {
        cancelError = error;
      }
    }
    try {
      await this.backend.dispose();
    } finally {
      this.disposed = true;
    }
    if (cancelError !== null) throw asRecorderError(cancelError);
  }

  /** Asks the permission, requesting once when it is undetermined, and refuses when not granted. */
  private async ensurePermission(): Promise<void> {
    let permission: PermissionState;
    try {
      permission = await this.backend.checkPermissions();
      if (permission === "undetermined") permission = await this.backend.requestPermissions();
    } catch (error) {
      // A cancelled prompt keeps its code, and so does any other backend failure.
      throw asRecorderError(error);
    }
    if (permission !== "granted") throw recorderError("ERR_MICROPHONE_PERMISSION_DENIED");
  }

  /** Frees a gate a failing start claimed; a running recording ends through abort(). */
  private releaseGate(): void {
    this.current = "idle";
    this.busy = false;
  }

  /** Ends the session at once: frees the gate and settles a pending stop as cancelled. */
  private abort(): void {
    this.generation += 1;
    const settle = this.stopSettler;
    this.stopSettler = null;
    this.stopping = false;
    this.current = "idle";
    this.busy = false;
    settle?.(recorderError("ERR_MICROPHONE_CANCELLED"));
  }
}
