/**
 * Insights → Plot threads (SPEC §10): one swimlane per thread, from where it
 * is introduced, through the scenes that touch it, to where it is resolved.
 * Threads still open are highlighted. Tap a dot to open that scene; tap a
 * thread's name for its bible entry.
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
  memory,
  toCsv,
  useChartColors,
  useInsightsData,
  workbench,
  type ChartExport,
  type EChartsOption,
  type InsightsViewProps,
  type ThreadStatus,
} from "@margin/sdk";

import { buildLanes, span } from "./model";

const STATUSES: { key: ThreadStatus; label: string }[] = [
  { key: "open", label: "Open" },
  { key: "resolved", label: "Resolved" },
  { key: "dropped", label: "Dropped" },
];

export function ThreadsView({ active }: InsightsViewProps) {
  const { data, error } = useInsightsData(async () => ({ threads: await memory.threads.list(null), scenes: await memory.metrics.series("words") }), active);
  const colors = useChartColors();
  const chartRef = useRef<ChartExport | null>(null);
  const [shown, setShown] = useState<ThreadStatus[]>(["open", "resolved", "dropped"]);

  const lanes = useMemo(() => (data ? buildLanes(data.threads, data.scenes, shown) : []), [data, shown]);
  const sceneTitles = useMemo(() => data?.scenes ?? [], [data]);
  const nOpen = data?.threads.filter((t) => t.status === "open").length ?? 0;

  const option = useMemo<EChartsOption>(() => {
    const n = Math.max(1, sceneTitles.length);
    const tone = (s: ThreadStatus) => (s === "open" ? colors.accent : s === "resolved" ? "#0072B2" : colors.muted);
    return {
      grid: { left: 190, right: 24, top: 12, bottom: 32 },
      tooltip: {
        formatter: (p) => {
          const d = (p as { data?: { tip?: string } }).data;
          return d?.tip ?? "";
        },
      },
      xAxis: {
        type: "value",
        min: -0.5,
        max: n - 0.5,
        interval: Math.max(1, Math.ceil(n / 12)),
        axisLabel: { formatter: (v: number) => String(Math.round(v) + 1) },
        name: "scene",
        nameLocation: "middle",
        nameGap: 22,
        splitLine: { show: false },
      },
      yAxis: {
        type: "category",
        inverse: true,
        data: lanes.map((l) => l.title),
        triggerEvent: true,
        axisLabel: { width: 170, overflow: "truncate", interval: 0, color: colors.text },
        axisTick: { show: false },
      },
      series: [
        {
          type: "lines",
          coordinateSystem: "cartesian2d",
          z: 1,
          data: lanes.map((l, y) => ({
            coords: [[l.from, y], [l.to, y]],
            lineStyle: { color: tone(l.status), width: l.status === "open" ? 6 : 3, opacity: l.status === "open" ? 0.5 : 0.35, type: (l.status === "dropped" ? "dashed" : "solid") as "dashed" | "solid" },
            tip: `${l.title}<br/>${l.status}`,
          })),
        },
        {
          type: "scatter",
          z: 3,
          name: "touched",
          symbolSize: 9,
          data: lanes.flatMap((l, y) =>
            l.touches.map((t) => ({ value: [t.order, y], scene: t.sceneId, tip: `${esc(l.title)}<br/>touched in ${esc(t.title)}`, itemStyle: { color: tone(l.status), borderColor: colors.surface, borderWidth: 1 } })),
          ),
        },
        {
          type: "scatter",
          z: 4,
          name: "introduced",
          symbol: "diamond",
          symbolSize: 15,
          data: lanes.flatMap((l, y) =>
            l.introduced === null ? [] : [{ value: [l.introduced, y], scene: sceneTitles[l.introduced]?.sceneId, tip: `${esc(l.title)}<br/>introduced in ${esc(sceneTitles[l.introduced]?.title ?? "")}`, itemStyle: { color: tone(l.status), borderColor: colors.text, borderWidth: 1 } }],
          ),
        },
        {
          type: "scatter",
          z: 4,
          name: "resolved",
          symbol: "rect",
          symbolSize: 13,
          data: lanes.flatMap((l, y) =>
            l.resolved === null ? [] : [{ value: [l.resolved, y], scene: sceneTitles[l.resolved]?.sceneId, tip: `${esc(l.title)}<br/>resolved in ${esc(sceneTitles[l.resolved]?.title ?? "")}`, itemStyle: { color: tone(l.status), borderColor: colors.text, borderWidth: 1 } }],
          ),
        },
      ],
    };
  }, [lanes, sceneTitles, colors]);

  if (error) return <InsightError message={error} />;
  const toggle = (s: ThreadStatus) => setShown((cur) => (cur.includes(s) ? cur.filter((x) => x !== s) : [...cur, s]));

  return (
    <div className="insight">
      <InsightHeader
        title="Plot threads"
        intro={data ? `${nOpen} open thread${nOpen === 1 ? "" : "s"} · ${data.threads.length} in all. A diamond is where a thread starts, a square where it ends, dots are scenes that touch it.` : "Loading…"}
      >
        <div className="chips" role="group" aria-label="Show" hidden={!data || data.threads.length === 0}>
          {STATUSES.map((s) => (
            <button key={s.key} className={shown.includes(s.key) ? "chip active" : "chip"} aria-pressed={shown.includes(s.key)} onClick={() => toggle(s.key)}>
              {s.label}
            </button>
          ))}
        </div>
      </InsightHeader>

      {data && data.threads.length === 0 ? (
        <InsightEmpty what="No plot threads have been noticed yet." />
      ) : (
        <>
          <InsightPanel note="Open threads are drawn in the accent colour and run to the end of the book.">
            <Chart
              option={option}
              height={Math.max(180, 80 + lanes.length * 30)}
              label="Swimlane chart of plot threads across the scenes of the book"
              exportRef={chartRef}
              onClick={(p) => {
                if (p.componentType === "yAxis") {
                  const lane = lanes.find((l) => l.title === p.name);
                  if (lane) workbench.openView(BIBLE_ENTRY_VIEW, lane.id);
                  return;
                }
                const scene = (p.data as { scene?: string } | undefined)?.scene;
                if (scene) workbench.openDocument(scene);
              }}
            />
            <ExportBar
              name="plot threads"
              chart={chartRef}
              csv={() =>
                toCsv(
                  ["thread", "status", "introduced in", "resolved in", "scenes it spans", "scenes that touch it", "status set by"],
                  lanes.map((l) => [
                    l.title,
                    l.status,
                    l.introduced === null ? "" : sceneTitles[l.introduced]?.title,
                    l.resolved === null ? "" : sceneTitles[l.resolved]?.title,
                    span(l),
                    l.touches.length,
                    l.canon ? "author" : "model",
                  ]),
                )
              }
            />
          </InsightPanel>
        </>
      )}
    </div>
  );
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
