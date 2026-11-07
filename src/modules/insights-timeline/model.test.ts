import { describe, expect, it } from "vitest";

import type { EventView, SceneCardView } from "@margin/sdk";

import { bands, buildItems, effective, moveOrder, povColors } from "./model";

const ev = (id: string, scene: string | null, order: number | null = null, label = ""): EventView =>
  ({
    event: {
      id,
      title: `Event ${id}`,
      storyTime: order !== null || label ? { label, order } : null,
      narrativePos: scene,
      entities: [],
      source: null,
      status: "inferred",
      createdBy: "ai",
      updated: "",
    },
    entityNames: [],
    sceneTitle: scene ? `Scene ${scene}` : null,
  }) as EventView;
const card = (sceneId: string, pov: string | null): SceneCardView => ({ card: { sceneId, pov } as SceneCardView["card"], title: sceneId, order: 0 });

describe("buildItems", () => {
  it("puts events in story order by the author's number, else where the book tells them", () => {
    // Told: a, b, c, d. The author says c happens first (0.5) and a last (9).
    const items = buildItems([ev("a", "s1", 9), ev("b", "s1"), ev("c", "s2", 0.5), ev("d", "s2")], []);
    const by = (id: string) => items.find((i) => i.id === id)!;
    expect(items.map((i) => i.narrative)).toEqual([0, 1, 2, 3]);
    expect([by("c").story, by("b").story, by("d").story, by("a").story]).toEqual([0, 1, 2, 3]);
    expect(by("a").explicit).toBe(9);
    expect(by("b").explicit).toBeNull();
  });

  it("uses the scene card's point of view, and none when there is no card", () => {
    const items = buildItems([ev("a", "s1"), ev("b", "s2"), ev("c", null)], [card("s1", " Mara ")]);
    expect(items.map((i) => i.pov)).toEqual(["Mara", "", ""]);
  });

  it("breaks ties by narrative position so the order is stable", () => {
    const items = buildItems([ev("a", "s1", 2), ev("b", "s1", 2)], []);
    expect(items.map((i) => i.story)).toEqual([0, 1]);
  });
});

describe("moveOrder", () => {
  const items = buildItems([ev("a", "s1"), ev("b", "s1"), ev("c", "s2"), ev("d", "s2")], []);
  it("returns a number between the neighbours", () => {
    // Move c (3rd, order 3) earlier: between b (2) and a (1) -> 1.5.
    expect(moveOrder(items, "c", -1)).toBe(1.5);
    // Move b later: between c (3) and d (4) -> 3.5.
    expect(moveOrder(items, "b", 1)).toBe(3.5);
  });
  it("steps past the end of the list", () => {
    expect(moveOrder(items, "d", 1)).toBeNull();
    expect(moveOrder(items, "a", -1)).toBeNull();
    expect(moveOrder(items, "nope", 1)).toBeNull();
  });
  it("moving makes the event swap places", () => {
    const n = moveOrder(items, "c", -1)!;
    const moved = items.map((i) => (i.id === "c" ? { ...i, explicit: n } : i));
    const re = buildItems(
      moved.map((i) => ev(i.id, i.sceneId, i.explicit)),
      [],
    );
    expect(re.sort((a, b) => a.story - b.story).map((i) => i.id)).toEqual(["a", "c", "b", "d"]);
  });
  it("finds room when two events tie", () => {
    const tied = buildItems([ev("a", "s1", 5), ev("b", "s1", 5), ev("c", "s1", 7)], []);
    const n = moveOrder(tied, "c", -1)!;
    expect(n).toBeLessThan(7);
    expect(n).toBeGreaterThan(0);
  });
  it("effective order is the number or the narrative place", () => {
    expect(effective({ explicit: null, narrative: 4 })).toBe(5);
    expect(effective({ explicit: 0.25, narrative: 4 })).toBe(0.25);
  });
});

describe("bands and colours", () => {
  const items = buildItems([ev("a", "s1"), ev("b", "s1"), ev("c", "s2"), ev("d", "s1"), ev("e", null)], [card("s1", "Mara"), card("s2", "Jon")]);
  it("joins consecutive events of one scene into a band", () => {
    expect(bands(items)).toEqual([
      { sceneId: "s1", pov: "Mara", from: 0, to: 1 },
      { sceneId: "s2", pov: "Jon", from: 2, to: 2 },
      { sceneId: "s1", pov: "Mara", from: 3, to: 3 },
    ]);
  });
  it("gives each point of view its own colour", () => {
    const c = povColors(items);
    expect([...c.keys()]).toEqual(["Mara", "Jon"]);
    expect(c.get("Mara")).not.toBe(c.get("Jon"));
  });
});
