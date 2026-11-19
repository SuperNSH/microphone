import { NativeRecorder, type NativeRecordConfig } from "nsh-microphone/native";
import type { ChunkSink, RecorderBackend } from "./backend";
import { asRecorderError, recorderError } from "./errors";
import type { Amplitude, Encoder, InputDevice, PermissionState, RecordConfig, StoppedRecording } from "./options";

/**
 * The Rust crate's recording (`nsh-microphone/native`). The app or package must list the crate
 * under `"rust"` in `nsh.json`; importing this module without it fails the bundle.
 */
export function rustBackend(): RecorderBackend {
  return new RustBackend();
}

/** Only the fields the crate reads; a file output becomes `path`, and an absent field keeps the crate's default. */
function nativeConfig(config: RecordConfig): NativeRecordConfig {
  const native: NativeRecordConfig = {};
  if (config.sampleRate !== undefined) native.sampleRate = config.sampleRate;
  if (config.channels !== undefined) native.channels = config.channels;
  if (config.bitRate !== undefined) native.bitRate = config.bitRate;
  if (config.encoder !== undefined) native.encoder = config.encoder;
  if (config.output?.kind === "file") native.path = config.output.path;
  return native;
}

class RustBackend implements RecorderBackend {
  private readonly native = new NativeRecorder();
  /** Ends the chunk listener of the running stream recording, when there is one. */
  private stopChunks: (() => void) | null = null;

  checkPermissions(): Promise<PermissionState> {
    return this.settle(async () => (await this.native.checkPermissions()) as PermissionState);
  }

  requestPermissions(): Promise<PermissionState> {
    return this.settle(async () => (await this.native.requestPermissions()) as PermissionState);
  }

  listInputDevices(): Promise<InputDevice[]> {
    return this.settle(async () => {
      const devices = await this.native.listInputDevices();
      return devices.map((device) => ({ id: device.id, label: device.label ?? undefined }));
    });
  }

  isEncoderSupported(encoder: Encoder): Promise<boolean> {
    return this.settle(() => this.native.isEncoderSupported(encoder));
  }

  start(config: RecordConfig, sink: ChunkSink): Promise<void> {
    return this.settle(async () => {
      this.endChunks();
      // An absent output is the default stream, as `RecordConfig` documents.
      if ((config.output?.kind ?? "stream") === "stream") {
        this.stopChunks = this.native.onChunk((chunk) =>
          sink({ data: Int16Array.from(chunk.data), sampleRate: chunk.sampleRate, channels: chunk.channels }),
        );
      }
      try {
        await this.native.start(nativeConfig(config));
      } catch (error) {
        this.endChunks();
        throw error;
      }
    });
  }

  pause(): Promise<void> {
    // The native pause and resume answer synchronously; a refusal throws.
    return this.settle(async () => this.native.pause());
  }

  resume(): Promise<void> {
    return this.settle(async () => this.native.resume());
  }

  amplitude(): Promise<Amplitude> {
    return this.settle(() => this.native.amplitude());
  }

  stop(): Promise<StoppedRecording> {
    return this.settle(async () => {
      const stopped = await this.native.stop();
      this.endChunks();
      return { path: stopped.path ?? null, durationMs: stopped.durationMs ?? null };
    });
  }

  cancel(): Promise<void> {
    return this.settle(async () => {
      try {
        await this.native.cancel();
      } finally {
        this.endChunks();
      }
    });
  }

  dispose(): Promise<void> {
    // The native class names it `release`: nsh-core reserves `dispose` for freeing the value.
    return this.settle(async () => {
      try {
        await this.native.release();
      } finally {
        this.endChunks();
      }
    });
  }

  private endChunks(): void {
    const stop = this.stopChunks;
    this.stopChunks = null;
    stop?.();
  }

  private async settle<T>(operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      throw asRecorderError(error);
    }
  }
}
