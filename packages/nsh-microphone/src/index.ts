export { Recorder, type RecordingState } from "./recorder";
export { type ChunkSink, type RecorderBackend } from "./backend";
export { RecorderError, asRecorderError, recorderError, type RecorderErrorCode } from "./errors";
export { memoryBackend, type MemoryRecorder, type MemoryRecorderConfig } from "./memory";
export {
  type Amplitude,
  type AudioChunk,
  type Encoder,
  type InputDevice,
  type Output,
  type PermissionState,
  type RecordConfig,
  type StoppedRecording,
} from "./options";
export {
  type Availability,
  type AvailabilityState,
  type MicrophoneAvailability,
  type UseMicrophoneAvailabilityOptions,
  useMicrophoneAvailability,
} from "./availability";
export { rustBackend } from "./rust";
