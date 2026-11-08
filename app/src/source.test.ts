import { expect, test } from "bun:test";

import { createFakeSource } from "./fake";
import { createCoreSource } from "./source";
import type { AppAction } from "./source";

test("core actions stay ordered across delayed edits and insertion", async () => {
  const calls: AppAction[] = [];
  const first = Promise.withResolvers<null>();

  const source = createCoreSource(async (action) => {
    calls.push(action);

    if (calls.length === 1) {
      await first.promise;
    }
  });

  const dispatched = [
    source.dispatch({ type: "edit", id: "one", text: "first" }),
    source.dispatch({ type: "edit", id: "one", text: "second\n" }),
    source.dispatch({ type: "insert", id: "one" }),
  ];

  await Promise.resolve();
  await Promise.resolve();
  expect(calls).toEqual([{ type: "edit", id: "one", text: "first" }]);
  first.resolve(null);
  await Promise.all(dispatched);
  expect(calls).toEqual([
    { type: "edit", id: "one", text: "first" },
    { type: "edit", id: "one", text: "second\n" },
    { type: "insert", id: "one" },
  ]);
});

test("a rejected action does not block later retries", async () => {
  let calls = 0;

  const source = createCoreSource(() => {
    calls += 1;

    if (calls === 1) {
      throw new Error("save failed");
    }
  });

  expect(await source.dispatch({ type: "insert_last" }).catch(String)).toBe("Error: save failed");
  await source.dispatch({ type: "insert_last" });
  expect(calls).toBe(2);
});

test("the fake saves original and edited text separately without desktop side effects", async () => {
  const fake = createFakeSource();
  const { id } = fake.session;
  await fake.dispatch({ type: "edit", id, text: "  manual edit\n\n" });

  for (let step = 0; step < 6; step += 1) {
    fake.next();
  }

  const saved = fake.getSnapshot().history.find((row) => row.id === id);
  expect(saved?.raw_text).toBe(
    "um, no, keep the the old value, actually no, use cache_key, no no don't rename it.",
  );
  expect(saved?.text).toBe("  manual edit\n\nuse cache_key, no no don't rename it.");
  await fake.dispatch({ type: "copy", id });
  await fake.dispatch({ type: "insert", id });
  expect(fake.actions.at(-1)).toEqual({ type: "insert", id });
  expect(fake.getSnapshot().history.find((row) => row.id === id)?.pasted_at).toBeNull();
  expect(fake.session.phase).toBe("reviewing");
});

test("snapshot listeners unsubscribe and the idle snapshot is stable", () => {
  const fake = createFakeSource();
  expect(fake.getSnapshot()).toBe(fake.getSnapshot());
  let updates = 0;

  const unsubscribe = fake.subscribe(() => {
    updates += 1;
  });

  fake.next();
  unsubscribe();
  fake.next();
  expect(updates).toBe(1);
});

test("host termination invalidates live state and rejects queued and future actions", async () => {
  const calls: AppAction[] = [];
  const release = Promise.withResolvers<null>();

  const source = createCoreSource(async (action) => {
    calls.push(action);
    await release.promise;
  });

  let changes = 0;
  source.subscribe(() => {
    changes += 1;
  });
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
  release.resolve(null);
  await first;
  expect(await queued.catch(String)).toBe("Error: capture actor stopped");
  expect(await source.dispatch({ type: "insert_last" }).catch(String)).toBe(
    "Error: capture actor stopped",
  );
  source.receive({ ...before, sequence: 2 });
  expect(source.getSnapshot().connectionError).toBe("capture actor stopped");
  expect(calls).toHaveLength(1);
});

test("snapshots without history keep the rows from the last one that carried them", () => {
  const fake = createFakeSource().getSnapshot();
  const source = createCoreSource(() => {});
  source.receive({ ...fake, sequence: 1 });
  source.receive({ ...fake, history: null, sessions: [], sequence: 2 });
  expect(source.getSnapshot().history).toBe(fake.history);
  expect(source.getSnapshot().sessions).toEqual([]);
});
