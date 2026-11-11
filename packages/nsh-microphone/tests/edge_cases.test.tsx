import { expect, test } from "nsh/test";
import {
  Recorder,
  memoryBackend,
  recorderError,
  type AudioChunk,
  type Encoder,
  type RecorderBackend,
} from "../src";

test("zero sample rate rejects ERR_MICROPHONE_INVALID_ARGUMENT", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await expect(recorder.start({ sampleRate: 0 })).rejects.toMatchObject({
    code: "ERR_MICROPHONE_INVALID_ARGUMENT",
  });
  expect(backend.starts()).toBe(0);
});

test("zero or three channels reject ERR_MICROPHONE_INVALID_ARGUMENT", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  for (const channels of [0, 3]) {
    await expect(recorder.start({ channels })).rejects.toMatchObject({
      code: "ERR_MICROPHONE_INVALID_ARGUMENT",
    });
  }
  await recorder.start({ channels: 2 });
  expect(backend.lastConfig()?.channels).toBe(2);
  await recorder.stop();
  expect(backend.starts()).toBe(1);
});

test("an empty file path rejects ERR_MICROPHONE_INVALID_ARGUMENT", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  for (const path of ["", "   "]) {
    await expect(recorder.start({ output: { kind: "file", path } })).rejects.toMatchObject({
      code: "ERR_MICROPHONE_INVALID_ARGUMENT",
    });
  }
  expect(backend.starts()).toBe(0);
});

test("zero bit rate on a compressed encoder rejects ERR_MICROPHONE_INVALID_ARGUMENT", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await expect(recorder.start({ bitRate: 0 })).rejects.toMatchObject({
    code: "ERR_MICROPHONE_INVALID_ARGUMENT",
  });
  await expect(recorder.start({ bitRate: 0, encoder: "opus" })).rejects.toMatchObject({
    code: "ERR_MICROPHONE_INVALID_ARGUMENT",
  });
  // PCM encoders carry their rate in the samples, so a zero bit rate is allowed on them.
  for (const encoder of ["wav", "pcm16bits"] as const) {
    await recorder.start({ bitRate: 0, encoder });
    expect(backend.lastConfig()?.encoder).toBe(encoder);
    await recorder.stop();
  }
  expect(backend.starts()).toBe(2);
});

test("an unsupported encoder rejects ERR_MICROPHONE_FORMAT_NOT_SUPPORTED", async () => {
  const backend = memoryBackend({ encoderSupported: false });
  const recorder = new Recorder({ backend });
  await expect(recorder.start({ encoder: "aacLc" })).rejects.toMatchObject({
    code: "ERR_MICROPHONE_FORMAT_NOT_SUPPORTED",
  });
  expect(backend.starts()).toBe(0);
  expect(recorder.state()).toBe("idle");
});

test("undetermined permission is requested once, then start proceeds", async () => {
  const memory = memoryBackend({ permission: "undetermined" });
  let requests = 0;
  // The memory backend has no ask counter, so the wrapper counts the requests itself.
  const backend: RecorderBackend = {
    ...memory,
    requestPermissions: async () => {
      requests += 1;
      return "granted";
    },
  };
  const recorder = new Recorder({ backend });
  await recorder.start({});
  expect(requests).toBe(1);
  expect(memory.starts()).toBe(1);
});

test("denied permission rejects ERR_MICROPHONE_PERMISSION_DENIED", async () => {
  const backend = memoryBackend({ permission: "denied" });
  const recorder = new Recorder({ backend });
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_PERMISSION_DENIED" });
  expect(backend.starts()).toBe(0);
});

test("a cancelled permission prompt rejects ERR_MICROPHONE_CANCELLED", async () => {
  const memory = memoryBackend();
  const backend: RecorderBackend = {
    ...memory,
    checkPermissions: async () => {
      throw recorderError("ERR_MICROPHONE_CANCELLED");
    },
  };
  const recorder = new Recorder({ backend });
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_CANCELLED" });
  expect(memory.starts()).toBe(0);
});

test("the config reaches the backend", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({
    sampleRate: 48000,
    channels: 2,
    bitRate: 96000,
    encoder: "opus",
    output: { kind: "file", path: "note.m4a" },
  });
  expect(backend.lastConfig()).toEqual({
    sampleRate: 48000,
    channels: 2,
    bitRate: 96000,
    encoder: "opus",
    output: { kind: "file", path: "note.m4a" },
  });
  await recorder.stop();
  await recorder.start({});
  expect(backend.lastConfig()).toEqual({
    sampleRate: 44100,
    channels: 1,
    bitRate: 128000,
    encoder: "aacLc",
    output: { kind: "stream" },
  });
});

test("chunks pause and resume with the recording", async () => {
  const backend = memoryBackend();
  const received: AudioChunk[] = [];
  const recorder = new Recorder({ backend });
  await recorder.start({ sampleRate: 48000 }, (chunk) => {
    received.push(chunk);
  });
  const chunk: AudioChunk = { data: new Int16Array([120, -120]), sampleRate: 48000, channels: 1 };
  expect(backend.pushChunk(chunk)).toBe(true);
  await recorder.pause();
  expect(backend.pushChunk(chunk)).toBe(false);
  await recorder.resume();
  expect(backend.pushChunk(chunk)).toBe(true);
  await recorder.stop();
  expect(backend.pushChunk(chunk)).toBe(false);
  expect(backend.chunks()).toBe(2);
  expect(received).toHaveLength(2);
  expect(received[0].data).toEqual(new Int16Array([120, -120]));
});

test("a late stop answer after cancel is ignored", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  await recorder.start({});
  backend.holdStop();
  const stopping = recorder.stop();
  await recorder.cancel();
  await expect(stopping).rejects.toMatchObject({ code: "ERR_MICROPHONE_CANCELLED" });
  // The cancel already ended the session, so the held answer has nothing left to settle.
  expect(backend.finishStop({ path: "late.m4a", durationMs: 99 })).toBe(false);
  expect(recorder.state()).toBe("idle");
  await recorder.start({});
  expect(backend.starts()).toBe(2);
  await recorder.stop();
});

test("a backend error keeps its code", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  backend.failNextStart("ERR_MICROPHONE_NOT_AVAILABLE");
  await expect(recorder.start({})).rejects.toMatchObject({ code: "ERR_MICROPHONE_NOT_AVAILABLE" });
  expect(recorder.state()).toBe("idle");
  await recorder.start({});
  backend.holdStop();
  const stopping = recorder.stop();
  expect(backend.finishStop("ERR_MICROPHONE_BACKEND")).toBe(true);
  await expect(stopping).rejects.toMatchObject({ code: "ERR_MICROPHONE_BACKEND" });
  // A failed stop leaves the recording running, so a cancel ends it.
  expect(recorder.isRecording()).toBe(true);
  await recorder.cancel();
  // starts() counts the starts that took; the one that failed with its code never counted.
  expect(backend.starts()).toBe(1);
});

test("each encoder has the name the Rust crate uses", async () => {
  const backend = memoryBackend();
  const recorder = new Recorder({ backend });
  const encoders: Encoder[] = [
    "aacLc",
    "aacEld",
    "aacHe",
    "amrNb",
    "amrWb",
    "flac",
    "opus",
    "pcm16bits",
    "vorbis",
    "wav",
  ];
  for (const encoder of encoders) {
    expect(await recorder.isEncoderSupported(encoder)).toBe(true);
    await recorder.start({ encoder });
    expect(backend.lastConfig()?.encoder).toBe(encoder);
    await recorder.stop();
  }
  expect(backend.starts()).toBe(encoders.length);
});
