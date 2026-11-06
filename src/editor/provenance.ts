/**
 * Provenance of accepted AI text (SPEC §7.6).
 *
 * Text that came from an accepted ghost suggestion is tracked as ranges in
 * plugin state, mapped through every edit. It is never a mark or node in the
 * document, so it cannot reach the Markdown (SPEC §17 check 5). Characters the
 * author retypes or edits lose it; text inserted inside a range splits it.
 *
 * The saved form is plain-text offsets plus a hash of that text, so it
 * survives a reload and notices an outside edit.
 */
import { Extension } from "@tiptap/core";
import type { Node as PmNode } from "@tiptap/pm/model";
import { Plugin, PluginKey, type EditorState, type Transaction } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";
import type { StepMap } from "@tiptap/pm/transform";

import { countWords } from "./schema";

/** Transaction meta set by an accepted suggestion: the inserted range. */
export const GHOST_ACCEPT = "ghostAccept";
export interface AcceptMeta {
  from: number;
  to: number;
}

/** A half-open range of document positions. */
export type Range = readonly [number, number];

/** Sort, drop empty ranges and merge ranges that touch or overlap. */
export function normalize(ranges: readonly Range[]): Range[] {
  const sorted = ranges.filter(([a, b]) => b > a).sort((x, y) => x[0] - y[0]);
  const out: [number, number][] = [];
  for (const [a, b] of sorted) {
    const last = out[out.length - 1];
    if (last && a <= last[1]) last[1] = Math.max(last[1], b);
    else out.push([a, b]);
  }
  return out;
}

function splitAt(ranges: readonly Range[], pos: number): Range[] {
  return ranges.flatMap((r): Range[] => (r[0] < pos && pos < r[1] ? [[r[0], pos], [pos, r[1]]] : [r]));
}

function subtract(ranges: readonly Range[], a: number, b: number): Range[] {
  return ranges.flatMap((r): Range[] => {
    if (r[1] <= a || r[0] >= b) return [r];
    const out: Range[] = [];
    if (r[0] < a) out.push([r[0], a]);
    if (r[1] > b) out.push([b, r[1]]);
    return out;
  });
}

/** Carry ranges through one step: replaced text leaves them, inserted text is not in them. */
export function mapThroughStep(ranges: readonly Range[], map: StepMap): Range[] {
  let rs: Range[] = [...ranges];
  map.forEach((oldStart, oldEnd) => {
    rs = splitAt(rs, oldStart);
    if (oldEnd > oldStart) rs = subtract(rs, oldStart, oldEnd);
  });
  // Starts move after text inserted at them, ends stay before it.
  return normalize(rs.map(([a, b]): Range => [map.map(a, 1), map.map(b, -1)]));
}

export function mapRanges(ranges: readonly Range[], tr: Transaction): Range[] {
  let rs = ranges;
  for (const step of tr.steps) rs = mapThroughStep(rs, step.getMap());
  return normalize(rs);
}

// --- Saved form: plain-text offsets ---------------------------------------------------

/**
 * The text provenance offsets refer to: the text of each non-empty text block,
 * joined by a blank line. Empty paragraphs are left out because the Markdown
 * file cannot hold them, so the text (and its hash) is the same after a reload.
 */
export function plainText(doc: PmNode): string {
  const blocks: string[] = [];
  doc.descendants((node) => {
    if (!node.isTextblock) return true;
    if (node.textContent !== "") blocks.push(node.textContent);
    return false;
  });
  return blocks.join("\n\n");
}

/** UTF-16 offset in `plainText` of a document position. */
export function offsetAt(doc: PmNode, pos: number): number {
  let offset = 0;
  let first = true;
  let found = -1;
  doc.descendants((node, p) => {
    if (found >= 0) return false;
    if (!node.isTextblock) return true;
    const text = node.textContent;
    if (text === "") return false;
    const start = p + 1;
    const end = start + node.content.size;
    if (pos < start) {
      found = offset;
      return false;
    }
    const sep = first ? 0 : 2;
    if (pos <= end) {
      found = offset + sep + node.textBetween(0, pos - start, "", "").length;
      return false;
    }
    offset += sep + text.length;
    first = false;
    return false;
  });
  return found >= 0 ? found : offset;
}

/**
 * The position for a plain-text offset. A range starts at the last position
 * that still has this offset (inside its paragraph) and ends at the first.
 */
export function posAt(doc: PmNode, offset: number, edge: "start" | "end"): number {
  let lo = 0;
  let hi = doc.content.size;
  if (edge === "end") {
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (offsetAt(doc, mid) >= offset) hi = mid;
      else lo = mid + 1;
    }
    return lo;
  }
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (offsetAt(doc, mid) <= offset) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

export interface OffsetRange {
  start: number;
  end: number;
}

/** Ranges as text offsets. Ranges holding no text (only structure) are dropped. */
export function toOffsets(doc: PmNode, ranges: readonly Range[]): OffsetRange[] {
  const out: OffsetRange[] = [];
  for (const [a, b] of ranges) {
    const start = offsetAt(doc, a);
    const end = offsetAt(doc, b);
    const last = out[out.length - 1];
    if (end <= start) continue;
    if (last && start <= last.end) last.end = Math.max(last.end, end);
    else out.push({ start, end });
  }
  return out;
}

