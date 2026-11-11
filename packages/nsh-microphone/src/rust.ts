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
  if (config.output.kind === "file") native.path = config.output.path;
  return native;
}

class RustBackend implements RecorderBackend {
  private readonly native = new NativeRecorder();

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

  start(config: RecordConfig, _sink: ChunkSink): Promise<void> {
    return this.settle(async () => {
      // Stream output over the Rust bridge is not wired in NSH 0.4.0; only a file crosses it.
      if (config.output.kind === "stream") throw recorderError("ERR_MICROPHONE_INVALID_ARGUMENT");
      await this.native.start(nativeConfig(config));
    });
  }

  pause(): Promise<void> {
    return this.settle(() => this.native.pause());
  }

  resume(): Promise<void> {
    return this.settle(() => this.native.resume());
  }

  amplitude(): Promise<Amplitude> {
    return this.settle(() => this.native.amplitude());
  }

  stop(): Promise<StoppedRecording> {
    return this.settle(() => this.native.stop());
  }

  cancel(): Promise<void> {
    return this.settle(() => this.native.cancel());
  }

  dispose(): Promise<void> {
    // The native class names it `release`: nsh-core reserves `dispose` for freeing the value.
    return this.settle(() => this.native.release());
  }

  private async settle<T>(operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      throw asRecorderError(error);
    }
  }
}
