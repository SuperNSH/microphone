import { expect, test } from "nsh/test";
import { asRecorderError, recorderError, type RecorderErrorCode } from "../src";

/** The wording apps see. The same strings as the Rust crate's ErrorKind messages. */
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

test("every code carries its message", () => {
  for (const code of Object.keys(MESSAGES) as RecorderErrorCode[]) {
    const error = recorderError(code);
    expect(error.code).toBe(code);
    expect(error.message).toBe(MESSAGES[code]);
    expect(error.name).toBe("RecorderError");
  }
});

test("asRecorderError maps a host rejection's code prefix", () => {
  expect(asRecorderError(recorderError("ERR_MICROPHONE_IN_PROGRESS")).code).toBe("ERR_MICROPHONE_IN_PROGRESS");
  expect(asRecorderError({ code: "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED", message: "x" }).code).toBe(
    "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED",
  );
  expect(asRecorderError({ code: "ERR_NSH_HOST_ERROR", message: "ERR_MICROPHONE_CANCELLED: text" }).code).toBe(
    "ERR_MICROPHONE_CANCELLED",
  );
  expect(asRecorderError({ code: "ERR_NSH_HOST_ERROR", message: "ERR_MICROPHONE_NO_SUCH_CODE: text" }).code).toBe(
    "ERR_MICROPHONE_BACKEND",
  );
  expect(asRecorderError(new Error("boom")).code).toBe("ERR_MICROPHONE_BACKEND");
});
