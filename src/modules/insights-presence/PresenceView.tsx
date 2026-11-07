/**
 * Insights → Character presence (SPEC §10): a heatmap of entries by scene or
 * chapter, built from `mentions.matrix`. Click a cell to open the first
 * mention in that scene with the words selected; click a name for its entry.
 */
import { useMemo, useRef, useState } from "react";

import {
  BIBLE_ENTRY_VIEW,
  Chart,
  ExportBar,
  InsightEmpty,
  InsightError,
  InsightHeader,
  InsightPanel,
  chapterGroups,
  heatLevels,
  heatPieces,
  memory,
  openSource,
  toCsv,
  useBinderTree,
  useChartColors,
  useInsightsData,
  workbench,
  type ChartExport,
  type EChartsOption,
  type InsightsViewProps,
} from "@margin/sdk";

import { buildPresence, typesIn, type By, type PresenceOptions, type SortBy } from "./model";

const LIMITS = [25, 50, 0] as const;

export function PresenceView({ active }: InsightsViewProps) {
  const { data: matrix, error } = useInsightsData(() => memory.mentions.matrix([]), active);
  const tree = useBinderTree();
  const colors = useChartColors();
  const chartRef = useRef<ChartExport | null>(null);
  const [by, setBy] = useState<By>("scene");
  const [types, setTypes] = useState<string[]>([]);
  const [sort, setSort] = useState<SortBy>("mentions");
  const [limit, setLimit] = useState<number>(25);
  const [minMentions, setMin] = useState(1);
  const [query, setQuery] = useState("");

  const groups = useMemo(() => chapterGroups(tree), [tree]);
  const opts: PresenceOptions = { by, types, sort, limit, minMentions, query };
  const presence = useMemo(() => (matrix ? buildPresence(matrix, groups, opts) : null), [matrix, groups, by, types, sort, limit, minMentions, query]); // eslint-disable-line react-hooks/exhaustive-deps
  const typeList = useMemo(() => (matrix ? typesIn(matrix) : []), [matrix]);

  const option = useMemo<EChartsOption>(() => {
    if (!presence) return {};
    const cuts = heatLevels(presence.cells.map((c) => c[2]));
    return {
      grid: { left: 150, right: 24, top: 8, bottom: 96 },
      tooltip: {
        formatter: (p) => {
          const [x, y, n] = (p as unknown as { data: [number, number, number] }).data;
          return `${presence.rows[y]?.label}<br/>${presence.columns[x]?.title}: ${n} mention${n === 1 ? "" : "s"}`;
        },
      },
      xAxis: {
        type: "category",
        data: presence.columns.map((c) => c.title),
        splitArea: { show: true, areaStyle: { color: [colors.heat[0] ?? colors.border] } },
        axisLabel: { rotate: 45, hideOverlap: true, interval: 0, width: 110, overflow: "truncate" },
      },
      yAxis: {
        type: "category",
        inverse: true,
        data: presence.rows.map((r) => r.label),
        triggerEvent: true,
        axisLabel: { width: 130, overflow: "truncate", interval: 0 },
      },
      visualMap: { show: false, type: "piecewise", pieces: heatPieces(colors, cuts), outOfRange: { color: colors.heat[0] } },
      series: [{ type: "heatmap", data: presence.cells, itemStyle: { borderColor: colors.surface, borderWidth: 1 }, emphasis: { itemStyle: { borderColor: colors.text } } }],
    };
  }, [presence, colors]);

  const openCell = async (x: number, y: number) => {
    const row = presence?.rows[y];
    const scene = presence?.sceneAt.get(`${x}:${y}`);
    if (!row || !scene) return;
    try {
      const hit = (await memory.mentions.byEntity(row.id)).find((m) => m.sceneId === scene);
      if (hit?.first) await openSource(hit.first);
      else workbench.openDocument(scene);
    } catch {
      workbench.openDocument(scene);
    }
  };

  if (error) return <InsightError message={error} />;
  const empty = matrix && presence && matrix.cells.length === 0;
  const toggle = (t: string) => setTypes((cur) => (cur.includes(t) ? cur.filter((x) => x !== t) : [...cur, t]));

  return (
    <div className="insight">
      <InsightHeader title="Character presence" intro="Who and what appears where. Counts come from the names and aliases in your text.">
        <div className="segmented" role="group" aria-label="Columns">
          {(["scene", "chapter"] as const).map((b) => (
            <button key={b} className={b === by ? "active" : ""} aria-pressed={b === by} onClick={() => setBy(b)}>
              By {b}
            </button>
          ))}
        </div>
      </InsightHeader>

      {empty ? (
        <InsightEmpty what="No bible entries are mentioned in the text yet." />
      ) : (
        <>
          <div className="insight-filters">
            <input type="search" placeholder="Find an entry" aria-label="Find an entry" value={query} onChange={(e) => setQuery(e.target.value)} />
            <label>
              Sort{" "}
              <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)}>
                <option value="mentions">Most mentioned</option>
                <option value="first">First appearance</option>
                <option value="name">Name</option>
              </select>
            </label>
            <label>
              Show{" "}
              <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
                {LIMITS.map((l) => (
                  <option key={l} value={l}>
                    {l === 0 ? "all" : `top ${l}`}
                  </option>
                ))}
              </select>
            </label>
            <label>
              At least{" "}
              <input type="number" min={1} value={minMentions} onChange={(e) => setMin(Math.max(1, Number(e.target.value) || 1))} style={{ width: "4em" }} /> mentions
            </label>
            <div className="chips" role="group" aria-label="Types">
              {typeList.map((t) => (
                <button key={t} className={types.includes(t) ? "chip active" : "chip"} aria-pressed={types.includes(t)} onClick={() => toggle(t)}>
                  {t}
                </button>
              ))}
            </div>
          </div>
          <InsightPanel note={presence && presence.hidden > 0 ? `${presence.hidden} more entries hidden. Show all, or filter by type.` : "Click a cell to read the first mention. Click a name to open its entry."}>
            <Chart
              option={option}
              height={Math.max(220, 140 + (presence?.rows.length ?? 0) * 20)}
              label="Heatmap of how often each bible entry is mentioned in each scene or chapter"
              exportRef={chartRef}
              onClick={(p) => {
                if (p.componentType === "yAxis") {
                  const row = presence?.rows.find((r) => r.label === p.name);
                  if (row) workbench.openView(BIBLE_ENTRY_VIEW, row.id);
                  return;
                }
                const d = p.data as [number, number, number] | undefined;
                if (d) void openCell(d[0], d[1]);
              }}
            />
            <ExportBar
              name="character presence"
              chart={chartRef}
              csv={() =>
                toCsv(
                  ["entry", "type", by === "scene" ? "scene" : "chapter", "mentions"],
                  (presence?.cells ?? []).map(([x, y, n]) => [presence?.rows[y]?.name, presence?.rows[y]?.type, presence?.columns[x]?.title, n]),
                )
              }
            />
          </InsightPanel>
        </>
      )}
    </div>
  );
}
