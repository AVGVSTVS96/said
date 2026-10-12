import { expect, test } from "bun:test";
import { createCoreSource, type AppAction } from "./source";
import { createFakeSource } from "./fake";

test("core actions stay ordered across delayed edits and insertion", async () => {
  const calls: AppAction[] = [];
  let finish: (() => void) | undefined;
  const source = createCoreSource(async (action) => {
    calls.push(action);
    if (calls.length === 1)
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
  });
  const first = source.dispatch({ type: "edit", id: "one", text: "first" });
  const second = source.dispatch({ type: "edit", id: "one", text: "second\n" });
  const insert = source.dispatch({ type: "insert", id: "one" });
  await Promise.resolve();
  await Promise.resolve();
  expect(calls).toEqual([{ type: "edit", id: "one", text: "first" }]);
  finish!();
  await Promise.all([first, second, insert]);
  expect(calls).toEqual([
    { type: "edit", id: "one", text: "first" },
    { type: "edit", id: "one", text: "second\n" },
    { type: "insert", id: "one" },
  ]);
});

test("a rejected action does not block later retries", async () => {
  let calls = 0;
  const source = createCoreSource(async () => {
    if (++calls === 1) throw new Error("save failed");
  });
  await expect(source.dispatch({ type: "insert_last" })).rejects.toThrow(
    "save failed",
  );
  await source.dispatch({ type: "insert_last" });
  expect(calls).toBe(2);
});

test("the fake saves original and edited text separately without desktop side effects", async () => {
  const fake = createFakeSource();
  const id = fake.getSnapshot().sessions[0].id;
  await fake.dispatch({ type: "edit", id, text: "  manual edit\n\n" });
  for (let step = 0; step < 6; step++) fake.next();
  const row = fake.getSnapshot().history.find((row) => row.id === id)!;
  expect(row.raw_text).toBe(
    "um, no, keep the the old value, actually no, use cache_key, no no don't rename it.",
  );
  expect(row.text).toBe(
    "  manual edit\n\nuse cache_key, no no don't rename it.",
  );
  await fake.dispatch({ type: "copy", id });
  await fake.dispatch({ type: "insert", id });
  expect(fake.actions.at(-1)).toEqual({ type: "insert", id });
  expect(
    fake.getSnapshot().history.find((row) => row.id === id)!.pasted_at,
  ).toBeNull();
  expect(fake.getSnapshot().sessions[0].phase).toBe("reviewing");
});

test("snapshot listeners unsubscribe and the idle snapshot is stable", () => {
  const fake = createFakeSource();
  expect(fake.getSnapshot()).toBe(fake.getSnapshot());
  let updates = 0;
  const unsubscribe = fake.subscribe(() => {
    updates++;
  });
  fake.next();
  unsubscribe();
  fake.next();
  expect(updates).toBe(1);
});

test("host termination invalidates live state and rejects queued and future actions", async () => {
  const calls: AppAction[] = [];
  let release: (() => void) | undefined;
  const source = createCoreSource(async (action) => {
    calls.push(action);
    await new Promise<void>((resolve) => { release = resolve; });
  });
  let changes = 0;
  source.subscribe(() => { changes++; });
  source.receive({ sessions: [], history: [], settings: null, devices: [], sequence: 1 });
  const first = source.dispatch({ type: "insert_last" });
  const queued = source.dispatch({ type: "insert_last" });
  await Promise.resolve();
  await Promise.resolve();
  const before = source.getSnapshot();
  source.disconnect("capture actor stopped");
  expect(source.connected).toBe(false);
  expect(source.getSnapshot()).not.toBe(before);
  expect(source.getSnapshot().connectionError).toBe("capture actor stopped");
  expect(source.getSnapshot().sessions).toEqual([]);
  expect(changes).toBe(2);
  release!();
  await first;
  await expect(queued).rejects.toThrow("capture actor stopped");
  await expect(source.dispatch({ type: "insert_last" })).rejects.toThrow("capture actor stopped");
  source.receive({ ...before, sequence: 2 });
  expect(source.getSnapshot().connectionError).toBe("capture actor stopped");
  expect(calls).toHaveLength(1);
});

test("snapshots without history keep the rows from the last one that carried them", () => {
  const fake = createFakeSource().getSnapshot();
  const source = createCoreSource(async () => {});
  source.receive({ ...fake, sequence: 1 });
  source.receive({ ...fake, history: null, sessions: [], sequence: 2 });
  expect(source.getSnapshot().history).toBe(fake.history);
  expect(source.getSnapshot().sessions).toEqual([]);
});
