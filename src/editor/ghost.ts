/**
 * Ghost text (SPEC §7): a suggestion shown dimmed after the cursor.
 *
 * It is a ProseMirror widget decoration and nothing else: never a document
 * node, never in the undo history, never saved (SPEC §2.1). Only an explicit
 * accept inserts text, as one undo step. Every key press cancels in-flight
 * generation (SPEC §2.2).
 */
import { Extension } from "@tiptap/core";
import { closeHistory } from "@tiptap/pm/history";
import { Plugin, PluginKey, TextSelection, type EditorState, type Transaction } from "@tiptap/pm/state";
import { ReplaceStep } from "@tiptap/pm/transform";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";

import { GHOST_ACCEPT, offsetAt, plainText, type AcceptMeta } from "./provenance";

export type GhostLength = "word" | "phrase" | "sentence";
export type GhostWireState = "streaming" | "done" | "empty" | "cancelled" | "failed";

export interface GhostTypography {
  quotes: boolean;
  dashes: boolean;
  ellipsis: boolean;
}

export interface GhostRequestArgs {
  requestId: string;
  docId: string;
  textBefore: string;
  length: GhostLength;
  variant: number;
  typography: GhostTypography;
}

/** The backend as the editor sees it. The SDK's `ai.ghost` implements it. */
export interface GhostClient {
  start(req: GhostRequestArgs): Promise<unknown>;
  cancel(requestId: string): Promise<unknown>;
  keystroke(): Promise<unknown>;
  subscribe(handler: (e: { requestId: string; state: GhostWireState; text: string }) => void): () => void;
}

export interface GhostOptions {
  docId: string;
  client: GhostClient;
  /** Ghost text is on (master switch and not Flow mode). */
  enabled: () => boolean;
  /** The model server can take a request. */
  ready: () => boolean;
  debounceMs: () => number;
  length: () => GhostLength;
  opacity: () => number;
  italic: () => boolean;
  neverPush: () => boolean;
  typography: () => GhostTypography;
  /** The editor has the keyboard focus. */
  focused?: (view: EditorView) => boolean;
  /** Text was accepted (for the session's AI-accepted word count). */
  onAccept?: (info: { text: string; from: number; to: number }) => void;
  now?: () => number;
}

/** Metadata the remote-edit mirror sets on transactions that came from another pane. */
const REMOTE = "marginRemote";

export const DISMISS_QUIET_MS = 1500;
export const MIN_DEBOUNCE_MS = 250;
export const MAX_DEBOUNCE_MS = 2000;
const MAX_ALTERNATIVES = 3;
/** A keystroke this recent may still be in flight to the backend. */
const KEYSTROKE_RACE_MS = 150;
const MAX_TEXT_BEFORE = 30_000;

export interface Ghost {
  pos: number;
  text: string;
  requestId: string;
  /** First time this suggestion shows (fade it in). */
  fresh: boolean;
}

type LastEdit = "none" | "type" | "delete" | "accept" | "remote" | "other";

interface GhostPluginState {
  ghost: Ghost | null;
  last: LastEdit;
  /** Counts document changes. */
  seq: number;
}

export const ghostKey = new PluginKey<GhostPluginState>("margin-ghost");

export function currentGhost(state: EditorState): Ghost | null {
  return ghostKey.getState(state)?.ghost ?? null;
}

/** The text a transaction inserted at `pos` and nothing else, if that is all it did. */
function pureInsertion(tr: Transaction, pos: number): string | null {
  if (tr.steps.length !== 1) return null;
  const step = tr.steps[0];
  if (!(step instanceof ReplaceStep)) return null;
  const { from, to, slice } = step as ReplaceStep & { from: number; to: number };
  if (from !== pos || to !== pos || slice.openStart !== 0 || slice.openEnd !== 0) return null;
  if (slice.content.childCount !== 1 || !slice.content.firstChild?.isText) return null;
  return slice.content.firstChild.text ?? null;
}

function classify(tr: Transaction): LastEdit {
  if (tr.getMeta(GHOST_ACCEPT)) return "accept";
  if (tr.getMeta(REMOTE)) return "remote";
  const steps = tr.steps as (ReplaceStep & { from: number; to: number })[];
  if (steps.length > 0 && steps.every((s) => s instanceof ReplaceStep && s.slice.size === 0 && s.to > s.from)) return "delete";
  if (steps.some((s) => s instanceof ReplaceStep && s.slice.size > 0)) return "type";
  return "other";
}

const wordCount = (s: string): number => s.split(/\s+/).filter(Boolean).length;

/**
 * Where a suggestion may appear (SPEC §7.1): a body paragraph, no selection,
 * the cursor at the end of the paragraph (only whitespace after it), and the
 * current or previous paragraph has at least three words.
 */
