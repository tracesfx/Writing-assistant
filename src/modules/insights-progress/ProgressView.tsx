/**
 * Insights → Writing progress (SPEC §10): words per day, the streak heatmap,
 * the manuscript's growth against the goal, and your words against accepted
 * AI words.
 */
import { useMemo, useRef, useState } from "react";

import {
  Chart,
  ExportBar,
  InsightError,
  InsightHeader,
  InsightPanel,
  chapterGroups,
  heatLevels,
  heatPieces,
  story,
  toCsv,
  useBinderTree,
  useChartColors,
  useInsightsData,
  workbench,
  type ChartExport,
  type EChartsOption,
  type InsightsViewProps,
} from "@margin/sdk";

import { addDays, chapterSplit, cumulative, dailySeries, dayRange, longestStreak, pace, streakEndingNow } from "./model";

const fmt = (n: number) => n.toLocaleString("en-US");
const RANGES = [30, 90, 365] as const;

export function ProgressView({ active }: InsightsViewProps) {
  const { data, error } = useInsightsData(
    async () => ({ summary: await story.stats.summary(), prov: await story.provenance.totals() }),
    active,
  );
  const tree = useBinderTree();
  const colors = useChartColors();
  const [range, setRange] = useState<(typeof RANGES)[number]>(90);
  const barsRef = useRef<ChartExport | null>(null);
  const calRef = useRef<ChartExport | null>(null);
  const growthRef = useRef<ChartExport | null>(null);
  const splitRef = useRef<ChartExport | null>(null);

  const summary = data?.summary;
  const today = summary?.date ?? "";
  const daily = useMemo(() => (summary ? dailySeries(summary.days, today, range) : []), [summary, today, range]);
  const goals = summary?.goals;

  const barsOption = useMemo<EChartsOption>(
    () => ({
      grid: { left: 48, right: 16, top: 24, bottom: 28 },
      tooltip: { trigger: "axis", valueFormatter: (v) => `${fmt(Number(v))} words` },
      xAxis: { type: "category", data: daily.map((d) => d.date), axisLabel: { formatter: (v: string) => v.slice(5), hideOverlap: true } },
      yAxis: { type: "value", name: "net words", nameGap: 12 },
      series: [
        {
          type: "bar",
          name: "Words",
          data: daily.map((d) => d.words),
          itemStyle: { color: colors.accent },
          markLine: goals?.dailyWords
            ? {
                silent: true,
                symbol: "none",
                lineStyle: { color: colors.muted, type: "dashed" },
                label: { formatter: "daily goal", color: colors.muted },
                data: [{ yAxis: goals.dailyWords }],
              }
            : undefined,
        },
      ],
    }),
    [daily, goals, colors],
  );

  const calOption = useMemo<EChartsOption>(() => {
    if (!summary) return {};
    const end = today;
    const start = addDays(end, -363);
    const by = new Map(summary.days.map((d) => [d.date, d.words]));
    const values = dayRange(start, end).map((d) => [d, Math.max(0, by.get(d) ?? 0)] as [string, number]);
    const cuts = heatLevels(values.map((v) => v[1]));
    return {
      tooltip: {
        formatter: (p) => {
          const d = (p as unknown as { data: [string, number] }).data;
          return `${d[0]}: ${fmt(d[1])} words`;
        },
      },
      visualMap: {
        show: false,
        type: "piecewise",
        pieces: heatPieces(colors, cuts),
      },
      calendar: {
        range: [start, end],
        cellSize: [14, 14],
        top: 28,
        left: 36,
        right: 12,
        itemStyle: { borderWidth: 3, borderColor: colors.surface },
        yearLabel: { show: false },
        dayLabel: { firstDay: 1, nameMap: "en" },
        monthLabel: { nameMap: "en" },
      },
      series: [{ type: "heatmap", coordinateSystem: "calendar", data: values }],
    };
  }, [summary, today, colors]);

  const growth = useMemo(() => (summary ? cumulative(summary.days, summary.projectWords) : []), [summary]);
  const growthOption = useMemo<EChartsOption>(() => {
    const line = pace(growth[0], goals ?? {});
    const target = goals?.projectWords ?? null;
    const goalEnd = goals?.deadline && goals.deadline > today ? goals.deadline : today;
    return {
      grid: { left: 56, right: 24, top: 28, bottom: 28 },
      tooltip: { trigger: "axis", valueFormatter: (v) => `${fmt(Number(v))} words` },
      legend: { top: 0, right: 0 },
      xAxis: { type: "time" },
      yAxis: { type: "value", name: "words", nameGap: 12 },
      series: [
        {
          type: "line",
          name: "Manuscript",
          step: "end",
          symbol: "circle",
          symbolSize: 5,
          data: growth.map((g) => [g.date, g.total]),
          itemStyle: { color: colors.accent },
          lineStyle: { color: colors.accent },
        },
        ...(line.length
          ? [
              {
                type: "line" as const,
                name: "Pace to deadline",
                symbol: "none",
                lineStyle: { type: "dashed" as const, color: colors.muted },
                itemStyle: { color: colors.muted },
                data: line.map((g) => [g.date, g.total]),
              },
            ]
          : []),
        ...(target && growth.length
          ? [
              {
                type: "line" as const,
                name: "Goal",
                symbol: "none",
                lineStyle: { color: colors.danger, type: "dotted" as const },
                itemStyle: { color: colors.danger },
                data: [
                  [growth[0]!.date, target],
                  [goalEnd, target],
                ],
              },
            ]
          : []),
      ],
    };
  }, [growth, goals, colors, today]);

  const groups = useMemo(() => chapterGroups(tree), [tree]);
  const split = useMemo(() => chapterSplit(tree, groups, data?.prov ?? null), [tree, groups, data]);
  const totalAi = data?.prov?.projectAiWords ?? 0;
  const splitOption = useMemo<EChartsOption>(
    () => ({
      grid: { left: 130, right: 24, top: 28, bottom: 24 },
      tooltip: { trigger: "axis", axisPointer: { type: "shadow" }, valueFormatter: (v) => `${fmt(Number(v))} words` },
      legend: { top: 0, right: 0 },
      xAxis: { type: "value" },
      yAxis: { type: "category", inverse: true, data: split.map((s) => s.title || "Untitled"), axisLabel: { width: 110, overflow: "truncate" } },
      series: [
        { type: "bar", stack: "w", name: "Your words", data: split.map((s) => s.yours), itemStyle: { color: colors.accent } },
        { type: "bar", stack: "w", name: "AI-accepted words", data: split.map((s) => s.ai), itemStyle: { color: "#0072B2" } },
      ],
    }),
    [split, colors],
  );

  if (error) return <InsightError message={error} />;
  const withWriting = summary?.days.filter((d) => d.words > 0).length ?? 0;

  return (
    <div className="insight">
      <InsightHeader
        title="Writing progress"
        intro={
          summary
            ? `${fmt(summary.projectWords)} words in the manuscript · ${streakEndingNow(summary.days, today)}-day streak (longest ${longestStreak(summary.days)}) · ${withWriting} days with writing`
            : "Loading…"
        }
      >
        <div className="segmented" role="group" aria-label="Range">
          {RANGES.map((r) => (
            <button key={r} className={r === range ? "active" : ""} aria-pressed={r === range} onClick={() => setRange(r)}>
              {r === 365 ? "1 year" : `${r} days`}
            </button>
          ))}
        </div>
      </InsightHeader>

      <InsightPanel title="Words per day" note="Net words: deleting counts against the day. Splitting, merging and moving scenes are not writing.">
        <Chart option={barsOption} height={240} label={`Bar chart of net words written per day over the last ${range} days`} exportRef={barsRef} />
        <ExportBar name="words per day" csv={() => toCsv(["date", "net words", "active minutes"], daily.map((d) => [d.date, d.words, d.minutes]))} chart={barsRef} />
      </InsightPanel>

      <InsightPanel title="Streak calendar" note="Darker days are bigger days. The last 52 weeks.">
        <Chart option={calOption} height={150} label="Calendar heatmap of days with writing over the last 52 weeks" exportRef={calRef} />
        <ExportBar name="streak calendar" csv={() => toCsv(["date", "net words"], (summary?.days ?? []).map((d) => [d.date, d.words]))} chart={calRef} />
      </InsightPanel>

      <InsightPanel
        title="Growth against the goal"
        note={
          goals?.projectWords
            ? `Goal ${fmt(goals.projectWords)} words${goals.deadline ? ` by ${goals.deadline}` : ""}.`
            : "Set a project goal and a deadline in Settings → Goals to see the goal and pace lines."
        }
      >
        <Chart option={growthOption} height={260} label="Line chart of the manuscript's word count over time against the goal" exportRef={growthRef} />
        <ExportBar name="growth" csv={() => toCsv(["date", "manuscript words at end of day"], growth.map((g) => [g.date, g.total]))} chart={growthRef} />
      </InsightPanel>

      <InsightPanel
        title="Your words and AI words"
        note={
          totalAi > 0
            ? `${fmt(totalAi)} words came from accepted suggestions. Retyping or editing a passage makes it yours. Click a bar to open the chapter.`
            : "No suggestions accepted in this project. Everything here is yours."
        }
      >
        <Chart
          option={splitOption}
          height={Math.max(160, 56 + split.length * 26)}
          label="Stacked bars of your words and accepted AI words per chapter"
          onClick={(p) => {
            const row = split[p.dataIndex];
            if (row) workbench.openDocument(row.id);
          }}
          exportRef={splitRef}
        />
        <ExportBar name="your words and ai words" csv={() => toCsv(["chapter", "your words", "ai-accepted words"], split.map((s) => [s.title, s.yours, s.ai]))} chart={splitRef} />
      </InsightPanel>
    </div>
  );
}
