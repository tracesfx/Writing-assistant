import { create } from "zustand";

import type { SearchHit, SearchScopeKind } from "@margin/sdk";

interface SearchState {
  text: string;
  replacement: string;
  regex: boolean;
  caseSensitive: boolean;
  wholeWord: boolean;
  scope: SearchScopeKind;
  scopeId: string | null;
  showReplace: boolean;
  hits: SearchHit[];
  truncated: boolean;
  /** Keys (`docId:block:start`) of hits left out of a replace. */
  excluded: Record<string, true>;
  error: string | null;
  busy: boolean;
  /** Bumped to focus the find field. */
  focusAt: number;
}

export const useSearch = create<SearchState>(() => ({
  text: "",
  replacement: "",
  regex: false,
  caseSensitive: false,
  wholeWord: false,
  scope: "manuscript",
  scopeId: null,
  showReplace: false,
  hits: [],
  truncated: false,
  excluded: {},
  error: null,
  busy: false,
  focusAt: 0,
}));

export const hitKey = (h: { docId: string; block: number; start: number }) => `${h.docId}:${h.block}:${h.start}`;
