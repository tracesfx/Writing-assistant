import { describe, expect, it } from "vitest";

import type { ChapterGroup, EntitySummary, MentionMatrix } from "@margin/sdk";

import { buildPresence, typesIn, type PresenceOptions } from "./model";

const ent = (id: string, name: string, type = "character", aliases: string[] = []): EntitySummary =>
  ({ id, name, type, aliases, status: "inferred", createdBy: "ai", factCount: 0, inferredFactCount: 0, relationCount: 0, mentionCount: 0, updated: "" }) as EntitySummary;

const matrix: MentionMatrix = {
  entities: [ent("mara", "Mara"), ent("jon", "Jon"), ent("kessel", "Kessel", "location"), ent("why", "Why the harbour is closed", "thread"), ent("ghost", "Nobody")],
  scenes: [
    { id: "s1", title: "Arrival", order: 0 },
    { id: "s2", title: "The quay", order: 1 },
    { id: "s3", title: "Night", order: 2 },
  ],
  cells: [
    { entityId: "mara", sceneId: "s1", count: 3 },
    { entityId: "mara", sceneId: "s2", count: 2 },
    { entityId: "mara", sceneId: "s3", count: 1 },
    { entityId: "jon", sceneId: "s2", count: 5 },
    { entityId: "kessel", sceneId: "s1", count: 1 },
    { entityId: "why", sceneId: "s1", count: 1 },
  ],
};
const groups = new Map<string, ChapterGroup>([
  ["s1", { id: "c1", title: "One", order: 0 }],
  ["s2", { id: "c1", title: "One", order: 0 }],
  ["s3", { id: "c2", title: "Two", order: 1 }],
]);
const opts = (o: Partial<PresenceOptions> = {}): PresenceOptions => ({ by: "scene", types: [], sort: "mentions", minMentions: 1, limit: 0, query: "", ...o });

describe("buildPresence", () => {
  it("makes a column per scene and a row per entry, most mentioned first, without threads or unmentioned entries", () => {
    const p = buildPresence(matrix, groups, opts());
    expect(p.columns.map((c) => c.title)).toEqual(["Arrival", "The quay", "Night"]);
    expect(p.rows.map((r) => r.name)).toEqual(["Mara", "Jon", "Kessel"]);
    expect(p.cells).toContainEqual([1, 1, 5]); // Jon in "The quay"
    expect(p.sceneAt.get("1:1")).toBe("s2");
  });

  it("rolls scenes up into chapters, summing counts, and opens the first scene with a mention", () => {
    const p = buildPresence(matrix, groups, opts({ by: "chapter" }));
    expect(p.columns.map((c) => c.title)).toEqual(["One", "Two"]);
    const mara = p.rows.findIndex((r) => r.name === "Mara");
    expect(p.cells).toContainEqual([0, mara, 5]); // 3 + 2
    expect(p.sceneAt.get(`0:${mara}`)).toBe("s1");
    const jon = p.rows.findIndex((r) => r.name === "Jon");
    expect(p.sceneAt.get(`0:${jon}`)).toBe("s2");
  });

  it("filters by type, by search (names and aliases), and by minimum mentions", () => {
    expect(buildPresence(matrix, groups, opts({ types: ["location"] })).rows.map((r) => r.name)).toEqual(["Kessel"]);
    expect(buildPresence(matrix, groups, opts({ types: ["thread"] })).rows.map((r) => r.name)).toEqual(["Why the harbour is closed"]);
    expect(buildPresence(matrix, groups, opts({ minMentions: 5 })).rows.map((r) => r.name)).toEqual(["Mara", "Jon"]);
    const aliased: MentionMatrix = { ...matrix, entities: [ent("mara", "Mara Voss", "character", ["the pilot"]), ...matrix.entities.slice(1)] };
    expect(buildPresence(aliased, groups, opts({ query: "pilot" })).rows.map((r) => r.name)).toEqual(["Mara Voss"]);
  });

  it("sorts by first appearance or name, and reports what the limit hid", () => {
    expect(buildPresence(matrix, groups, opts({ sort: "first" })).rows.map((r) => r.name)).toEqual(["Kessel", "Mara", "Jon"]);
    expect(buildPresence(matrix, groups, opts({ sort: "name" })).rows.map((r) => r.name)).toEqual(["Jon", "Kessel", "Mara"]);
    const p = buildPresence(matrix, groups, opts({ limit: 2 }));
    expect(p.rows).toHaveLength(2);
    expect(p.hidden).toBe(1);
  });

  it("tells two entries with the same name apart", () => {
    const twin: MentionMatrix = {
      ...matrix,
      entities: [ent("a", "Rowan", "character"), ent("b", "Rowan", "location")],
      cells: [
        { entityId: "a", sceneId: "s1", count: 1 },
        { entityId: "b", sceneId: "s1", count: 1 },
      ],
    };
    expect(buildPresence(twin, groups, opts()).rows.map((r) => r.label).sort()).toEqual(["Rowan (character)", "Rowan (location)"]);
  });

  it("lists entry types without threads, most common first", () => {
    expect(typesIn(matrix)).toEqual(["character", "location"]);
  });
});
