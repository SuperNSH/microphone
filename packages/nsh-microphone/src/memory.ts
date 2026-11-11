import type { ChunkSink, RecorderBackend } from "./backend";
import { recorderError, type RecorderErrorCode } from "./errors";
import type { Amplitude, AudioChunk, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording } from "./options";

/** The answers a memory backend gives. Each call answers what the config says. */
export interface MemoryRecorderConfig {
  /** What `checkPermissions` answers. Defaults to granted. */
  permission?: PermissionState;
  /** What `listInputDevices` answers. Defaults to one built-in microphone. */
  devices?: InputDevice[];
  /** What `isEncoderSupported` answers. Defaults to true. */
  encoderSupported?: boolean;
}

/** A backend the test drives: it finishes stops, feeds chunks and counts what was asked. */
export interface MemoryRecorder extends RecorderBackend {
  /** Changes what `checkPermissions` answers. */
  setPermission(state: PermissionState): void;
  /** Makes the next `start` reject with this code, once. */
  failNextStart(code: RecorderErrorCode): void;
  /** Makes the next `stop` wait for `finishStop`. */
  holdStop(): void;
  /** Answers the held stop: its recording, or a code to reject with. False when nothing is held. */
  finishStop(result: StoppedRecording | RecorderErrorCode): boolean;
  /** Delivers the chunk while a recording runs; paused recordings drop it. Says which happened. */
  pushChunk(chunk: AudioChunk): boolean;
  recording(): boolean;
  lastConfig(): RecordConfig | undefined;
  starts(): number;
  pauses(): number;
  resumes(): number;
  stops(): number;
  cancels(): number;
  disposals(): number;
  chunks(): number;
}

/** A memory backend, for tests and for apps that run without a microphone. */
export function memoryBackend(config: MemoryRecorderConfig = {}): MemoryRecorder {
  let permission = config.permission ?? "granted";
  let devices: InputDevice[] = config.devices ?? [{ id: "built-in-mic", label: "Built-in microphone" }];
  let encoderSupported = config.encoderSupported ?? true;
  let failStart: RecorderErrorCode | null = null;
  let holdArmed = false;
  let heldStop: ((result: StoppedRecording | RecorderErrorCode) => void) | null = null;
  let sink: ChunkSink | null = null;
  let running = false;
  let paused = false;
  let last: RecordConfig | undefined;
  let startCount = 0;
  let pauseCount = 0;
  let resumeCount = 0;
  let stopCount = 0;
  let cancelCount = 0;
  let disposalCount = 0;
  let chunkCount = 0;

  const endSession = () => {
    running = false;
    paused = false;
    sink = null;
    heldStop = null;
  };

  return {
    checkPermissions: async () => permission,
    // The memory prompt grants; a test that needs a refusal overrides this method.
    requestPermissions: async () => "granted",
    listInputDevices: async () => [...devices],
    isEncoderSupported: async (_encoder: Encoder) => encoderSupported,
    start: (next: RecordConfig, nextSink: ChunkSink) => {
      last = next;
      if (failStart !== null) {
        const code = failStart;
        failStart = null;
        return Promise.reject(recorderError(code));
      }
      startCount += 1;
      sink = nextSink;
      running = true;
      paused = false;
      return Promise.resolve();
    },
    pause: async () => {
      pauseCount += 1;
      paused = true;
    },
    resume: async () => {
      resumeCount += 1;
      paused = false;
    },
    amplitude: async (): Promise<Amplitude> => ({ average: 0, peak: 0 }),
    stop: () => {
      stopCount += 1;
      endSession();
      if (!holdArmed) {
        return Promise.resolve({ path: "recording.m4a", durationMs: 0 });
      }
      holdArmed = false;
      return new Promise<StoppedRecording>((resolve, reject) => {
        heldStop = (result) => {
          heldStop = null;
          if (typeof result === "string") reject(recorderError(result));
          else resolve(result);
        };
      });
    },
    cancel: async () => {
      cancelCount += 1;
      endSession();
    },
    dispose: async () => {
      disposalCount += 1;
      endSession();
    },
    setPermission: (state) => {
      permission = state;
    },
    failNextStart: (code) => {
      failStart = code;
    },
    holdStop: () => {
      holdArmed = true;
    },
    finishStop: (result) => {
      const done = heldStop;
      heldStop = null;
      done?.(result);
      return done !== null;
    },
    pushChunk: (chunk) => {
      if (!running || paused || sink === null) return false;
      sink(chunk);
      chunkCount += 1;
      return true;
    },
    recording: () => running,
    lastConfig: () => last,
    starts: () => startCount,
    pauses: () => pauseCount,
    resumes: () => resumeCount,
    stops: () => stopCount,
    cancels: () => cancelCount,
    disposals: () => disposalCount,
    chunks: () => chunkCount,
  };
}