export function ghostSite(state: EditorState): { head: number; textBefore: string } | null {
  const sel = state.selection;
  if (!sel.empty) return null;
  const $head = sel.$head;
  // Body paragraphs sit directly in the document: not headings, quotes, lists or styled blocks.
  if ($head.depth !== 1 || $head.parent.type.name !== "paragraph") return null;
  const after = $head.parent.textBetween($head.parentOffset, $head.parent.content.size, "", "￼");
  if (after.trim() !== "") return null;
  const index = $head.index(0);
  const prev = index > 0 ? state.doc.child(index - 1) : null;
  const words = wordCount($head.parent.textContent) + (prev?.type.name === "paragraph" ? wordCount(prev.textContent) : 0);
  if (words < 3) return null;
  // The same text provenance uses: empty paragraphs leave no blank lines.
  let textBefore = plainText(state.doc).slice(0, offsetAt(state.doc, $head.pos));
  if (textBefore.length > MAX_TEXT_BEFORE) {
    const cut = textBefore.indexOf("\n\n", textBefore.length - MAX_TEXT_BEFORE);
    textBefore = cut >= 0 ? textBefore.slice(cut + 2) : textBefore.slice(-MAX_TEXT_BEFORE);
  }
  return { head: $head.pos, textBefore };
}

interface Pending {
  id: string;
  pos: number;
  /** The document the request was made for; any edit makes the answer stale. */
  doc: EditorState["doc"];
  variant: number;
  /** Which alternative was showing, to go back to it if the new one repeats. */
  prevIndex: number;
  /** This request is the one retry after an alternative repeated an earlier one. */
  retried: boolean;
}

const noop = (): void => undefined;

export class GhostController {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private pending: Pending | null = null;
  private alts: string[] = [];
  private altIndex = 0;
  private dismissed: { pos: number; at: number } | null = null;
  private deleteStreak = false;
  private counter = 0;
  /** The latest keystroke notification still on its way to the backend. */
  private keystroke: { at: number; done: Promise<unknown> } | null = null;
  private readonly unsubscribe: () => void;

  constructor(
    readonly view: EditorView,
    private readonly opts: GhostOptions,
  ) {
    this.unsubscribe = opts.client.subscribe((e) => this.onEvent(e));
  }

  destroy(): void {
    clearTimeout(this.timer);
    this.unsubscribe();
    this.cancelPending();
  }

  private now(): number {
    return this.opts.now?.() ?? Date.now();
  }

  get visible(): boolean {
    return currentGhost(this.view.state) !== null;
  }

  /** Any key (SPEC §2.2): stop generating now, and tell the scheduler. */
  onKey(): void {
    clearTimeout(this.timer);
    this.cancelPending();
    const done = this.opts.client.keystroke().catch(noop);
    const sent = { at: Date.now(), done };
    this.keystroke = sent;
    void done.then(() => {
      if (this.keystroke === sent) this.keystroke = null;
    });
  }

  /** The editor lost the keyboard: a suggestion nobody is looking at is noise. */
  onBlur(): void {
    clearTimeout(this.timer);
    this.cancelPending();
    if (this.visible) this.setGhost(null);
  }

  private cancelPending(): void {
    if (!this.pending) return;
    const id = this.pending.id;
    this.pending = null;
    this.opts.client.cancel(id).catch(noop);
  }

  /** After every state update: reschedule on typing, drop what the edit made stale. */
  afterUpdate(prev: EditorState): void {
    const view = this.view;
    if (view.isDestroyed) return;
    const st = ghostKey.getState(view.state);
    const before = ghostKey.getState(prev);
    if (!st || !before) return;
    if (st.seq !== before.seq) {
      clearTimeout(this.timer);
      if (st.last === "delete") this.deleteStreak = true;
      else if (st.last === "type") {
        this.deleteStreak = false;
        this.schedule();
      }
      if (this.pending && this.pending.doc !== view.state.doc) this.cancelPending();
    } else if (!view.state.selection.eq(prev.selection)) {
      clearTimeout(this.timer);
      if (this.pending && (!view.state.selection.empty || view.state.selection.head !== this.pending.pos)) this.cancelPending();
    }
    if (!st.ghost && !this.pending) {
      this.alts = [];
      this.altIndex = 0;
    }
  }

  private schedule(): void {
    const ms = Math.min(MAX_DEBOUNCE_MS, Math.max(MIN_DEBOUNCE_MS, this.opts.debounceMs()));
    this.timer = setTimeout(() => this.request(0, false), ms);
  }

