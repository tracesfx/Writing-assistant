/**
 * Mapping between ProseMirror positions and the backend's text-block
 * offsets (search hits, spelling ranges). A text block is a paragraph or
 * heading; offsets are UTF-16 with a footnote or line break counting as one
 * character, which is exactly one ProseMirror position each.
 */
import type { Node as PmNode } from "@tiptap/pm/model";

import { offsetLeafText } from "./schema";

export interface TextBlock {
  /** Position of the block's first content character. */
  pos: number;
  text: string;
  node: PmNode;
}

/**
 * Text blocks in document order. Empty paragraphs are skipped, because the
 * saved Markdown has none (the backend's block indices count only blocks
 * that exist on disk).
 */
export function textBlocks(doc: PmNode, opts: { keepEmpty?: boolean } = {}): TextBlock[] {
  const out: TextBlock[] = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    const text = doc.textBetween(pos + 1, pos + 1 + node.content.size, "\n", offsetLeafText);
    if (opts.keepEmpty || text.trim() !== "" || node.type.name === "heading") {
      out.push({ pos: pos + 1, text, node });
    }
    return false;
  });
  return out;
}

/**
 * Positions of a backend hit in the current document. Falls back to the
 * nearest occurrence of `expected` if the editor's blocks differ slightly
 * from the saved file (e.g. leading spaces the file normalized away).
 */
export function locate(
  doc: PmNode,
  target: { block: number; start: number; end: number },
  expected?: string,
): { from: number; to: number } | null {
  const blocks = textBlocks(doc);
  const b = blocks[target.block];
  if (b) {
    const slice = b.text.slice(target.start, target.end).replace(/￼/g, "");
    if (!expected || slice === expected) {
      return { from: b.pos + target.start, to: b.pos + target.end };
    }
  }
  if (!expected) return null;
  // Search outward from the expected block.
  const order = blocks
    .map((blk, i) => ({ blk, d: Math.abs(i - target.block) }))
    .sort((a, b2) => a.d - b2.d);
  for (const { blk } of order) {
    const i = blk.text.indexOf(expected);
    if (i >= 0) return { from: blk.pos + i, to: blk.pos + i + expected.length };
  }
  return null;
}
