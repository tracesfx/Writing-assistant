/**
 * Bookmarks inside documents (SPEC §11.4): `Ctrl+F2` toggles one on the
 * current paragraph, `F2` jumps to the next. Positions are mapped through
 * every edit; they are view state, never part of the document.
 */
import { Extension } from "@tiptap/core";
import { Plugin, PluginKey, TextSelection, type EditorState, type Transaction } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";

interface BookmarkState {
  positions: number[];
  decos: DecorationSet;
}

export const bookmarkKey = new PluginKey<BookmarkState>("marginBookmarks");

type Meta = { set: number[] } | { toggle: number };

/** Start of the text block containing `pos`. */
function blockStart(state: EditorState, pos: number): number | null {
  const $p = state.doc.resolve(Math.min(Math.max(pos, 0), state.doc.content.size));
  for (let d = $p.depth; d > 0; d--) {
    if ($p.node(d).isTextblock) return $p.before(d);
  }
  return null;
}

function build(state: { doc: EditorState["doc"] }, positions: number[]): DecorationSet {
  const decos: Decoration[] = [];
  for (const p of positions) {
    const node = state.doc.nodeAt(p);
    if (node?.isTextblock) decos.push(Decoration.node(p, p + node.nodeSize, { class: "bookmarked" }));
  }
  return DecorationSet.create(state.doc, decos);
}

function normalize(state: { doc: EditorState["doc"] }, positions: number[]): number[] {
  const valid = positions.filter((p) => p >= 0 && p < state.doc.content.size && state.doc.nodeAt(p)?.isTextblock);
  return [...new Set(valid)].sort((a, b) => a - b);
}

/** Delay before positions moved by edits are reported. */
export const BOOKMARK_SAVE_MS = 1500;

export const Bookmarks = Extension.create<{ initial: number[]; onChange: (positions: number[]) => void }>({
  name: "bookmarks",
  addOptions() {
    return { initial: [], onChange: () => undefined };
  },
  addProseMirrorPlugins() {
    const opts = this.options;
    // Positions shifted by typing are reported once the author pauses, so the
    // saved session never points at the wrong paragraph after a restart.
    let shiftTimer: ReturnType<typeof setTimeout> | undefined;
    return [
      new Plugin<BookmarkState>({
        key: bookmarkKey,
        state: {
          init: (_, state) => {
            const positions = normalize(state, opts.initial);
            return { positions, decos: build(state, positions) };
          },
          apply(tr: Transaction, value, _old, state) {
            const meta = tr.getMeta(bookmarkKey) as Meta | undefined;
            let positions = value.positions;
            if (tr.docChanged) {
              positions = positions.map((p) => tr.mapping.map(p, 1));
            }
            if (meta && "set" in meta) positions = meta.set;
            if (meta && "toggle" in meta) {
              positions = positions.includes(meta.toggle)
                ? positions.filter((p) => p !== meta.toggle)
                : [...positions, meta.toggle];
            }
            if (!tr.docChanged && !meta) return value;
            positions = normalize(state, positions);
            if (meta) {
              clearTimeout(shiftTimer);
              queueMicrotask(() => opts.onChange(positions));
            } else if (positions.join() !== value.positions.join()) {
              clearTimeout(shiftTimer);
              const shifted = positions;
              shiftTimer = setTimeout(() => opts.onChange(shifted), BOOKMARK_SAVE_MS);
            }
            return { positions, decos: build(state, positions) };
          },
        },
        props: {
          decorations: (state) => bookmarkKey.getState(state)?.decos ?? null,
        },
      }),
    ];
  },
});

export function bookmarkPositions(state: EditorState): number[] {
  return bookmarkKey.getState(state)?.positions ?? [];
}

export function toggleBookmark(view: EditorView): void {
  const start = blockStart(view.state, view.state.selection.head);
  if (start === null) return;
  view.dispatch(view.state.tr.setMeta(bookmarkKey, { toggle: start }));
}

/** Move to the next bookmark after the cursor (wrapping); false if none. */
export function nextBookmark(view: EditorView, dir: 1 | -1 = 1): boolean {
  const list = bookmarkPositions(view.state);
  if (list.length === 0) return false;
  const head = blockStart(view.state, view.state.selection.head) ?? view.state.selection.head;
  const target =
    dir === 1 ? (list.find((p) => p > head) ?? list[0]!) : ([...list].reverse().find((p) => p < head) ?? list.at(-1)!);
  const sel = TextSelection.near(view.state.doc.resolve(target + 1));
  view.dispatch(view.state.tr.setSelection(sel).scrollIntoView());
  return true;
}
