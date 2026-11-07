/** Pure data shaping for Insights → Relationship graph. */
import { CATEGORY_COLORS, type RelationGraph, type RelationView } from "@margin/sdk";

export interface GraphFilter {
  /** Relation kinds switched off. */
  hiddenKinds: string[];
  /** Show only relations first stated in chapters up to this rank (null = all). */
  asOf: number | null;
  /** Hide entries mentioned fewer times than this. */
  minMentions: number;
}

export interface VisibleNode {
  id: string;
  name: string;
  type: string;
  mentions: number;
}

export interface VisibleGraph {
  nodes: VisibleNode[];
  edges: RelationView[];
  /** All relation kinds in the graph, most used first (the legend). */
  kinds: { kind: string; count: number; color: string }[];
}

/** A stable colour per relation kind: the same kind keeps its colour as filters change. */
export function kindColors(kinds: string[]): Map<string, string> {
  const sorted = [...new Set(kinds)].sort();
  return new Map(sorted.map((k, i) => [k, CATEGORY_COLORS[i % CATEGORY_COLORS.length]!]));
}

/**
 * Apply the filters. `chapterRank` maps a document id to the rank of its
 * chapter (0 = first). A relation with no known passage is always shown.
 */
export function buildGraph(g: RelationGraph, chapterRank: Map<string, number>, f: GraphFilter): VisibleGraph {
  const colors = kindColors(g.edges.map((e) => e.kind));
  const counts = new Map<string, number>();
  for (const e of g.edges) counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
  const kinds = [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([kind, count]) => ({ kind, count, color: colors.get(kind)! }));

  const nodeById = new Map(g.nodes.map((n) => [n.id, n]));
  const edges = g.edges.filter((e) => {
    if (f.hiddenKinds.includes(e.kind)) return false;
    if (f.asOf !== null && e.source) {
      const rank = chapterRank.get(e.source.doc);
      if (rank !== undefined && rank > f.asOf) return false;
    }
    const a = nodeById.get(e.from);
    const b = nodeById.get(e.to);
    return !!a && !!b && a.mentions >= f.minMentions && b.mentions >= f.minMentions;
  });
  const used = new Set(edges.flatMap((e) => [e.from, e.to]));
  const nodes = g.nodes.filter((n) => used.has(n.id)).map((n) => ({ id: n.id, name: n.name, type: n.type, mentions: n.mentions }));
  return { nodes, edges, kinds };
}

/** Node diameter in pixels from its mention count (area grows with mentions). */
export function nodeSize(mentions: number, max: number): number {
  const t = max > 0 ? Math.sqrt(Math.max(0, mentions) / max) : 0;
  return Math.round(18 + t * 34);
}

export interface Placed {
  id: string;
  label: string;
  x: number;
  y: number;
  size: number;
}

export interface PlacedEdge {
  from: string;
  to: string;
  color: string;
  directed: boolean;
  label: string;
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** The graph as a standalone SVG document, from the positions on screen. */
export function graphSvg(nodes: Placed[], edges: PlacedEdge[], colors: { text: string; surface: string; node: string; border: string }): string {
  const pad = 40;
  const xs = nodes.map((n) => n.x);
  const ys = nodes.map((n) => n.y);
  const minX = Math.min(0, ...xs) - pad;
  const minY = Math.min(0, ...ys) - pad;
  const maxX = Math.max(1, ...xs) + pad;
  const maxY = Math.max(1, ...ys) + pad;
  const w = Math.round(maxX - minX);
  const h = Math.round(maxY - minY);
  const at = new Map(nodes.map((n) => [n.id, n]));
  const lines = edges
    .map((e) => {
      const a = at.get(e.from);
      const b = at.get(e.to);
      if (!a || !b) return "";
      return `<line x1="${a.x.toFixed(1)}" y1="${a.y.toFixed(1)}" x2="${b.x.toFixed(1)}" y2="${b.y.toFixed(1)}" stroke="${e.color}" stroke-width="2"><title>${esc(e.label)}</title></line>`;
    })
    .join("");
  const circles = nodes
    .map(
      (n) =>
        `<circle cx="${n.x.toFixed(1)}" cy="${n.y.toFixed(1)}" r="${(n.size / 2).toFixed(1)}" fill="${colors.node}" stroke="${colors.border}"/>` +
        `<text x="${n.x.toFixed(1)}" y="${(n.y + n.size / 2 + 13).toFixed(1)}" font-size="12" font-family="sans-serif" text-anchor="middle" fill="${colors.text}">${esc(n.label)}</text>`,
    )
    .join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${minX.toFixed(0)} ${minY.toFixed(0)} ${w} ${h}">` +
    `<rect x="${minX.toFixed(0)}" y="${minY.toFixed(0)}" width="${w}" height="${h}" fill="${colors.surface}"/>${lines}${circles}</svg>`
  );
}
