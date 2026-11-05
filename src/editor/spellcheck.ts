/**
 * Spellcheck decorations (SPEC §11.7). Paragraph texts are checked by the
 * backend after a typing pause, off the UI thread; results are cached by
 * text so unchanged paragraphs are never re-sent. The word being typed is
 * not flagged.
 */
import { Extension } from "@tiptap/core";
import { Plugin, PluginKey, type EditorState } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";

import { textBlocks } from "./textmap";

export interface SpellRangeLike {
  start: number;
  end: number;
}

export interface SpellOptions {
  enabled: () => boolean;
  check: (texts: string[]) => Promise<SpellRangeLike[][]>;
  /** Words the author chose to ignore this session. */
  ignored: () => Set<string>;
  onMenu: (m: { word: string; from: number; to: number; x: number; y: number }) => void;
}

export const spellKey = new PluginKey<DecorationSet>("marginSpell");

const IDLE_MS = 450;
const MAX_CACHE = 6000;
const BATCH = 300;

const spellViews = new WeakMap<EditorView, { schedule(ms: number): void }>();

/** Results shared by every editor (keyed by paragraph text). */
const cache = new Map<string, SpellRangeLike[]>();

export function clearSpellCache(): void {
  cache.clear();
}

function buildDecos(state: EditorState, opts: SpellOptions): DecorationSet {
  if (!opts.enabled()) return DecorationSet.empty;
  const ignored = opts.ignored();
  const head = state.selection.head;
  const decos: Decoration[] = [];
  for (const b of textBlocks(state.doc)) {
    const ranges = cache.get(b.text);
    if (!ranges) continue;
    for (const r of ranges) {
      const from = b.pos + r.start;
      const to = b.pos + r.end;
      const word = b.text.slice(r.start, r.end);
      if (ignored.has(word)) continue;
      if (state.selection.empty && to === head) continue; // still typing it
      decos.push(Decoration.inline(from, to, { class: "misspelled", "data-word": word }));
    }
  }
  return DecorationSet.create(state.doc, decos);
}

class SpellView {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private running = false;
  private destroyed = false;

  constructor(
    private view: EditorView,
    private opts: SpellOptions,
  ) {
    spellViews.set(view, this);
    this.schedule(50);
  }

  update(view: EditorView, prev: EditorState): void {
    this.view = view;
    if (!prev.doc.eq(view.state.doc)) this.schedule(IDLE_MS);
  }

  schedule(ms: number): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.run(), ms);
  }

  private async run(): Promise<void> {
    if (this.destroyed || this.running) return;
    const view = this.view;
    if (!this.opts.enabled()) {
      if (spellKey.getState(view.state) !== DecorationSet.empty) {
        view.dispatch(view.state.tr.setMeta(spellKey, DecorationSet.empty));
      }
      return;
    }
    const missing = [...new Set(textBlocks(view.state.doc).map((b) => b.text))].filter((t) => !cache.has(t));
    if (missing.length > 0) {
      this.running = true;
      try {
        for (let i = 0; i < missing.length; i += BATCH) {
          const batch = missing.slice(i, i + BATCH);
          const res = await this.opts.check(batch);
          batch.forEach((t, k) => cache.set(t, res[k] ?? []));
        }
        while (cache.size > MAX_CACHE) {
          const first = cache.keys().next().value;
          if (first === undefined) break;
          cache.delete(first);
        }
      } catch {
        // No project or dictionary yet: try again later, quietly.
        this.running = false;
        this.schedule(5000);
        return;
      }
      this.running = false;
    }
    if (this.destroyed) return;
    const v = this.view;
    v.dispatch(v.state.tr.setMeta(spellKey, buildDecos(v.state, this.opts)).setMeta("addToHistory", false));
  }

  destroy(): void {
    this.destroyed = true;
    clearTimeout(this.timer);
  }
}

export const Spellcheck = Extension.create<SpellOptions>({
  name: "spellcheck",
  addOptions() {
    return {
      enabled: () => false,
      check: async () => [],
      ignored: () => new Set<string>(),
      onMenu: () => undefined,
    };
  },
  addProseMirrorPlugins() {
    const opts = this.options;
    return [
      new Plugin<DecorationSet>({
        key: spellKey,
        state: {
          init: () => DecorationSet.empty,
          apply(tr, value) {
            const meta = tr.getMeta(spellKey) as DecorationSet | undefined;
            if (meta instanceof DecorationSet) return meta;
            return tr.docChanged ? value.map(tr.mapping, tr.doc) : value;
          },
        },
        view: (view) => new SpellView(view, opts),
        props: {
          decorations: (state) => spellKey.getState(state) ?? null,
          handleDOMEvents: {
            contextmenu(view, event) {
              const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
              if (!at) return false;
              const set = spellKey.getState(view.state);
              const hit = set?.find(at.pos, at.pos)[0];
              if (!hit) return false;
              event.preventDefault();
              const word = view.state.doc.textBetween(hit.from, hit.to);
              opts.onMenu({ word, from: hit.from, to: hit.to, x: event.clientX, y: event.clientY });
              return true;
            },
          },
        },
      }),
    ];
  },
});

/** Re-check (e.g. after adding a word or changing the dictionary). */
export function refreshSpelling(view: EditorView, opts: { clearCache?: boolean } = {}): void {
  if (opts.clearCache) clearSpellCache();
  spellViews.get(view)?.schedule(0);
}
