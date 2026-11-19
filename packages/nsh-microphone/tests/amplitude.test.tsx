import { expect, open, test } from "nsh/test";

type App = Awaited<ReturnType<typeof open>>;

/** How many levels the fixture's listener has been told about. */
function heard(app: App): number {
  const line = app.texts().find((text) => text.startsWith("heard "));
  return Number(line?.slice("heard ".length));
}

test("amplitude is heard every interval while a recording runs, paused or not, and not while idle", async () => {
  const app = await open("./fixtures/amplitude.tsx", { clock: {} });
  // Idle: the interval fires, finds no recording and says nothing.
  await app.clock!.runFor(500);
  expect(heard(app)).toBe(0);
  expect(app.texts()).toContain("level none");

  await app.press("id:loud");
  await app.press("id:record");
  await app.clock!.runFor(350);
  expect(heard(app)).toBeGreaterThanOrEqual(3);
  expect(app.texts()).toContain("level 0.5 0.75");

  // A paused recording still has a level.
  await app.press("id:pause");
  const atPause = heard(app);
  await app.clock!.runFor(250);
  expect(heard(app)).toBeGreaterThan(atPause);

  // A stopped recording is quiet again.
  await app.press("id:stop");
  const atStop = heard(app);
  await app.clock!.runFor(500);
  expect(heard(app)).toBe(atStop);
});

test("amplitude stops arriving once its listener is unsubscribed", async () => {
  const app = await open("./fixtures/amplitude.tsx", { clock: {} });
  await app.press("id:record");
  await app.clock!.runFor(250);
  expect(heard(app)).toBeGreaterThanOrEqual(2);
  await app.press("id:unsubscribe");
  const atUnsubscribe = heard(app);
  await app.clock!.runFor(500);
  expect(heard(app)).toBe(atUnsubscribe);
});
