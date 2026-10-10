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

test("the adapter reads the incoming core-owned edited field, including empty text", () => {
  const session = createFakeSource().getSnapshot().sessions[0];
  const edited = { ...session, edited: "user-owned" };
  const empty = { ...session, edited: "" };
  expect(userText(edited)).toBe("user-owned");
  expect(userText(empty)).toBe("");
  expect(userText(session)).toBeNull();
});