  /** Ask for a suggestion. `manual` skips the typing-pause rules (cycling alternatives). */
  request(variant: number, manual: boolean, retried = false): void {
    const { view, opts } = this;
    if (view.isDestroyed) return;
    if (!opts.ready()) return;
    if (!manual && (!opts.enabled() || this.deleteStreak)) return;
    if (manual && !opts.enabled()) return;
    if (!(opts.focused ?? ((v) => v.hasFocus()))(view) || view.composing) return;
    const site = ghostSite(view.state);
    if (!site) return;
    if (!manual) {
      if (this.visible) return;
      if (this.dismissed && this.dismissed.pos === site.head && this.now() - this.dismissed.at < DISMISS_QUIET_MS) return;
    }
    const prevIndex = this.altIndex;
    this.cancelPending();
    const id = `g${++this.counter}${Math.random().toString(36).slice(2, 6)}`;
    this.pending = { id, pos: site.head, doc: view.state.doc, variant, prevIndex, retried };
    const req: GhostRequestArgs = {
      requestId: id,
      docId: opts.docId,
      textBefore: site.textBefore,
      length: opts.length(),
      variant,
      typography: opts.typography(),
    };
    const failed = () => {
      if (this.pending?.id === id) this.pending = null;
    };
    // IPC calls are separate requests and can overtake each other. A request made right
    // after a key press (Alt+] to ask for an alternative) must not reach the backend
    // before that key's notification, or the notification would cancel it.
    const ks = this.keystroke;
    if (ks && Date.now() - ks.at < KEYSTROKE_RACE_MS) {
      void ks.done.then(() => {
        // Superseded or cancelled while waiting: do not start it.
        if (this.pending?.id === id) opts.client.start(req).catch(failed);
      });
    } else {
      opts.client.start(req).catch(failed);
    }
  }

  private onEvent(e: { requestId: string; state: GhostWireState; text: string }): void {
    const p = this.pending;
    const view = this.view;
    if (!p || p.id !== e.requestId || view.isDestroyed) return;
    const sel = view.state.selection;
    if (view.state.doc !== p.doc || !sel.empty || sel.head !== p.pos) {
      this.cancelPending();
      return;
    }
    if (e.state === "streaming") {
      this.show(p, e.text);
    } else if (e.state === "done") {
      this.pending = null;
      if (p.variant > 0 && this.alts.includes(e.text)) {
        // The same suggestion again: go back to the one that was showing, and try once more.
        const back = this.alts[p.prevIndex];
        if (back !== undefined) this.show(p, back);
        if (!p.retried) this.request(p.variant, true, true);
        return;
      }
      this.alts[p.variant] = e.text;
      this.altIndex = p.variant;
      this.show(p, e.text);
    } else {
      this.pending = null;
      // Nothing worth showing: take back what streamed so far (unless an alternative is still there).
      const g = currentGhost(view.state);
      if (g?.requestId === p.id && e.state === "empty") this.setGhost(null);
    }
  }

  private show(p: Pending, text: string): void {
    const cur = currentGhost(this.view.state);
    const fresh = !cur || cur.requestId !== p.id;
    this.setGhost({ pos: p.pos, text, requestId: p.id, fresh });
  }

  private setGhost(ghost: Ghost | null): void {
    if (this.view.isDestroyed) return;
    this.view.dispatch(this.view.state.tr.setMeta(ghostKey, { ghost }).setMeta("addToHistory", false));
  }

  /** Insert all of the suggestion, or its next word, as one undo step. */
  accept(mode: "all" | "word"): boolean {
    const { view } = this;
    const g = currentGhost(view.state);
    if (!g) return false;
    let chunk = g.text;
    if (mode === "word") {
      const m = /^\s*\S+/.exec(g.text);
      if (m) chunk = m[0];
    }
    const rest = g.text.slice(chunk.length);
    const from = g.pos;
    const to = from + chunk.length;
    const tr = view.state.tr.insertText(chunk, from);
    tr.setSelection(TextSelection.near(tr.doc.resolve(to), -1));
    const meta: AcceptMeta = { from, to };
    tr.setMeta(GHOST_ACCEPT, meta);
    // The rest of the suggestion stays (word accept); all of it, or nothing left, clears it.
    tr.setMeta(ghostKey, { ghost: rest.trim() ? { pos: to, text: rest, requestId: g.requestId, fresh: false } : null });
    closeHistory(tr);
    view.dispatch(tr.scrollIntoView());
    if (!rest.trim()) this.cancelPending();
    this.opts.onAccept?.({ text: chunk, from, to });
    return true;
  }

  dismiss(): boolean {
    const g = currentGhost(this.view.state);
    if (!g && !this.pending) return false;
    this.dismissed = { pos: g?.pos ?? this.pending?.pos ?? -1, at: this.now() };
    this.cancelPending();
    if (g) this.setGhost(null);
    return true;
  }

