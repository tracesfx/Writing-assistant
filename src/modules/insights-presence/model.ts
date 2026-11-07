/** Pure data shaping for Insights → Character presence. */
import type { ChapterGroup, MentionMatrix } from "@margin/sdk";

export type By = "scene" | "chapter";
export type SortBy = "mentions" | "first" | "name";

export interface PresenceOptions {
  by: By;
  /** Entry types to show; empty = all (threads excluded unless listed). */
  types: string[];
  sort: SortBy;
  /** Hide entries mentioned fewer times than this in the whole book. */
  minMentions: number;
  /** Show at most this many entries (0 = all). */
  limit: number;
  query: string;
}

export interface Column {
  /** A scene id, or a chapter's group id. */
  id: string;
  title: string;
  /** The scene a click opens: the scene itself. */
  firstScene: string;
}

export interface Row {
  id: string;
  /** What the axis shows; made unique when two entries share a name. */
  label: string;
  name: string;
  type: string;
  total: number;
}

export interface Presence {
  columns: Column[];
  rows: Row[];
  /** `[column, row, count]` for every cell with a mention. */
  cells: [number, number, number][];
  /** For a cell: the scene to open (in chapter mode, the first one with a mention). */
  sceneAt: Map<string, string>;
  /** Entries hidden by the filters. */
  hidden: number;
}

export function buildPresence(m: MentionMatrix, groups: Map<string, ChapterGroup>, o: PresenceOptions): Presence {
  const sceneIndex = new Map(m.scenes.map((s, i) => [s.id, i]));
  // Columns, in manuscript order.
  const columns: Column[] = [];
  const colOf = new Map<string, number>();
  for (const s of m.scenes) {
    if (o.by === "scene") {
      colOf.set(s.id, columns.length);
      columns.push({ id: s.id, title: s.title || "Untitled", firstScene: s.id });
    } else {
      const g = groups.get(s.id);
      const key = g?.id ?? s.id;
      let c = colOf.get(key);
      if (c === undefined) {
        c = columns.length;
        colOf.set(key, c);
        columns.push({ id: key, title: g?.title ?? (s.title || "Untitled"), firstScene: s.id });
      }
      colOf.set(s.id, c);
    }
  }

  const q = o.query.trim().toLowerCase();
  const wanted = new Set(o.types);
  const byEntity = new Map<string, Map<number, { count: number; scene: string; order: number }>>();
  const totals = new Map<string, number>();
  const firstAt = new Map<string, number>();
  for (const cell of m.cells) {
    const col = colOf.get(cell.sceneId);
    if (col === undefined) continue;
    const order = sceneIndex.get(cell.sceneId) ?? 0;
    let cols = byEntity.get(cell.entityId);
    if (!cols) byEntity.set(cell.entityId, (cols = new Map()));
    const prev = cols.get(col);
    if (prev) {
      prev.count += cell.count;
      if (order < prev.order) {
        prev.scene = cell.sceneId;
        prev.order = order;
      }
    } else cols.set(col, { count: cell.count, scene: cell.sceneId, order });
    totals.set(cell.entityId, (totals.get(cell.entityId) ?? 0) + cell.count);
    firstAt.set(cell.entityId, Math.min(firstAt.get(cell.entityId) ?? Infinity, order));
  }

  let ents = m.entities.filter((e) => {
    if (!totals.has(e.id)) return false;
    if (wanted.size ? !wanted.has(e.type) : e.type === "thread") return false;
    if ((totals.get(e.id) ?? 0) < o.minMentions) return false;
    return !q || e.name.toLowerCase().includes(q) || e.aliases.some((a) => a.toLowerCase().includes(q));
  });
  const order = (a: (typeof ents)[number], b: (typeof ents)[number]) => {
    if (o.sort === "name") return a.name.localeCompare(b.name);
    if (o.sort === "first") return (firstAt.get(a.id) ?? 0) - (firstAt.get(b.id) ?? 0) || a.name.localeCompare(b.name);
    return (totals.get(b.id) ?? 0) - (totals.get(a.id) ?? 0) || a.name.localeCompare(b.name);
  };
  ents = [...ents].sort(order);
  const hidden = o.limit > 0 ? Math.max(0, ents.length - o.limit) : 0;
  if (o.limit > 0) ents = ents.slice(0, o.limit);

  const names = new Map<string, number>();
  for (const e of ents) names.set(e.name, (names.get(e.name) ?? 0) + 1);
  const rows: Row[] = ents.map((e) => ({
    id: e.id,
    name: e.name,
    type: e.type,
    total: totals.get(e.id) ?? 0,
    label: (names.get(e.name) ?? 0) > 1 ? `${e.name} (${e.type})` : e.name,
  }));

  const cells: [number, number, number][] = [];
  const sceneAt = new Map<string, string>();
  rows.forEach((r, y) => {
    for (const [x, c] of byEntity.get(r.id) ?? []) {
      cells.push([x, y, c.count]);
      sceneAt.set(`${x}:${y}`, c.scene);
    }
  });
  return { columns, rows, cells, sceneAt, hidden };
}

/** The entry types present in the matrix (threads left out), most common first. */
export function typesIn(m: MentionMatrix): string[] {
  const n = new Map<string, number>();
  for (const e of m.entities) if (e.type !== "thread") n.set(e.type, (n.get(e.type) ?? 0) + 1);
  return [...n.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([t]) => t);
}
