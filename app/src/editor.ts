export interface Draft {
  text: string;
  edited: boolean;
}

export const editorText = (draft: Draft | null, committed: string, partial: string) =>
  draft?.edited === true ? draft.text : committed + partial;

export const editDraft = (text: string): Draft => ({ text, edited: true });
