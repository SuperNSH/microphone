import { open, test } from "nsh/test";

test("the entry records on its memory backend and a stop resolves once with the file", async () => {
  const app = await open("../App.tsx", { clock: {} });
  await app.waitFor("text:idle");
  await app.press("id:record");
  await app.waitFor("text:state recording");
  await app.press("id:stop");
  await app.waitFor("text:stopped recording.m4a");
  // A second stop finds the recorder idle and answers with its code.
  await app.press("id:stop");
  await app.waitFor("text:ERR_MICROPHONE_INVALID_STATE");
});
