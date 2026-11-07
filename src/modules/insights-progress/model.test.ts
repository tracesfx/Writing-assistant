import { describe, expect, it } from "vitest";

import type { BinderNode, BinderTree, DayPoint } from "@margin/sdk";

import { addDays, chapterSplit, cumulative, dailySeries, dayRange, longestStreak, pace, streakEndingNow } from "./model";

const day = (date: string, words: number, activeSeconds = 0): DayPoint => ({ date, words, activeSeconds });

describe("days", () => {
  it("adds days across month, year and daylight-saving changes", () => {
    expect(addDays("2026-01-31", 1)).toBe("2026-02-01");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2026-03-08", 1)).toBe("2026-03-09"); // US spring forward
    expect(addDays("2026-11-01", 1)).toBe("2026-11-02"); // US fall back
    expect(dayRange("2026-02-27", "2026-03-02")).toHaveLength(4);
  });

  it("fills idle days with zero and keeps minutes", () => {
    const s = dailySeries([day("2026-11-04", 500, 1800), day("2026-11-06", -20)], "2026-11-06", 4);
    expect(s.map((d) => [d.date, d.words, d.minutes])).toEqual([
      ["2026-11-03", 0, 0],
      ["2026-11-04", 500, 30],
      ["2026-11-05", 0, 0],
      ["2026-11-06", -20, 0],
    ]);
  });
});

describe("streaks", () => {
  const days = [day("2026-11-01", 5), day("2026-11-02", 5), day("2026-11-03", 0), day("2026-11-05", 9), day("2026-11-06", 9)];
  it("counts consecutive days with words, ending today or yesterday", () => {
    expect(streakEndingNow(days, "2026-11-06")).toBe(2);
    expect(streakEndingNow(days, "2026-11-07")).toBe(2); // today still empty: yesterday counts
    expect(streakEndingNow(days, "2026-11-08")).toBe(0);
  });
  it("finds the longest run; a zero-word day breaks it", () => {
    expect(longestStreak(days)).toBe(2);
    expect(longestStreak([])).toBe(0);
  });
});

describe("growth", () => {
  it("counts the manuscript back from today's total", () => {
    const days = [day("2026-11-01", 100), day("2026-11-02", -10), day("2026-11-04", 50)];
    expect(cumulative(days, 1000)).toEqual([
      { date: "2026-11-01", total: 960 },
      { date: "2026-11-02", total: 950 },
      { date: "2026-11-04", total: 1000 },
    ]);
    expect(cumulative([], 5)).toEqual([]);
  });
  it("draws a pace line only with a goal and a later deadline", () => {
    const first = { date: "2026-11-01", total: 100 };
    expect(pace(first, { projectWords: 1000, deadline: "2026-12-01" })).toEqual([first, { date: "2026-12-01", total: 1000 }]);
    expect(pace(first, { projectWords: 1000 })).toEqual([]);
    expect(pace(first, { projectWords: 1000, deadline: "2026-10-01" })).toEqual([]);
    expect(pace(undefined, { projectWords: 1000, deadline: "2026-12-01" })).toEqual([]);
  });
});

describe("your words and AI words", () => {
  const node = (id: string, title: string, wordCount: number, kind: BinderNode["kind"] = "scene", children: BinderNode[] = []): BinderNode => ({
    id,
    kind,
    title,
    status: null,
    labels: [],
    target: null,
    include: true,
    ai: "inherit" as BinderNode["ai"],
    hasSynopsis: false,
    wordCount,
    totalWordCount: wordCount,
    children,
  });
  const tree = {
    sections: [
      {
        section: "manuscript",
        totalWordCount: 0,
        nodes: [node("c1", "One", 0, "chapter", [node("s1", "a", 100), node("s2", "b", 50)]), node("c2", "Two", 0, "chapter", [node("s3", "c", 30)])],
      },
    ],
    trash: [],
  } as unknown as BinderTree;
  const groupOf = new Map([
    ["s1", { id: "c1", title: "One" }],
    ["s2", { id: "c1", title: "One" }],
    ["s3", { id: "c2", title: "Two" }],
  ]);

  it("splits each chapter into words you wrote and words you accepted", () => {
    const out = chapterSplit(tree, groupOf, { projectAiWords: 40, docs: [{ docId: "s1", aiWords: 40 }] });
    expect(out).toEqual([
      { id: "c1", title: "One", yours: 110, ai: 40 },
      { id: "c2", title: "Two", yours: 30, ai: 0 },
    ]);
  });
  it("never shows more AI words than the scene has", () => {
    const out = chapterSplit(tree, groupOf, { projectAiWords: 999, docs: [{ docId: "s3", aiWords: 999 }] });
    expect(out.find((r) => r.id === "c2")).toEqual({ id: "c2", title: "Two", yours: 0, ai: 30 });
  });
  it("is empty without provenance data or text", () => {
    expect(chapterSplit(null, groupOf, null)).toEqual([]);
    expect(chapterSplit(tree, groupOf, null).every((r) => r.ai === 0)).toBe(true);
  });
});
