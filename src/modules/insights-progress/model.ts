/** Pure data shaping for Insights → Writing progress (tested without a screen). */
import type { BinderNode, BinderTree, DayPoint, Goals, ProvenanceTotals } from "@margin/sdk";

export function isoDay(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** Parse `YYYY-MM-DD` as a local date at noon (daylight-saving safe). */
export function parseDay(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1, 12);
}

export function addDays(s: string, n: number): string {
  const d = parseDay(s);
  d.setDate(d.getDate() + n);
  return isoDay(d);
}

/** Every day from `from` to `to`, inclusive. */
export function dayRange(from: string, to: string): string[] {
  const out: string[] = [];
  for (let d = from; d <= to && out.length < 4000; d = addDays(d, 1)) out.push(d);
  return out;
}

export interface DayWords {
  date: string;
  words: number;
  minutes: number;
}

/** The last `count` days ending `today`, with idle days as zero. */
export function dailySeries(days: DayPoint[], today: string, count: number): DayWords[] {
  const by = new Map(days.map((d) => [d.date, d]));
  return dayRange(addDays(today, -(count - 1)), today).map((date) => {
    const d = by.get(date);
    return { date, words: d?.words ?? 0, minutes: Math.round((d?.activeSeconds ?? 0) / 60) };
  });
}

/** Consecutive days with words, ending today (or yesterday if today is empty). */
export function streakEndingNow(days: DayPoint[], today: string): number {
  const written = new Set(days.filter((d) => d.words > 0).map((d) => d.date));
  let day = written.has(today) ? today : addDays(today, -1);
  let n = 0;
  while (written.has(day)) {
    n++;
    day = addDays(day, -1);
  }
  return n;
}

export function longestStreak(days: DayPoint[]): number {
  const written = [...new Set(days.filter((d) => d.words > 0).map((d) => d.date))].sort();
  let best = 0;
  let run = 0;
  let prev = "";
  for (const d of written) {
    run = prev && addDays(prev, 1) === d ? run + 1 : 1;
    best = Math.max(best, run);
    prev = d;
  }
  return best;
}

export interface CumulativePoint {
  date: string;
  total: number;
}

/**
 * The manuscript's size at the end of each active day. The days only record
 * net change, so it is counted back from today's total.
 */
export function cumulative(days: DayPoint[], projectWords: number): CumulativePoint[] {
  const sorted = [...days].sort((a, b) => a.date.localeCompare(b.date));
  const out: CumulativePoint[] = [];
  let later = 0;
  for (let i = sorted.length - 1; i >= 0; i--) {
    out.unshift({ date: sorted[i]!.date, total: projectWords - later });
    later += sorted[i]!.words;
  }
  return out;
}

/** Straight line from the first day's total to the goal on the deadline. */
export function pace(first: CumulativePoint | undefined, goals: Goals): CumulativePoint[] {
  if (!first || !goals.projectWords || !goals.deadline || goals.deadline <= first.date) return [];
  return [first, { date: goals.deadline, total: goals.projectWords }];
}

export interface ChapterSplit {
  id: string;
  title: string;
  yours: number;
  ai: number;
}

/** Words per chapter, split into what the author wrote and what they accepted from suggestions. */
export function chapterSplit(tree: BinderTree | null, groupOf: Map<string, { id: string; title: string }>, prov: ProvenanceTotals | null): ChapterSplit[] {
  const ai = new Map((prov?.docs ?? []).map((d) => [d.docId, d.aiWords]));
  const rows = new Map<string, ChapterSplit>();
  const ms = tree?.sections.find((s) => s.section === "manuscript");
  const walk = (nodes: BinderNode[]) => {
    for (const n of nodes) {
      const g = groupOf.get(n.id);
      if (g && n.wordCount > 0) {
        const row = rows.get(g.id) ?? { id: g.id, title: g.title, yours: 0, ai: 0 };
        const a = Math.min(ai.get(n.id) ?? 0, n.wordCount);
        row.ai += a;
        row.yours += n.wordCount - a;
        rows.set(g.id, row);
      }
      walk(n.children);
    }
  };
  if (ms) walk(ms.nodes);
  return [...rows.values()];
}