export function fromOffsets(doc: PmNode, offsets: readonly OffsetRange[]): Range[] {
  return normalize(offsets.map((o): Range => [posAt(doc, o.start, "start"), posAt(doc, o.end, "end")]));
}

/** Words inside the ranges. */
export function aiWordCount(doc: PmNode, ranges: readonly Range[]): number {
  let n = 0;
  for (const [a, b] of ranges) n += countWords(doc.textBetween(a, b, "\n\n", ""));
  return n;
}

// --- SHA-256 (the record's text hash; WebCrypto is async and absent in some test hosts) ---

const PRIMES: number[] = [];
for (let n = 2; PRIMES.length < 64; n++) if (PRIMES.every((p) => n % p !== 0)) PRIMES.push(n);
const frac32 = (x: number): number => Math.floor((x - Math.floor(x)) * 2 ** 32) >>> 0;
const K = PRIMES.map((p) => frac32(Math.cbrt(p)));
const H0 = PRIMES.slice(0, 8).map((p) => frac32(Math.sqrt(p)));
const rotr = (x: number, n: number): number => (x >>> n) | (x << (32 - n));

export function sha256Hex(text: string): string {
  const msg = new TextEncoder().encode(text);
  const len = msg.length;
  const padded = ((len + 9 + 63) >> 6) << 6;
  const buf = new Uint8Array(padded);
  buf.set(msg);
  buf[len] = 0x80;
  const view = new DataView(buf.buffer);
  view.setUint32(padded - 8, Math.floor((len * 8) / 2 ** 32));
  view.setUint32(padded - 4, (len * 8) >>> 0);
  const h = Uint32Array.from(H0);
  const w = new Uint32Array(64);
  for (let off = 0; off < padded; off += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(off + i * 4);
    for (let i = 16; i < 64; i++) {
      const a = w[i - 15]!;
      const b = w[i - 2]!;
      const s0 = rotr(a, 7) ^ rotr(a, 18) ^ (a >>> 3);
      const s1 = rotr(b, 17) ^ rotr(b, 19) ^ (b >>> 10);
      w[i] = (w[i - 16]! + s0 + w[i - 7]! + s1) >>> 0;
    }
    let a = h[0]!;
    let b = h[1]!;
    let c = h[2]!;
    let d = h[3]!;
    let e = h[4]!;
    let f = h[5]!;
    let g = h[6]!;
    let hh = h[7]!;
    for (let i = 0; i < 64; i++) {
      const t1 = (hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + K[i]! + w[i]!) >>> 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    const sum = [a, b, c, d, e, f, g, hh];
    for (let i = 0; i < 8; i++) h[i] = (h[i]! + sum[i]!) >>> 0;
  }
  return [...h].map((x) => x.toString(16).padStart(8, "0")).join("");
}

// --- The plugin -------------------------------------------------------------------------

export interface ProvenanceOptions {
  /** Tint AI-assisted text (off by default). Read at render time. */
  highlight: () => boolean;
  /** The ranges changed, or the text moved under existing ranges. */
  onChange?: (ranges: readonly Range[], doc: PmNode) => void;
}

interface ProvState {
  ranges: Range[];
}

export const provenanceKey = new PluginKey<ProvState>("margin-provenance");

export function aiRanges(state: EditorState): readonly Range[] {
  return provenanceKey.getState(state)?.ranges ?? [];
}

/** Replace the tracked ranges (used when a saved record is loaded). */
export function setAiRanges(view: EditorView, ranges: readonly Range[]): void {
  view.dispatch(view.state.tr.setMeta(provenanceKey, { ranges: normalize(ranges) }).setMeta("addToHistory", false));
}

export const Provenance = Extension.create<ProvenanceOptions>({
  name: "provenance",
  addOptions() {
    return { highlight: () => false };
  },
  addProseMirrorPlugins() {
    const opts = this.options;
    return [
      new Plugin<ProvState>({
        key: provenanceKey,
        state: {
          init: () => ({ ranges: [] }),
          apply(tr, st) {
            const set = tr.getMeta(provenanceKey) as ProvState | undefined;
            let ranges = set ? set.ranges : st.ranges;
            if (!set && tr.docChanged && ranges.length > 0) ranges = mapRanges(ranges, tr);
            const accepted = tr.getMeta(GHOST_ACCEPT) as AcceptMeta | undefined;
            if (accepted) ranges = normalize([...ranges, [accepted.from, accepted.to]]);
            return ranges === st.ranges ? st : { ranges };
          },
        },
        props: {
          decorations(state) {
            if (!opts.highlight()) return null;
            const ranges = provenanceKey.getState(state)?.ranges ?? [];
            if (ranges.length === 0) return null;
            const size = state.doc.content.size;
            return DecorationSet.create(
              state.doc,
              ranges.filter(([a, b]) => a >= 0 && b <= size).map(([a, b]) => Decoration.inline(a, b, { class: "ai-assisted" })),
            );
          },
        },
        view() {
          return {
            update(view, prev) {
              const now = provenanceKey.getState(view.state)?.ranges ?? [];
              const before = provenanceKey.getState(prev)?.ranges ?? [];
              const moved = view.state.doc !== prev.doc && now.length > 0;
              if (now !== before || moved) opts.onChange?.(now, view.state.doc);
            },
          };
        },
      }),
    ];
  },
});
