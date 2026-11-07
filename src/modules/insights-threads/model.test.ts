import { describe, expect, it } from "vitest";

import type { MetricPoint, SceneRef, ThreadStatus, ThreadView } from "@margin/sdk";

import { buildLanes, span } from "./model";

const scenes: MetricPoint[] = ["s1", "s2", "s3", "s4", "s5"].map((id, i) => ({ sceneId: id, title: `Scene ${i + 1}`, order: i, value: 100 }));
const ref = (id: string): SceneRef => ({ id, title: id, order: Number(id.slice(1)) - 1 });
const thread = (id: string, status: ThreadStatus, intro: string | null, touches: string[], resolved: string | null = null): ThreadView =>
  ({
    id,
    title: `Thread ${id}`,
    status,
    statusCanon: false,
    introducedIn: intro ? ref(intro) : null,
    touches: touches.map(ref),
    resolvedIn: resolved ? ref(resolved) : null,
    entryStatus: "inferred",
  }) as ThreadView;
const all: ThreadStatus[] = ["open", "resolved", "dropped"];

describe("buildLanes", () => {
  const threads = [
    thread("r", "resolved", "s1", ["s1", "s3"], "s4"),
    thread("o", "open", "s2", ["s2", "s5"]),
    thread("d", "dropped", "s1", ["s1", "s2"]),
  ];

  it("runs a resolved thread from introduction to resolution, an open one to the last scene, a dropped one to its last mention", () => {
    const lanes = buildLanes(threads, scenes, all);
    const by = (id: string) => lanes.find((l) => l.id === id)!;
    expect([by("r").from, by("r").to, by("r").resolved]).toEqual([0, 3, 3]);
    expect([by("o").from, by("o").to, by("o").resolved]).toEqual([1, 4, null]);
    expect([by("d").from, by("d").to]).toEqual([0, 1]);
  });

  it("lists open threads first", () => {
    expect(buildLanes(threads, scenes, all).map((l) => l.id)).toEqual(["o", "d", "r"]);
  });

  it("shows only the statuses asked for", () => {
    expect(buildLanes(threads, scenes, ["open"]).map((l) => l.id)).toEqual(["o"]);
    expect(buildLanes(threads, scenes, [])).toEqual([]);
  });

  it("copes with a thread that has no introduction recorded", () => {
    const [lane] = buildLanes([thread("x", "open", null, ["s3", "s2"])], scenes, all);
    expect(lane?.introduced).toBe(1);
    expect(lane?.from).toBe(1);
    expect(lane?.touches.map((t) => t.order)).toEqual([1, 2]);
  });

  it("measures how long a resolved thread stayed open", () => {
    const lanes = buildLanes(threads, scenes, all);
    expect(span(lanes.find((l) => l.id === "r")!)).toBe(4);
    expect(span(lanes.find((l) => l.id === "o")!)).toBeNull();
  });
});
