/** The codes a recorder rejection carries. Apps match on `code`. */
export type RecorderErrorCode =
  | "ERR_MICROPHONE_NOT_AVAILABLE"
  | "ERR_MICROPHONE_PERMISSION_DENIED"
  | "ERR_MICROPHONE_CANCELLED"
  | "ERR_MICROPHONE_IN_PROGRESS"
  | "ERR_MICROPHONE_INVALID_STATE"
  | "ERR_MICROPHONE_INVALID_ARGUMENT"
  | "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED"
  | "ERR_MICROPHONE_BACKEND";

/** Same wording as the Rust crate's messages. None holds the reason text. */
const MESSAGES: Record<RecorderErrorCode, string> = {
  ERR_MICROPHONE_NOT_AVAILABLE: "Microphone recording is not available on this device.",
  ERR_MICROPHONE_PERMISSION_DENIED: "Recording was refused. Allow microphone access in the system settings.",
  ERR_MICROPHONE_CANCELLED: "The operation was cancelled.",
  ERR_MICROPHONE_IN_PROGRESS: "A recording is already running. Stop or cancel it first.",
  ERR_MICROPHONE_INVALID_STATE: "The recorder is in a state that does not allow this call.",
  ERR_MICROPHONE_INVALID_ARGUMENT: "An argument is empty or not allowed on this platform.",
  ERR_MICROPHONE_FORMAT_NOT_SUPPORTED: "The encoder or format is not supported on this platform.",
  ERR_MICROPHONE_BACKEND: "The platform's audio service failed.",
};

const KNOWN = new Set<string>(Object.keys(MESSAGES));

/** A rejection of the microphone API. `code` is the stable name; the message says what to do. */
export class RecorderError extends Error {
  readonly code: RecorderErrorCode;

  constructor(code: RecorderErrorCode) {
    super(MESSAGES[code]);
    this.name = "RecorderError";
    this.code = code;
  }
}

/** The error for a known code. */
export function recorderError(code: RecorderErrorCode): RecorderError {
  return new RecorderError(code);
}

/**
 * Turns whatever the native side or a host threw into a coded `RecorderError`. A host's
 * rejection carries `ERR_NSH_HOST_ERROR`, and its message starts with the code, as
 * `ERR_MICROPHONE_CANCELLED: text`. Anything unknown is a backend failure.
 */
export function asRecorderError(error: unknown): RecorderError {
  if (error instanceof RecorderError) return error;
  const raw = (error ?? {}) as { code?: unknown; message?: unknown };
  const code = typeof raw.code === "string" ? raw.code : "";
  const message = typeof raw.message === "string" ? raw.message : "";
  if (code === "ERR_NSH_HOST_ERROR") {
    const named = /^(ERR_MICROPHONE_[A-Z_]+): /.exec(message)?.[1];
    return named !== undefined && KNOWN.has(named)
      ? recorderError(named as RecorderErrorCode)
      : recorderError("ERR_MICROPHONE_BACKEND");
  }
  return KNOWN.has(code) ? recorderError(code as RecorderErrorCode) : recorderError("ERR_MICROPHONE_BACKEND");
}
