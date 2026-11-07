/**
 * Insights → Relationship graph (SPEC §10): a Cytoscape network of the bible's
 * relations. Edge colour is the relation type, node size the mention count.
 * Filter by type, and by "as of chapter N". Tap a name for its entry; tap a
 * relation to read the passage it comes from.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import {
  BIBLE_ENTRY_VIEW,
  ExportBar,
  InsightEmpty,
  InsightError,
  InsightHeader,
  InsightPanel,
  chapterGroups,
  flattenTree,
  memory,
  openSource,
  toCsv,
  useBinderTree,
  useChartColors,
  useInsightsData,
  workbench,
  type ChartExport,
  type InsightsViewProps,
  type RelationView,
} from "@margin/sdk";

import { buildGraph, graphSvg, kindColors, nodeSize, type VisibleGraph } from "./model";

type Cy = import("cytoscape").Core;

type Selection = { kind: "node"; id: string } | { kind: "edge"; id: string } | null;

export function GraphView({ active }: InsightsViewProps) {
  const { data, error } = useInsightsData(() => memory.relations.graph(), active);
  const tree = useBinderTree();
  const colors = useChartColors();
  const host = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Cy | null>(null);
  const exportRef = useRef<ChartExport | null>(null);
  const [hidden, setHidden] = useState<string[]>([]);
  const [asOf, setAsOf] = useState<number | null>(null);
  const [minMentions, setMin] = useState(0);
  const [sel, setSel] = useState<Selection>(null);

  // Chapters in binder order that hold text, and the chapter rank of every document.
  const { chapters, rankOf } = useMemo(() => {
    const groups = chapterGroups(tree);
    const rank = new Map<string, number>();
    const list: { id: string; title: string }[] = [];
    const seen = new Map<string, number>();
    for (const f of flattenTree(tree)) {
      if (f.section !== "manuscript") continue;
      const g = groups.get(f.node.id);
      if (!g || f.node.wordCount <= 0) continue;
      if (!seen.has(g.id)) {
        seen.set(g.id, list.length);
        list.push({ id: g.id, title: g.title });
      }
      rank.set(f.node.id, seen.get(g.id)!);
    }
    return { chapters: list, rankOf: rank };
  }, [tree]);

  const graph: VisibleGraph | null = useMemo(
    () => (data ? buildGraph(data, rankOf, { hiddenKinds: hidden, asOf, minMentions }) : null),
    [data, rankOf, hidden, asOf, minMentions],
  );
  const edgeById = useMemo(() => new Map((graph?.edges ?? []).map((e) => [e.id, e])), [graph]);
  const nameOf = useMemo(() => new Map((data?.nodes ?? []).map((n) => [n.id, n])), [data]);
  const allKinds = useMemo(() => (data ? kindColors(data.edges.map((e) => e.kind)) : new Map<string, string>()), [data]);

  // Build / update the Cytoscape network when the data or the filters change.
  useEffect(() => {
    if (!graph || !host.current || !active) return;
    let cancelled = false;
    void import("cytoscape").then(({ default: cytoscape }) => {
      const el = host.current;
      if (cancelled || !el) return;
      cyRef.current?.destroy();
      const max = Math.max(1, ...graph.nodes.map((n) => n.mentions));
      const cy = cytoscape({
        container: el,
        elements: [
          ...graph.nodes.map((n) => ({ data: { id: n.id, label: n.name, size: nodeSize(n.mentions, max), type: n.type, mentions: n.mentions } })),
          ...graph.edges.map((e) => ({ data: { id: e.id, source: e.from, target: e.to, kind: e.kind, color: allKinds.get(e.kind) ?? colors.muted, directed: e.directed ? 1 : 0 } })),
        ],
        layout: { name: "cose", animate: false, randomize: true, numIter: 250, coolingFactor: 0.95, nodeDimensionsIncludeLabels: false, idealEdgeLength: () => 90, nodeRepulsion: () => 9000, padding: 24 },
        wheelSensitivity: 0.3,
        minZoom: 0.2,
        maxZoom: 3,
        style: [
          {
            selector: "node",
            style: {
              width: "data(size)",
              height: "data(size)",
              "background-color": colors.accent,
              "border-width": 1,
              "border-color": colors.border,
              label: "data(label)",
              color: colors.text,
              "font-size": 11,
              "text-valign": "bottom",
              "text-margin-y": 4,
              "text-outline-color": colors.surface,
              "text-outline-width": 2,
            },
          },
          {
            selector: "edge",
            style: {
              width: 2.5,
              "line-color": "data(color)",
              "target-arrow-color": "data(color)",
              "target-arrow-shape": "triangle",
              "curve-style": "bezier",
              opacity: 0.85,
            },
          },
          { selector: "edge[directed = 0]", style: { "target-arrow-shape": "none" } },
          { selector: ":selected", style: { "border-width": 3, "border-color": colors.text, width: 4 } },
        ],
      });
      // A graph with a few relations would otherwise fill the frame with huge circles.
      if (cy.zoom() > 1.2) {
        cy.zoom(1.2);
        cy.center();
      }
      cy.on("tap", "node", (ev) => setSel({ kind: "node", id: String(ev.target.id()) }));
      cy.on("tap", "edge", (ev) => setSel({ kind: "edge", id: String(ev.target.id()) }));
      cy.on("tap", (ev) => {
        if (ev.target === cy) setSel(null);
      });
      cyRef.current = cy;
      (el as HTMLDivElement & { __marginExport?: { current: ChartExport | null } }).__marginExport = exportRef;
      // A handle for the app-driving tests (CDP), which need pixel positions of nodes.
      (el as HTMLDivElement & { __marginCy?: Cy }).__marginCy = cy;
      exportRef.current = {
        png: async () => {
          const url = cy.png({ full: true, scale: 2, bg: colors.surface, output: "base64uri" });
          const bin = atob(url.slice(url.indexOf(",") + 1));
          return Uint8Array.from(bin, (c) => c.charCodeAt(0));
        },
        svg: async () =>
          graphSvg(
            cy.nodes().map((n) => ({ id: n.id(), label: String(n.data("label")), x: n.position("x"), y: n.position("y"), size: Number(n.data("size")) })),
            cy.edges().map((e) => ({
              from: e.source().id(),
              to: e.target().id(),
              color: String(e.data("color")),
              directed: e.data("directed") === 1,
              label: `${e.source().data("label")} ${e.data("kind")} ${e.target().data("label")}`,
            })),
            { text: colors.text, surface: colors.surface, node: colors.accent, border: colors.border },
          ),
      };
    });
    return () => {
      cancelled = true;
    };
  }, [graph, allKinds, colors, active]);

  useEffect(
    () => () => {
      cyRef.current?.destroy();
      cyRef.current = null;
      exportRef.current = null;
    },
    [],
  );

  if (error) return <InsightError message={error} />;
  const empty = data && data.edges.length === 0;
  const edge = sel?.kind === "edge" ? edgeById.get(sel.id) : undefined;
  const node = sel?.kind === "node" ? nameOf.get(sel.id) : undefined;
  const toggleKind = (k: string) => setHidden((cur) => (cur.includes(k) ? cur.filter((x) => x !== k) : [...cur, k]));

  return (
    <div className="insight">
      <InsightHeader title="Relationship graph" intro="How the people, places and groups in your bible connect. Bigger circles are mentioned more.">
        {chapters.length > 1 && (
          <label className="asof">
            As of{" "}
            <input
              type="range"
              min={0}
              max={chapters.length - 1}
              value={asOf ?? chapters.length - 1}
              onChange={(e) => setAsOf(Number(e.target.value) >= chapters.length - 1 ? null : Number(e.target.value))}
              aria-label="Show relations up to this chapter"
            />{" "}
            <span className="asof-label">{asOf === null ? "the end" : (chapters[asOf]?.title ?? "")}</span>
          </label>
        )}
      </InsightHeader>

      {empty ? (
        <InsightEmpty what="No relationships in the bible yet." />
      ) : (
        <>
          <div className="insight-filters">
            <div className="chips" role="group" aria-label="Relation types">
              {(graph?.kinds ?? []).map((k) => (
                <button key={k.kind} className={hidden.includes(k.kind) ? "chip" : "chip active"} aria-pressed={!hidden.includes(k.kind)} onClick={() => toggleKind(k.kind)}>
                  <span className="swatch" style={{ background: k.color }} aria-hidden="true" /> {k.kind} <span className="muted">{k.count}</span>
                </button>
              ))}
            </div>
            <label>
              At least{" "}
              <input type="number" min={0} value={minMentions} onChange={(e) => setMin(Math.max(0, Number(e.target.value) || 0))} style={{ width: "4em" }} /> mentions
            </label>
          </div>
          <InsightPanel>
            <div ref={host} className="insight-graph" role="img" aria-label={`Network of ${graph?.nodes.length ?? 0} bible entries and ${graph?.edges.length ?? 0} relations`} />
            <div className="insight-detail" aria-live="polite">
              {node && (
                <>
                  <strong>{node.name}</strong> <span className="muted">{node.type} · {node.mentions} mentions</span>{" "}
                  <button onClick={() => workbench.openView(BIBLE_ENTRY_VIEW, node.id)}>Open entry</button>
                </>
              )}
              {edge && <EdgeDetail edge={edge} />}
              {!node && !edge && <span className="muted">Tap a circle for its entry, or a line to read where the book says it.</span>}
            </div>
            <ExportBar
              name="relationship graph"
              chart={exportRef}
              csv={() =>
                toCsv(
                  ["from", "relation", "to", "directed", "status", "since", "passage"],
                  (graph?.edges ?? []).map((e) => [e.fromName, e.kind, e.toName, e.directed, e.status, e.since, e.source?.quote]),
                )
              }
            />
          </InsightPanel>
        </>
      )}
    </div>
  );
}

function EdgeDetail({ edge }: { edge: RelationView }) {
  return (
    <>
      <strong>
        {edge.fromName} {edge.directed ? "→" : "–"} {edge.toName}
      </strong>{" "}
      <span className="muted">{edge.kind}{edge.status === "canon" ? " · canon" : " · inferred"}</span>
      {edge.source && (
        <>
          {" "}
          <em>“{edge.source.quote}”</em>{" "}
          <button onClick={() => void openSource(edge.source!)}>Read the passage</button>
        </>
      )}
    </>
  );
}
