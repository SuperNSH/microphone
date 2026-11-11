/** The encoders a recording can ask for, as Flutter's record names them. */
export type Encoder =
  | "aacLc"
  | "aacEld"
  | "aacHe"
  | "amrNb"
  | "amrWb"
  | "flac"
  | "opus"
  | "pcm16bits"
  | "vorbis"
  | "wav";

/** What a permission check answers, as react-native-audio-api names the states. */
export type PermissionState = "granted" | "denied" | "undetermined";

/** Where a recording goes: a file, or chunks through the sink. */
export type Output = { kind: "file"; path: string } | { kind: "stream" };

/** The options of one recording. A missing field takes the default noted on it. */
export interface RecordConfig {
  /** Samples per second. Defaults to 44100. */
  sampleRate?: number;
  /** 1 for mono or 2 for stereo. Defaults to 1. */
  channels?: number;
  /** Bits per second of the encoded output. Defaults to 128000. */
  bitRate?: number;
  /** The encoder to record with. Defaults to "aacLc". */
  encoder?: Encoder;
  /** Where the recording goes. Defaults to a stream. */
  output?: Output;
}

/** An input device the platform listed. */
export interface InputDevice {
  id: string;
  label?: string;
}

/** The loudness of the running recording, both normalized to 0..1. */
export interface Amplitude {
  average: number;
  peak: number;
}

/** What a finished recording left behind. A stream recording answers null for both. */
export interface StoppedRecording {
  path: string | null;
  durationMs: number | null;
}

/** One buffer of PCM samples, at the rate and channel count of the recording. */
export interface AudioChunk {
  data: Int16Array;
  sampleRate: number;
  channels: number;
}
