export type Draft = { text: string; edited: boolean };

export function editorText(
  draft: Draft | null,
  committed: string,
  partial: string,
) {
  return draft?.edited ? draft.text : committed + partial;
}

export function editDraft(text: string): Draft {
  return { text, edited: true };
}
