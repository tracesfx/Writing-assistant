/** Pure data shaping for Insights → Timeline. */
import { CATEGORY_COLORS, type EventView, type SceneCardView } from "@margin/sdk";

export interface Item {
  id: string;
  title: string;
  sceneId: string | null;
  sceneTitle: string;
  /** The story-time label the reading gave ("that night"). */
  when: string;
  entities: string[];
  /** The author's number, or null when the event sits where the book tells it. */
  explicit: number | null;
  /** Where the book tells it, 0-based. */
  narrative: number;
  /** Where it happens, 0-based (ties broken by narrative position). */
  story: number;
  pov: string;
  canon: boolean;
}

/** The order an event is sorted by in story time: the author's number, else its narrative place (1 = first). */
export const effective = (e: { explicit: number | null; narrative: number }): number => e.explicit ?? e.narrative + 1;

export function buildItems(events: EventView[], cards: SceneCardView[]): Item[] {
  const povOf = new Map(cards.map((c) => [c.card.sceneId, c.card.pov?.trim() ?? ""]));
  const items: Item[] = events.map((v, i) => ({
    id: v.event.id,
    title: v.event.title,
    sceneId: v.event.narrativePos,
    sceneTitle: v.sceneTitle ?? "",
    when: v.event.storyTime?.label ?? "",
    entities: v.entityNames,
    explicit: v.event.storyTime?.order ?? null,
    narrative: i,
    story: i,
    pov: (v.event.narrativePos && povOf.get(v.event.narrativePos)) || "",
    canon: v.event.status === "canon",
  }));
  [...items]
    .sort((a, b) => effective(a) - effective(b) || a.narrative - b.narrative)
    .forEach((it, rank) => {
      it.story = rank;
    });
  return items;
}

/**
 * The number that moves `id` one place earlier (`-1`) or later (`1`) in story
 * order, between its new neighbours. Null when it is already at the end.
 */
export function moveOrder(items: Item[], id: string, dir: -1 | 1): number | null {
  const byStory = [...items].sort((a, b) => a.story - b.story);
  const i = byStory.findIndex((x) => x.id === id);
  const near = byStory[i + dir];
  if (i < 0 || !near) return null;
  const far = byStory[i + 2 * dir];
  const a = effective(near);
  const b = far ? effective(far) : a + dir;
  // Between the neighbour and the one beyond it; a tie has no room, so step just past the neighbour.
  const mid = (a + b) / 2;
  return mid === a ? a + dir * 0.001 : Math.round(mid * 1000) / 1000;
}

/** A stable colour per point-of-view character, in order of first appearance. */
export function povColors(items: Item[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const it of items) {
    const key = it.pov || "";
    if (key && !out.has(key)) out.set(key, CATEGORY_COLORS[out.size % CATEGORY_COLORS.length]!);
  }
  return out;
}

export interface Band {
  sceneId: string;
  pov: string;
  from: number;
  to: number;
}

/** Runs of events from the same scene, for the point-of-view bands behind the narrative lane. */
export function bands(items: Item[]): Band[] {
  const out: Band[] = [];
  for (const it of [...items].sort((a, b) => a.narrative - b.narrative)) {
    if (!it.sceneId) continue;
    const last = out.at(-1);
    if (last && last.sceneId === it.sceneId) last.to = it.narrative;
    else out.push({ sceneId: it.sceneId, pov: it.pov, from: it.narrative, to: it.narrative });
  }
  return out;
}
