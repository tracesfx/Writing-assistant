/**
 * Remembered places, persisted per project through `uiState`
 * (SPEC §11.4): the cursor and scroll position of each document, the last
 * edited place ("Where was I?"), bookmarks, and back/forward navigation.
 */
import { uiState, useWorkbench } from "@margin/sdk";

export interface DocPlace {
  pos: number;
  scroll: number;
}

export interface LastEdited {
  docId: string;
  pos: number;
  at: string;
}

const PLACES = "editor.places";
const LAST = "editor.lastEdited";
const MARKS = "editor.bookmarks";
const MAX_PLACES = 300;

export function placeOf(docId: string): DocPlace | undefined {
  return uiState.get<Record<string, DocPlace>>(PLACES)?.[docId];
}

let pendingPlaces: Record<string, DocPlace> = {};
let placeTimer: ReturnType<typeof setTimeout> | undefined;

export function rememberPlace(docId: string, place: DocPlace): void {
  pendingPlaces[docId] = place;
  clearTimeout(placeTimer);
  placeTimer = setTimeout(() => {
    const all = { ...(uiState.get<Record<string, DocPlace>>(PLACES) ?? {}), ...pendingPlaces };
    pendingPlaces = {};
    const keys = Object.keys(all);
    for (const k of keys.slice(0, Math.max(0, keys.length - MAX_PLACES))) delete all[k];
    uiState.set(PLACES, all);
  }, 1000);
}

export function lastEdited(): LastEdited | undefined {
  return uiState.get<LastEdited>(LAST);
}

let lastTimer: ReturnType<typeof setTimeout> | undefined;
export function rememberEdit(docId: string, pos: number): void {
  clearTimeout(lastTimer);
  lastTimer = setTimeout(() => uiState.set(LAST, { docId, pos, at: new Date().toISOString() }), 1000);
}

export function bookmarksOf(docId: string): number[] {
  return uiState.get<Record<string, number[]>>(MARKS)?.[docId] ?? [];
}

export function saveBookmarks(docId: string, positions: number[]): void {
  const all = { ...(uiState.get<Record<string, number[]>>(MARKS) ?? {}) };
  if (positions.length) all[docId] = positions;
  else delete all[docId];
  uiState.set(MARKS, all);
}

// --- Navigation history (Alt+← / Alt+→) -------------------------------------

interface NavEntry {
  docId: string;
  pos: number;
}

const back: NavEntry[] = [];
const forward: NavEntry[] = [];
let navigating = false;

/** Record leaving a document (called when the active document changes). */
export function recordNavigation(from: NavEntry | undefined): void {
  if (navigating || !from) return;
  if (back.at(-1)?.docId === from.docId) back.pop();
  back.push(from);
  if (back.length > 100) back.shift();
  forward.length = 0;
}

export function navigate(dir: "back" | "forward", current: NavEntry | undefined, open: (e: NavEntry) => void): boolean {
  const from = dir === "back" ? back : forward;
  const to = dir === "back" ? forward : back;
  const target = from.pop();
  if (!target) return false;
  if (current) to.push(current);
  navigating = true;
  try {
    open(target);
  } finally {
    setTimeout(() => {
      navigating = false;
    }, 0);
  }
  return true;
}

/** Track the active document across panes to feed the history. */
export function wireNavigation(getPos: (docId: string) => number | undefined): () => void {
  let prev: string | undefined;
  return useWorkbench.subscribe((s) => {
    const pane = s.panes.find((p) => p.id === s.focusedPane);
    const doc = pane?.tabs.find((t) => t.id === pane.activeTabId)?.resourceId;
    if (doc === prev) return;
    if (prev) recordNavigation({ docId: prev, pos: getPos(prev) ?? placeOf(prev)?.pos ?? 1 });
    prev = doc;
  });
}
