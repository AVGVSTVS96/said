import { expect, test } from "bun:test";
import { createCoreSource, type UiAction } from "./source";
import { createFakeSource } from "./fake";

test("core actions stay ordered across delayed edits and insertion", async () => {
  const calls: UiAction[] = [];
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
  expect(row.text).toBe("  manual edit\n\n");
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
