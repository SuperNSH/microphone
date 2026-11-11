import { expect, open, test } from "nsh/test";

test("availability checks at start and refreshes on demand", async () => {
  const app = await open("./fixtures/availability.tsx", { clock: {} });
  await app.waitFor("text:availability granted device true runs 2");
  expect(app.texts()).toContain("reads 1");
  await app.press("id:refresh");
  await app.waitFor("text:reads 2");
  // The answer did not change, so the binding that shows it did not run again.
  expect(app.texts()).toContain("availability granted device true runs 2");
  await app.setLifecycle("background");
  await app.setLifecycle("active");
  await app.waitFor("text:reads 3");
  expect(app.texts()).toContain("availability granted device true runs 2");
});

test("availability reports a failed check with its code", async () => {
  const app = await open("./fixtures/availability.tsx", { clock: {} });
  await app.waitFor("text:availability granted device true runs 2");
  await app.press("id:fail");
  await app.press("id:refresh");
  await app.waitFor("text:availability failed ERR_MICROPHONE_NOT_AVAILABLE runs 3");
});
