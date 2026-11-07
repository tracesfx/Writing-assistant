/**
 * Insights → Structure overview (SPEC §10): words per chapter, stacked by
 * point of view, with an optional metric line (tension, share of dialogue…).
 * Metrics come from `metrics.series`, so an extractor's metric shows up here
 * as soon as the model reports it.
 */
import { useMemo, useRef, useState } from "react";

import {
  CATEGORY_COLORS,
  Chart,
  ExportBar,
  InsightEmpty,
  InsightError,
  InsightHeader,
  InsightPanel,
  chapterGroups,
  memory,
  toCsv,
  useBinderTree,
  useChartColors,
  useInsightsData,
  useMemoryExtractors,
  workbench,
  type ChartExport,
  type EChartsOption,
  type InsightsViewProps,
} from "@margin/sdk";

import { UNKNOWN_POV, buildStructure, metricChoices, type Rollup } from "./model";

const fmt = (n: number) => n.toLocaleString("en-US");

export function StructureView({ active }: InsightsViewProps) {
  const extractors = useMemoryExtractors();
  const choices = useMemo(() => metricChoices(extractors), [extractors]);
  const [by, setBy] = useState<Rollup>("chapter");
  const [metricKey, setMetricKey] = useState<string>("none");
  const metric = choices.find((c) => c.key === metricKey) ?? null;
  const { data, error } = useInsightsData(
    async () => ({
      words: await memory.metrics.series("words"),
      cards: await memory.scenes.cards(),
      metric: metricKey === "none" ? null : await memory.metrics.series(metricKey),
    }),
    active,
    // Refetch when the metric changes: part of the identity of the request.
    metricKey,
  );
  const tree = useBinderTree();
  const colors = useChartColors();
  const chartRef = useRef<ChartExport | null>(null);
  const groups = useMemo(() => chapterGroups(tree), [tree]);
  const s = useMemo(() => (data ? buildStructure(data.words, data.cards, groups, by, data.metric) : null), [data, groups, by]);

  const option = useMemo<EChartsOption>(() => {
    if (!s) return {};
    const colorOf = (pov: string, i: number) => (pov === UNKNOWN_POV ? colors.border : CATEGORY_COLORS[i % CATEGORY_COLORS.length]!);
    return {
      grid: { left: 56, right: metric ? 56 : 24, top: 36, bottom: 90 },
      tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
      legend: { top: 0, type: "scroll" },
      xAxis: { type: "category", data: s.categories.map((c) => c.title), axisLabel: { rotate: 40, interval: 0, hideOverlap: true, width: 100, overflow: "truncate" } },
      yAxis: [
        { type: "value", name: "words", nameGap: 14 },
        ...(metric ? [{ type: "value" as const, name: metric.title, min: 0, max: metric.max ?? undefined, splitLine: { show: false } }] : []),
      ],
      series: [
        ...s.povs.map((pov, i) => ({
          type: "bar" as const,
          stack: "words",
          name: pov,
          data: s.words.get(pov) ?? [],
          itemStyle: { color: colorOf(pov, i) },
        })),
        ...(metric
          ? [
              {
                type: "line" as const,
                name: metric.title,
                yAxisIndex: 1,
                connectNulls: false,
                symbol: "circle",
                symbolSize: 7,
                lineStyle: { color: colors.text, width: 2 },
                itemStyle: { color: colors.text },
                data: s.metric,
              },
            ]
          : []),
      ],
    };
  }, [s, metric, colors]);

  if (error) return <InsightError message={error} />;
  const noMetricYet = metric && s && s.metric.every((m) => m === null);

  return (
    <div className="insight">
      <InsightHeader title="Structure overview" intro="How long each part of the book is, and whose eyes we see it through.">
        <div className="segmented" role="group" aria-label="Group by">
          {(["chapter", "scene"] as const).map((r) => (
            <button key={r} className={r === by ? "active" : ""} aria-pressed={r === by} onClick={() => setBy(r)}>
              By {r}
            </button>
          ))}
        </div>
        <label>
          Line{" "}
          <select value={metricKey} onChange={(e) => setMetricKey(e.target.value)}>
            <option value="none">None</option>
            {choices.map((c) => (
              <option key={c.key} value={c.key}>
                {c.title}
              </option>
            ))}
          </select>
        </label>
      </InsightHeader>

      {data && data.words.length === 0 ? (
        <InsightEmpty what="There are no scenes with text yet." />
      ) : (
        <InsightPanel
          note={
            noMetricYet
              ? `${metric?.title} has not been measured yet. Margin adds it as it reads each scene while you rest (Settings → Memory).`
              : "Point of view comes from what the model read in each scene. Click a bar to open that part of the book."
          }
        >
          <Chart
            option={option}
            height={320}
            label="Stacked bars of words per chapter by point of view, with an optional metric line"
            exportRef={chartRef}
            onClick={(p) => {
              const c = s?.categories[p.dataIndex];
              if (c) workbench.openDocument(c.firstScene);
            }}
          />
          <ExportBar
            name="structure overview"
            chart={chartRef}
            csv={() =>
              toCsv(
                [by, "scenes", ...(s?.povs.map((p) => `words (${p})`) ?? []), "total words", ...(metric ? [metric.title] : [])],
                (s?.categories ?? []).map((c, i) => {
                  const per = (s?.povs ?? []).map((p) => s?.words.get(p)?.[i] ?? 0);
                  return [c.title, c.sceneIds.length, ...per, per.reduce((a, b) => a + b, 0), ...(metric ? [s?.metric[i] ?? null] : [])];
                }),
              )
            }
          />
          <p className="muted insight-note">
            {s ? `${fmt(s.categories.reduce((n, _c, i) => n + [...s.words.values()].reduce((a, w) => a + (w[i] ?? 0), 0), 0))} words in ${s.categories.length} ${by === "chapter" ? "chapters" : "scenes"}.` : ""}
          </p>
        </InsightPanel>
      )}
    </div>
  );
}
