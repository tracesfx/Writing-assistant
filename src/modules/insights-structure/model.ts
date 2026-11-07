/** Pure data shaping for Insights → Structure overview. */
import type { ChapterGroup, MetricPoint, SceneCardView } from "@margin/sdk";

export type Rollup = "chapter" | "scene";

export const UNKNOWN_POV = "No point of view";

export interface Category {
  id: string;
  title: string;
  /** The scene a click opens. */
  firstScene: string;
  sceneIds: string[];
}

export interface Structure {
  categories: Category[];
  /** Point-of-view names in order of first appearance. */
  povs: string[];
  /** `words[pov][category]` */
  words: Map<string, number[]>;
  /** The metric per category (word-weighted mean over its scenes), null where unknown. */
  metric: (number | null)[];
}

const clean = (s: string | null | undefined) => (s ?? "").trim();

export function buildStructure(
  words: MetricPoint[],
  cards: SceneCardView[],
  groups: Map<string, ChapterGroup>,
  by: Rollup,
  metric: MetricPoint[] | null,
): Structure {
  const cardOf = new Map(cards.map((c) => [c.card.sceneId, c.card]));
  const metricOf = new Map((metric ?? []).map((m) => [m.sceneId, m.value]));
  const categories: Category[] = [];
  const catOf = new Map<string, number>();
  for (const w of words) {
    const g = by === "chapter" ? groups.get(w.sceneId) : undefined;
    const key = g?.id ?? w.sceneId;
    let i = catOf.get(key);
    if (i === undefined) {
      i = categories.length;
      catOf.set(key, i);
      categories.push({ id: key, title: (g?.title ?? w.title) || "Untitled", firstScene: w.sceneId, sceneIds: [] });
    }
    categories[i]!.sceneIds.push(w.sceneId);
  }

  const povs: string[] = [];
  const grid = new Map<string, number[]>();
  const metricSum: number[] = categories.map(() => 0);
  const metricWeight: number[] = categories.map(() => 0);
  for (const w of words) {
    const i = catOf.get((by === "chapter" ? groups.get(w.sceneId)?.id : undefined) ?? w.sceneId)!;
    const pov = clean(cardOf.get(w.sceneId)?.pov) || UNKNOWN_POV;
    if (!grid.has(pov)) {
      povs.push(pov);
      grid.set(pov, categories.map(() => 0));
    }
    const n = Math.max(0, w.value ?? 0);
    grid.get(pov)![i]! += n;
    const m = metricOf.get(w.sceneId);
    if (m !== undefined && m !== null && n > 0) {
      metricSum[i]! += m * n;
      metricWeight[i]! += n;
    }
  }
  // "No point of view" last, so named characters take the first colours.
  povs.sort((a, b) => (a === UNKNOWN_POV ? 1 : b === UNKNOWN_POV ? -1 : 0));
  return {
    categories,
    povs,
    words: grid,
    metric: categories.map((_, i) => (metricWeight[i]! > 0 ? Math.round((metricSum[i]! / metricWeight[i]!) * 100) / 100 : null)),
  };
}

export interface MetricChoice {
  key: string;
  title: string;
  max: number | null;
}

/** The numbers a scene can be charted by: built in, plus what extractors report. */
export function metricChoices(extractors: { metricKey: string; title: string; max: number }[]): MetricChoice[] {
  return [
    { key: "dialogueRatio", title: "Share of dialogue", max: 1 },
    { key: "entityCount", title: "Entries present", max: null },
    ...extractors.map((x) => ({ key: x.metricKey, title: x.title, max: x.max })),
  ];
}
