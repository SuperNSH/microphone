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

test("onStateChanged hears each change once, in order, and not after it stops", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  const heard: string[] = [];
  const stopListening = recorder.onStateChanged((state) => heard.push(state));
  await recorder.start({});
  await recorder.pause();
  // A refused pause changes nothing, so it announces nothing.
  await expect(recorder.pause()).rejects.toMatchObject({ code: "ERR_MICROPHONE_INVALID_STATE" });
  await recorder.resume();
  await recorder.stop();
  expect(heard).toEqual(["recording", "paused", "recording", "idle"]);

  stopListening();
  await recorder.start({});
  expect(heard).toHaveLength(4);
  await recorder.cancel();
});

test("onStateChanged announces a failed start's claim and release, and a cancel", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  const heard: string[] = [];
  recorder.onStateChanged((state) => heard.push(state));
  backend.failNextStart("ERR_MICROPHONE_BACKEND");
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_BACKEND" });
  expect(heard).toEqual(["recording", "idle"]);
  await recorder.start({});
  await recorder.cancel();
  expect(heard).toEqual(["recording", "idle", "recording", "idle"]);
});

test("a state listener that throws does not reject the call or silence the others", async () => {
  const recorder = new Recorder({ backend: memoryBackend() });
  const heard: string[] = [];
  recorder.onStateChanged(() => {
    throw new Error("listener failure");
  });
  recorder.onStateChanged((state) => heard.push(state));
  await recorder.start({});
  expect(recorder.isRecording()).toBe(true);
  expect(heard).toEqual(["recording"]);
  await recorder.stop();
});

test("a state listener may read the recorder back, and one function listens twice independently", async () => {
  const recorder = new Recorder({ backend: memoryBackend() });
  const seen: string[] = [];
  const listener = () => seen.push(recorder.state());
  const first = recorder.onStateChanged(listener);
  recorder.onStateChanged(listener);
  await recorder.start({});
  expect(seen).toEqual(["recording", "recording"]);
  first();
  await recorder.stop();
  expect(seen).toEqual(["recording", "recording", "idle"]);
});
