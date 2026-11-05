/** Editor module state: live status of each open editor, keyed by tab id. */
import type { Editor } from "@tiptap/core";
import { create } from "zustand";

import type { SaveState } from "../../editor/autosave";
import type { DocHub } from "./hub";

export interface EditorStatus {
  docId: string;
  save: SaveState;
  words: number;
  selectionWords: number;
}

interface EditorStore {
  byTab: Record<string, EditorStatus>;
  update(tabId: string, patch: Partial<EditorStatus> & { docId: string }): void;
  remove(tabId: string): void;
}

export const useEditorStore = create<EditorStore>((set) => ({
  byTab: {},
  update(tabId, patch) {
    set((s) => {
      const prev = s.byTab[tabId] ?? { docId: patch.docId, save: { status: "saved" }, words: 0, selectionWords: 0 };
      return { byTab: { ...s.byTab, [tabId]: { ...prev, ...patch } } };
    });
  },
  remove(tabId) {
    set((s) => {
      const next = { ...s.byTab };
      delete next[tabId];
      return { byTab: next };
    });
  },
}));

/** Live editor instances, for commands. Not reactive. */
export interface EditorHandle {
  docId: string;
  editor: Editor;
  hub: DocHub;
}
export const editors = new Map<string, EditorHandle>();

/** Words the author chose to ignore while spellchecking this session. */
export const ignoredWords = new Set<string>();
