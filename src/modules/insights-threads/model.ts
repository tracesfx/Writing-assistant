/** Pure data shaping for Insights → Plot threads. */
import type { MetricPoint, ThreadStatus, ThreadView } from "@margin/sdk";

export interface Lane {
  id: string;
  title: string;
  status: ThreadStatus;
  canon: boolean;
  /** Scene positions (0-based, manuscript order). */
  from: number;
  to: number;
  introduced: number | null;
  resolved: number | null;
  touches: { order: number; sceneId: string; title: string }[];
}

const RANK: Record<ThreadStatus, number> = { open: 0, dropped: 1, resolved: 2 };

/**
 * One swimlane per thread: from where it is introduced to where it is
 * resolved. A thread still open runs to the last scene; a dropped one ends at
 * its last mention. Open threads come first.
 */
export function buildLanes(threads: ThreadView[], scenes: MetricPoint[], statuses: ThreadStatus[]): Lane[] {
  const last = Math.max(0, scenes.length - 1);
  const pos = new Map(scenes.map((s, i) => [s.sceneId, i]));
  const at = (r: { id: string; order: number } | null): number | null => (r ? (pos.get(r.id) ?? r.order) : null);
  const lanes: Lane[] = threads
    .filter((t) => statuses.includes(t.status))
    .map((t) => {
      const touches = t.touches
        .map((s) => ({ order: at(s) ?? 0, sceneId: s.id, title: s.title }))
        .sort((a, b) => a.order - b.order);
      const introduced = at(t.introducedIn) ?? touches[0]?.order ?? null;
      const resolved = at(t.resolvedIn);
      const lastTouch = touches.at(-1)?.order ?? introduced ?? 0;
      const from = Math.min(introduced ?? lastTouch, touches[0]?.order ?? Infinity);
      const to = t.status === "open" ? last : (resolved ?? lastTouch);
      return { id: t.id, title: t.title, status: t.status, canon: t.statusCanon, from, to: Math.max(from, to), introduced, resolved: t.status === "resolved" ? resolved : null, touches };
    });
  return lanes.sort((a, b) => RANK[a.status] - RANK[b.status] || a.from - b.from || a.title.localeCompare(b.title));
}

/** How many scenes the thread stays open for (null while it is still open). */
export function span(l: Lane): number | null {
  return l.status === "resolved" && l.resolved !== null && l.introduced !== null ? l.resolved - l.introduced + 1 : null;
}