  /** Next (+1) or previous (-1) alternative; the next one is fetched lazily, up to three. */
  cycle(dir: 1 | -1): boolean {
    const g = currentGhost(this.view.state);
    if (!g) {
      if (dir > 0) this.request(0, true);
      return dir > 0;
    }
    if (this.pending) return true;
    if (this.alts.length === 0) this.alts[0] = g.text;
    const next = this.altIndex + dir;
    if (next < 0) return true;
    const known = this.alts[next];
    if (known !== undefined) {
      this.altIndex = next;
      this.setGhost({ ...g, text: known, fresh: false });
    } else if (next < MAX_ALTERNATIVES) {
      this.request(next, true);
    }
    return true;
  }
}

const controllers = new WeakMap<EditorView, GhostController>();

/** The ghost controller of an editor view, for commands and tests. */
export function ghostControl(view: EditorView): GhostController | undefined {
  return controllers.get(view);
}

function ghostDom(view: EditorView, g: Ghost, o: GhostOptions): HTMLElement {
  const el = document.createElement("span");
  el.className = "ghost-text";
  if (g.fresh) el.classList.add("ghost-text--fresh");
  if (o.italic()) el.classList.add("ghost-text--italic");
  el.setAttribute("aria-hidden", "true");
  el.setAttribute("contenteditable", "false");
  el.style.setProperty("--ghost-opacity", String(o.opacity()));
  el.textContent = g.text;
  if (o.neverPush()) {
    // Hidden until measured, then clipped to the room left on this line (SPEC §7.3).
    el.classList.add("ghost-text--clip");
    const later = typeof requestAnimationFrame === "function" ? requestAnimationFrame : (fn: () => void) => setTimeout(fn, 0);
    later(() => clipToLine(view, el, g.pos));
  }
  return el;
}

function clipToLine(view: EditorView, el: HTMLElement, pos: number): void {
  if (view.isDestroyed || !el.isConnected) return;
  try {
    const caret = view.coordsAtPos(pos);
    const box = view.dom.getBoundingClientRect();
    const pad = parseFloat(getComputedStyle(view.dom).paddingRight) || 0;
    el.style.maxWidth = `${Math.max(0, box.right - pad - caret.left)}px`;
    el.classList.add("ghost-text--ready");
  } catch {
    el.classList.add("ghost-text--ready");
  }
}

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock", "NumLock", "ScrollLock", "OS"]);

export const GhostText = Extension.create<GhostOptions>({
  name: "ghost",
  addProseMirrorPlugins() {
    const opts = this.options;
    return [
      new Plugin<GhostPluginState>({
        key: ghostKey,
        state: {
          init: () => ({ ghost: null, last: "none", seq: 0 }),
          apply(tr, prev) {
            const meta = tr.getMeta(ghostKey) as { ghost: Ghost | null } | undefined;
            let ghost = prev.ghost;
            const last = tr.docChanged ? classify(tr) : prev.last;
            if (meta) {
              ghost = meta.ghost;
            } else if (ghost) {
              if (tr.docChanged) {
                // Typing the start of the suggestion shrinks it; anything else drops it.
                const typed = pureInsertion(tr, ghost.pos);
                if (typed && ghost.text.startsWith(typed)) {
                  const rest = ghost.text.slice(typed.length);
                  ghost = rest ? { ...ghost, pos: ghost.pos + typed.length, text: rest, fresh: false } : null;
                } else {
                  ghost = null;
                }
              } else if (tr.selectionSet && !(tr.selection.empty && tr.selection.head === ghost.pos)) {
                ghost = null;
              }
            }
            return { ghost, last, seq: prev.seq + (tr.docChanged ? 1 : 0) };
          },
        },
        props: {
          decorations(state) {
            const g = ghostKey.getState(state)?.ghost;
            if (!g || g.pos > state.doc.content.size) return null;
            return DecorationSet.create(state.doc, [
              Decoration.widget(g.pos, (view) => ghostDom(view, g, opts), {
                side: 1,
                key: `ghost:${g.requestId}:${g.text}`,
                ignoreSelection: true,
              }),
            ]);
          },
          // ProseMirror consumes Escape itself, so it never reaches the app's keymap
          // while the editor has focus: dismissing has to happen here.
          handleKeyDown(view, event) {
            if (event.key !== "Escape") return false;
            const c = controllers.get(view);
            return c?.visible ? c.dismiss() : false;
          },
          handleDOMEvents: {
            keydown(view, event) {
              if (!MODIFIER_KEYS.has(event.key)) controllers.get(view)?.onKey();
              return false;
            },
            blur(view) {
              controllers.get(view)?.onBlur();
              return false;
            },
            compositionstart(view) {
              controllers.get(view)?.onKey();
              return false;
            },
          },
        },
        view(view) {
          const controller = new GhostController(view, opts);
          controllers.set(view, controller);
          return {
            update: (_v, prev) => controller.afterUpdate(prev),
            destroy: () => {
              controller.destroy();
              controllers.delete(view);
            },
          };
        },
      }),
    ];
  },
});
