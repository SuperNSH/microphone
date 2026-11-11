import { expect, test } from "nsh/test";
import { Recorder, memoryBackend } from "../src";

/** Lets a held stop's answer travel from the backend to the recorder's gate. */
async function delivered(): Promise<void> {
  for (let turn = 0; turn < 4; turn += 1) await Promise.resolve();
}

test("start while recording rejects with ERR_MICROPHONE_IN_PROGRESS", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_IN_PROGRESS" });
  expect(backend.starts()).toBe(1);
  expect(recorder.isRecording()).toBe(true);
  await recorder.stop();
});

test("pause and resume track the state machine", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  expect(recorder.state()).toBe("recording");
  await recorder.pause();
  expect(recorder.isPaused()).toBe(true);
  await expect(recorder.pause()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(await recorder.amplitude()).toEqual({ average: 0, peak: 0 });
  await recorder.resume();
  expect(recorder.isRecording()).toBe(true);
  expect(await recorder.stop()).toEqual({ path: "recording.m4a", durationMs: 0 });
  expect(recorder.state()).toBe("idle");
  expect(backend.pauses()).toBe(1);
  expect(backend.resumes()).toBe(1);
});

test("pause when idle rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await expect(recorder.pause()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.pauses()).toBe(0);
});

test("resume when recording rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  await expect(recorder.resume()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.resumes()).toBe(0);
  expect(recorder.isRecording()).toBe(true);
});

test("stop when idle rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await expect(recorder.stop()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.stops()).toBe(0);
});

test("stop resolves once; a second stop rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  backend.holdStop();
  const stopping = recorder.stop();
  await expect(recorder.stop()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.finishStop({ path: "held.m4a", durationMs: 7 })).toBe(true);
  await delivered();
  expect(recorder.state()).toBe("idle");
  expect(await stopping).toEqual({ path: "held.m4a", durationMs: 7 });
  // Nothing is held anymore, so the recorder's answer cannot arrive a second time.
  expect(backend.finishStop({ path: "again.m4a", durationMs: 9 })).toBe(false);
  await expect(recorder.stop()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.stops()).toBe(1);
});

test("cancel when idle rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await expect(recorder.cancel()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  expect(backend.cancels()).toBe(0);
});

test("amplitude when idle rejects ERR_MICROPHONE_INVALID_STATE", async () => {
  const recorder = new Recorder({ backend: memoryBackend() });
  await expect(recorder.amplitude()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
});

test("clones share one gate", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  // The TS twin of a Rust clone is another binding to the same handle: one gate behind both.
  const twin = recorder;
  await twin.start({});
  expect(recorder.isRecording()).toBe(true);
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_IN_PROGRESS" });
  expect(await twin.stop()).toEqual({ path: "recording.m4a", durationMs: 0 });
  expect(recorder.state()).toBe("idle");
  expect(backend.starts()).toBe(1);
});

test("dispose while recording cancels and later calls reject ERR_MICROPHONE_NOT_AVAILABLE", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  await recorder.dispose();
  expect(backend.cancels()).toBe(1);
  expect(backend.disposals()).toBe(1);
  await expect(recorder.checkPermissions()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.pause()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.amplitude()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.stop()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.cancel()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  await expect(recorder.dispose()).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  expect(backend.starts()).toBe(1);
  expect(backend.disposals()).toBe(1);
});
