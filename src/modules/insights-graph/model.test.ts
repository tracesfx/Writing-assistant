import { describe, expect, it } from "vitest";

import type { RelationGraph, RelationView } from "@margin/sdk";

import { buildGraph, graphSvg, kindColors, nodeSize } from "./model";

const edge = (id: string, from: string, to: string, kind: string, doc: string | null = null): RelationView =>
  ({
    id,
    from,
    fromName: from,
    to,
    toName: to,
    kind,
    directed: false,
    status: "inferred",
    source: doc ? { doc, quote: "q" } : null,
    since: null,
    createdBy: "ai",
  }) as RelationView;

const graph: RelationGraph = {
  nodes: [
    { id: "a", name: "A", type: "character", mentions: 10 },
    { id: "b", name: "B", type: "character", mentions: 4 },
    { id: "c", name: "C", type: "character", mentions: 1 },
    { id: "d", name: "D", type: "location", mentions: 6 },
  ],
  edges: [edge("1", "a", "b", "sibling", "s1"), edge("2", "a", "c", "ally", "s3"), edge("3", "b", "d", "located-in", null), edge("4", "a", "d", "sibling", "s2")],
};
const rank = new Map([
  ["s1", 0],
  ["s2", 1],
  ["s3", 2],
]);
const all = { hiddenKinds: [], asOf: null, minMentions: 0 };

describe("buildGraph", () => {
  it("keeps entries that have a relation, and lists the kinds most used first", () => {
    const g = buildGraph(graph, rank, all);
    expect(g.nodes.map((n) => n.id).sort()).toEqual(["a", "b", "c", "d"]);
    expect(g.kinds.map((k) => [k.kind, k.count])).toEqual([["sibling", 2], ["ally", 1], ["located-in", 1]]);
  });

  it("hides relation kinds that are switched off, and entries left with none", () => {
    const g = buildGraph(graph, rank, { ...all, hiddenKinds: ["ally"] });
    expect(g.edges.map((e) => e.id)).toEqual(["1", "3", "4"]);
    expect(g.nodes.map((n) => n.id)).not.toContain("c");
    // The legend still lists the hidden kind, so it can be switched back on.
    expect(g.kinds.map((k) => k.kind)).toContain("ally");
  });

  it("shows the book as of a chapter; relations with no passage are always shown", () => {
    const g = buildGraph(graph, rank, { ...all, asOf: 0 });
    expect(g.edges.map((e) => e.id)).toEqual(["1", "3"]);
    expect(buildGraph(graph, rank, { ...all, asOf: 1 }).edges.map((e) => e.id)).toEqual(["1", "3", "4"]);
    expect(buildGraph(graph, rank, { ...all, asOf: 2 }).edges).toHaveLength(4);
  });

  it("drops entries mentioned too rarely, with their relations", () => {
    const g = buildGraph(graph, rank, { ...all, minMentions: 4 });
    expect(g.edges.map((e) => e.id)).toEqual(["1", "3", "4"]);
  });

  it("gives a kind the same colour whatever the filter", () => {
    const c = kindColors(["sibling", "ally", "sibling"]);
    expect(c.get("ally")).not.toBe(c.get("sibling"));
    expect(buildGraph(graph, rank, all).kinds.find((k) => k.kind === "ally")?.color).toBe(buildGraph(graph, rank, { ...all, hiddenKinds: ["sibling"] }).kinds.find((k) => k.kind === "ally")?.color);
  });
});

describe("nodeSize", () => {
  it("grows with mentions, within bounds", () => {
    expect(nodeSize(0, 10)).toBe(18);
    expect(nodeSize(10, 10)).toBe(52);
    expect(nodeSize(5, 10)).toBeGreaterThan(nodeSize(1, 10));
    expect(nodeSize(3, 0)).toBe(18);
  });
});

describe("graphSvg", () => {
  it("writes a standalone SVG and escapes names", () => {
    const svg = graphSvg(
      [
        { id: "a", label: "Tom & <Ann>", x: 0, y: 0, size: 30 },
        { id: "b", label: "B", x: 100, y: 50, size: 20 },
      ],
      [{ from: "a", to: "b", color: "#0072B2", directed: false, label: "a sibling b" }],
      { text: "#000", surface: "#fff", node: "#a60", border: "#ccc" },
    );
    expect(svg.startsWith("<svg xmlns=")).toBe(true);
    expect(svg).toContain("Tom &amp; &lt;Ann&gt;");
    expect(svg).not.toContain("<Ann>");
    expect(svg.match(/<circle/g)).toHaveLength(2);
    expect(svg.match(/<line/g)).toHaveLength(1);
  });
});
