import { describe, expect, it } from "vitest";

import type { ChapterGroup, MetricPoint, SceneCardView } from "@margin/sdk";

import { UNKNOWN_POV, buildStructure, metricChoices } from "./model";

const pt = (sceneId: string, order: number, value: number | null): MetricPoint => ({ sceneId, title: `T-${sceneId}`, order, value });
const card = (sceneId: string, pov: string | null): SceneCardView => ({ card: { sceneId, pov } as SceneCardView["card"], title: "", order: 0 });
const groups = new Map<string, ChapterGroup>([
  ["s1", { id: "c1", title: "One", order: 0 }],
  ["s2", { id: "c1", title: "One", order: 0 }],
  ["s3", { id: "c2", title: "Two", order: 1 }],
]);
const words = [pt("s1", 0, 100), pt("s2", 1, 300), pt("s3", 2, 50)];
const cards = [card("s1", "Mara"), card("s2", "Jon"), card("s3", "Mara")];

describe("buildStructure", () => {
  it("stacks words per chapter by point of view", () => {
    const s = buildStructure(words, cards, groups, "chapter", null);
    expect(s.categories.map((c) => [c.title, c.sceneIds])).toEqual([["One", ["s1", "s2"]], ["Two", ["s3"]]]);
    expect(s.povs).toEqual(["Mara", "Jon"]);
    expect(s.words.get("Mara")).toEqual([100, 50]);
    expect(s.words.get("Jon")).toEqual([300, 0]);
    expect(s.categories[0]!.firstScene).toBe("s1");
  });

  it("shows scenes one by one when asked", () => {
    const s = buildStructure(words, cards, groups, "scene", null);
    expect(s.categories.map((c) => c.id)).toEqual(["s1", "s2", "s3"]);
    expect(s.words.get("Mara")).toEqual([100, 0, 50]);
  });

  it("files scenes without a card or point of view under one grey group, last", () => {
    const s = buildStructure(words, [card("s2", "Jon"), card("s3", "  ")], groups, "chapter", null);
    expect(s.povs).toEqual(["Jon", UNKNOWN_POV]);
    expect(s.words.get(UNKNOWN_POV)).toEqual([100, 50]);
  });

  it("averages a metric over a chapter's scenes, weighted by words, and leaves gaps empty", () => {
    const tension = [pt("s1", 0, 2), pt("s2", 1, 6)]; // s3 not measured yet
    const s = buildStructure(words, cards, groups, "chapter", tension);
    expect(s.metric[0]).toBe(5); // (2*100 + 6*300) / 400
    expect(s.metric[1]).toBeNull();
  });

  it("copes with no scenes", () => {
    const s = buildStructure([], [], groups, "chapter", null);
    expect(s.categories).toEqual([]);
    expect(s.povs).toEqual([]);
  });
});

describe("metricChoices", () => {
  it("offers the built-ins and every extractor's metric", () => {
    const c = metricChoices([{ metricKey: "tension", title: "Tension (0–10)", max: 10 }]);
    expect(c.map((x) => x.key)).toEqual(["dialogueRatio", "entityCount", "tension"]);
    expect(c.at(-1)?.max).toBe(10);
  });
});
