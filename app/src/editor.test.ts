import { expect, test } from "bun:test";
import { editDraft, editorText } from "./editor";
import { createFakeSource } from "./fake";
import { userText } from "./source";

test("recognition replaces only its provisional tail and preserves literal bytes", () => {
  expect(editorText(null, "  no no\n", "é cache_key  ")).toBe(
    "  no no\né cache_key  ",
  );
  expect(editorText(null, "  no no\n", "actually no")).toBe(
    "  no no\nactually no",
  );
});

test("a user's draft survives late partials, commits and an empty edit", () => {
  const fake = createFakeSource();
  const draft = editDraft("my edit\n  stays here\n");
  for (let step = 0; step < 6; step++) {
    fake.next();
    const session = fake.getSnapshot().sessions[0];
    expect(editorText(draft, session.committed, session.partial)).toBe(
      draft.text,
    );
    expect(editorText(editDraft(""), session.committed, session.partial)).toBe(
      "",
    );
  }
});

test("the adapter reads core-owned edited text, including empty text", () => {
  const session = createFakeSource().getSnapshot().sessions[0];
  const edited = { ...session, edited: "user-owned" };
  const empty = { ...session, edited: "" };
  expect(userText(edited)).toBe("user-owned");
  expect(userText(empty)).toBe("");
  expect(userText(session)).toBeNull();
});

test("core-owned editor snapshots preserve the edit while later words stream", async () => {
  const fake = createFakeSource();
  const id = fake.getSnapshot().sessions[0].id;
  await fake.dispatch({ type: "edit", id, text: "  my edit stays  \n" });
  for (let step = 0; step < 6; step++) fake.next();
  expect(userText(fake.getSnapshot().sessions[0])).toBe(
    "  my edit stays  \nuse cache_key, no no don't rename it.",
  );
});

test("editing committed words leaves the unedited provisional tail live", async () => {
  const fake = createFakeSource();
  const id = fake.getSnapshot().sessions[0].id;
  await fake.dispatch({
    type: "edit",
    id,
    text: "edited prefix the the old value",
  });
  fake.next();
  fake.next();
  expect(userText(fake.getSnapshot().sessions[0])).toBe(
    "edited prefix the the old value, actually",
  );
});
