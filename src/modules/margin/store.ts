/**
 * Margin panel state (SPEC §8): ideas, questions and phrasing results for
 * this session. Pinned ideas live in the project (`memory/ideas.json`).
 */
import { create } from "zustand";

import {
  editorHost,
  memory,
  uiState,
  workbench,
  type AskAnswer,
  type Idea,
  type PhrasingResult,
  type PinnedIdea,
} from "@margin/sdk";

export interface AskItem {
  id: number;
  question: string;
  answer?: AskAnswer;
  error?: string;
}

interface MarginStore {
  tab: string;
  ideas: Idea[];
  ideasBusy: boolean;
  ideasNote: string | null;
  /** Texts the author dismissed this session (the "don't suggest" list). */
  dismissed: string[];
  expanded: Record<string, { text: string; contextId: string }>;
  pinned: PinnedIdea[];
  asks: AskItem[];
  askBusy: boolean;
  phrasing: { selected: string; result: PhrasingResult | null; busy: boolean; error: string | null } | null;
  /** Fresh ideas arrived while another tab or the editor had focus. */
  fresh: boolean;
}

export const useMargin = create<MarginStore>(() => ({
  tab: "margin.ideas",
  ideas: [],
  ideasBusy: false,
  ideasNote: null,
  dismissed: [],
  expanded: {},
  pinned: [],
  asks: [],
  askBusy: false,
  phrasing: null,
  fresh: false,
}));

const set = useMargin.setState;
const get = useMargin.getState;

export function errMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Auto ideas at paragraph ends (SPEC §8.1): on by default, remembered per project. */
export const AUTO_KEY = "margin.autoIdeas";
export function autoIdeas(): boolean {
  return uiState.get<boolean>(AUTO_KEY) ?? true;
}

let lastAutoText = "";

export async function requestIdeas(auto = false): Promise<void> {
  const h = editorHost.active();
  if (!h) {
    if (!auto) set({ ideasNote: "Open a scene and put the cursor where you are writing." });
    return;
  }
  const textBefore = h.textBeforeCursor();
  if (textBefore.trim().length < 20) {
    if (!auto) set({ ideasNote: "Write a little more first: ideas come from what is on the page." });
    return;
  }
  if (get().ideasBusy) return;
  if (auto) lastAutoText = textBefore;
  set({ ideasBusy: true, ideasNote: null });
  try {
    const res = await memory.margin.ideas({ docId: h.docId, textBefore, avoid: get().dismissed, auto });
    set((s) => ({
      ideas: res.ideas.length > 0 ? res.ideas : s.ideas,
      ideasNote: res.note,
      // A dot only if the author is not looking at the Ideas tab right now.
      fresh: res.ideas.length > 0 && auto,
    }));
  } catch (e) {
    set({ ideasNote: auto ? null : errMessage(e) });
  } finally {
    set({ ideasBusy: false });
  }
}

/** True when the last auto run was for different text than is on the page now. */
export function autoDue(textBefore: string): boolean {
  return textBefore !== lastAutoText && textBefore.trim().split(/\s+/).length >= 20;
}

export function dismissIdea(idea: Idea): void {
  set((s) => ({ ideas: s.ideas.filter((i) => i.id !== idea.id), dismissed: [...s.dismissed, idea.text].slice(-30) }));
}

export async function expandIdea(idea: Idea): Promise<void> {
  const h = editorHost.active();
  try {
    const out = await memory.margin.expand(h?.docId ?? idea.refs[0]?.doc ?? "", idea.text, h?.textBeforeCursor() ?? "");
    set((s) => ({ expanded: { ...s.expanded, [idea.id]: out } }));
  } catch (e) {
    workbench.notify({ message: `Could not expand the idea: ${errMessage(e)}` });
  }
}

export async function loadPinned(): Promise<void> {
  try {
    set({ pinned: await memory.margin.pinned() });
  } catch {
    set({ pinned: [] });
  }
}

export async function pinIdea(idea: Idea): Promise<void> {
  const doc = editorHost.active()?.docId ?? idea.refs[0]?.doc ?? "";
  try {
    set({ pinned: await memory.margin.pin(idea, doc) });
    workbench.notify({ message: "Pinned in your project notes.", key: "margin.pin" });
  } catch (e) {
    workbench.notify({ message: `Could not pin: ${errMessage(e)}` });
  }
}

export async function unpinIdea(id: string): Promise<void> {
  try {
    set({ pinned: await memory.margin.unpin(id) });
  } catch (e) {
    workbench.notify({ message: `Could not unpin: ${errMessage(e)}` });
  }
}

export async function ideaToNote(idea: Idea): Promise<void> {
  const doc = editorHost.active()?.docId ?? idea.refs[0]?.doc;
  if (!doc) return;
  try {
    const note = await memory.margin.toNote(doc, idea.text);
    workbench.notify({ message: "Added to this scene's notes.", action: { label: "Open", run: () => workbench.openDocument(note) }, key: "margin.note" });
  } catch (e) {
    workbench.notify({ message: `Could not add the note: ${errMessage(e)}` });
  }
}

let askId = 1;
export async function askBook(question: string): Promise<void> {
  const q = question.trim();
  if (!q || get().askBusy) return;
  const id = askId++;
  set((s) => ({ askBusy: true, asks: [{ id, question: q }, ...s.asks].slice(0, 20) }));
  try {
    const answer = await memory.margin.ask(q);
    set((s) => ({ asks: s.asks.map((a) => (a.id === id ? { ...a, answer } : a)) }));
  } catch (e) {
    set((s) => ({ asks: s.asks.map((a) => (a.id === id ? { ...a, error: errMessage(e) } : a)) }));
  } finally {
    set({ askBusy: false });
  }
}

export async function requestPhrasing(): Promise<void> {
  const h = editorHost.active();
  const ctx = h?.selectionContext();
  if (!h || !ctx || !ctx.text.trim()) {
    set({ phrasing: { selected: "", result: null, busy: false, error: "Select some text in a scene first." } });
    return;
  }
  set({ phrasing: { selected: ctx.text, result: null, busy: true, error: null } });
  try {
    const result = await memory.margin.phrasing({ docId: h.docId, selected: ctx.text, before: ctx.before, after: ctx.after });
    set({ phrasing: { selected: ctx.text, result, busy: false, error: null } });
  } catch (e) {
    set({ phrasing: { selected: ctx.text, result: null, busy: false, error: errMessage(e) } });
  }
}
