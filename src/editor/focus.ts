/**
 * Focus mode (dim everything but the current sentence or paragraph) and
 * typewriter scrolling (keep the caret line at a fixed height), SPEC §11.2.
 * Both are view-only: decorations and scrolling, never document changes.
 */
import { Extension } from "@tiptap/core";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

import type { FocusMode as Mode } from "../sdk/settings";

export const focusKey = new PluginKey("marginFocus");

/** Sentence containing `offset` in `text`: [start, end). */
export function sentenceAt(text: string, offset: number): [number, number] {
  const re = /[.!?…]+["”’)\]]*(?=\s|$)/gu;
  let start = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const end = m.index + m[0].length;
    if (end >= offset && offset >= start) {
      // A caret right after the full stop still belongs to that sentence.
      return [start, end];
    }
    start = end;
    while (start < text.length && /\s/u.test(text[start]!)) start++;
  }
  return [start, text.length];
}

export const FocusMode = Extension.create<{ mode: () => Mode }>({
  name: "focusMode",
  addOptions() {
    return { mode: () => "off" as Mode };
  },
  addProseMirrorPlugins() {
    const opts = this.options;
    return [
      new Plugin({
        key: focusKey,
        props: {
          decorations(state) {
            const mode = opts.mode();
            if (mode === "off") return null;
            const { $head } = state.selection;
            if ($head.depth < 1) return null;
            const decos: Decoration[] = [];
            const top = $head.before(1);
            const topNode = state.doc.nodeAt(top);
            if (topNode) decos.push(Decoration.node(top, top + topNode.nodeSize, { class: "focus-current" }));
            if (mode === "sentence" && $head.parent.isTextblock) {
              const start = $head.start();
              const text = state.doc.textBetween(start, $head.end(), "\n", "￼");
              const [s, e] = sentenceAt(text, $head.parentOffset);
              if (e > s) decos.push(Decoration.inline(start + s, start + e, { class: "focus-sentence" }));
            }
            return DecorationSet.create(state.doc, decos);
          },
          attributes(): Record<string, string> {
            const mode = opts.mode();
            return mode === "off" ? {} : { class: `focus-mode focus-mode--${mode}` };
          },
        },
      }),
    ];
  },
});

/** Fraction of the viewport height where the caret line is kept. */
const TYPEWRITER_LINE = 0.45;

export const Typewriter = Extension.create<{ enabled: () => boolean }>({
  name: "typewriter",
  addOptions() {
    return { enabled: () => false };
  },
  addProseMirrorPlugins() {
    const opts = this.options;
    let frame = 0;
    return [
      new Plugin({
        view: () => ({
          update(view, prev) {
            if (!opts.enabled() || !view.hasFocus()) return;
            if (prev.doc.eq(view.state.doc) && prev.selection.eq(view.state.selection)) return;
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(() => {
              const scroller = view.dom.closest(".editor-scroll");
              if (!(scroller instanceof HTMLElement)) return;
              let coords: { top: number; bottom: number };
              try {
                coords = view.coordsAtPos(view.state.selection.head);
              } catch {
                return;
              }
              const rect = scroller.getBoundingClientRect();
              const target = rect.top + rect.height * TYPEWRITER_LINE;
              const delta = (coords.top + coords.bottom) / 2 - target;
              if (Math.abs(delta) > 2) scroller.scrollTop += delta;
            });
          },
          destroy() {
            cancelAnimationFrame(frame);
          },
        }),
        props: {
          attributes(): Record<string, string> {
            return opts.enabled() ? { "data-typewriter": "on" } : {};
          },
        },
      }),
    ];
  },
});
