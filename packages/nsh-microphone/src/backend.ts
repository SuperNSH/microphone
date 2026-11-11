import type { Amplitude, AudioChunk, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording } from "./options";

/** Receives each chunk of a stream recording. Chunks are dropped while the recording is paused. */
export interface ChunkSink {
  (chunk: AudioChunk): void;
}

/** What a `Recorder` runs on: the Rust crate, or a memory backend in tests. */
export interface RecorderBackend {
  checkPermissions(): Promise<PermissionState>;
  requestPermissions(): Promise<PermissionState>;
  listInputDevices(): Promise<InputDevice[]>;
  isEncoderSupported(encoder: Encoder): Promise<boolean>;
  start(config: RecordConfig, sink: ChunkSink): Promise<void>;
  pause(): Promise<void>;
  resume(): Promise<void>;
  amplitude(): Promise<Amplitude>;
  stop(): Promise<StoppedRecording>;
  cancel(): Promise<void>;
  dispose(): Promise<void>;
}
