/**
 * Insights → Timeline (SPEC §10): the events of the book in two lanes, the
 * order the book tells them and the order they happen, with the point-of-view
 * character as colour bands. The model gives events a label ("that night");
 * the author places them in story order with the arrows. That is an author
 * action, so the event becomes canon and later readings leave it alone.
 */
import { useMemo, useRef, useState } from "react";

import {
  Chart,
  ExportBar,
  InsightEmpty,
  InsightError,
  InsightHeader,
  InsightPanel,
  memory,
  openSource,
  toCsv,
  useChartColors,
  useInsightsData,
  workbench,
  type ChartExport,
  type EChartsOption,
  type InsightsViewProps,
} from "@margin/sdk";

import { bands, buildItems, moveOrder, povColors, type Item } from "./model";

export function TimelineView({ active }: InsightsViewProps) {
  const { data, error } = useInsightsData(async () => ({ events: await memory.timeline.events(), cards: await memory.scenes.cards() }), active);
  const colors = useChartColors();
  const chartRef = useRef<ChartExport | null>(null);
  const [selId, setSelId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const items = useMemo(() => (data ? buildItems(data.events, data.cards) : []), [data]);
  const pov = useMemo(() => povColors(items), [items]);
  const sel = items.find((i) => i.id === selId) ?? null;
  const colorOf = (it: Item) => pov.get(it.pov) ?? colors.muted;

  const option = useMemo<EChartsOption>(() => {
    const n = items.length;
    const areas = bands(items).map((b) => [
      { xAxis: b.from - 0.5, yAxis: 0.62, itemStyle: { color: pov.get(b.pov) ?? colors.border, opacity: 0.22 } },
      { xAxis: b.to + 0.5, yAxis: 1.45 },
    ]);
    return {
      grid: { left: 150, right: 24, top: 16, bottom: 28 },
      tooltip: {
        formatter: (p) => {
          const d = (p as { data: unknown }).data;
          const idx = Array.isArray(d) ? Number(d[2]) : Number((d as { idx: number }).idx);
          const it = items[idx];
          if (!it) return "";
          return `<strong>${esc(it.title)}</strong><br/>${esc(it.sceneTitle)}${it.when ? ` · ${esc(it.when)}` : ""}${it.pov ? `<br/>POV: ${esc(it.pov)}` : ""}`;
        },
      },
      xAxis: { type: "value", min: -0.5, max: Math.max(0.5, n - 0.5), show: false },
      yAxis: {
        type: "value",
        min: -1,
        max: 2,
        interval: 1,
        splitLine: { show: false },
        axisLabel: { formatter: (v: number) => (v === 1 ? "As the book tells it" : v === 0 ? "As it happens" : ""), color: colors.text },
      },
      series: [
        {
          type: "lines",
          coordinateSystem: "cartesian2d",
          silent: false,
          z: 1,
          lineStyle: { width: 1.5, opacity: 0.55 },
          data: items.map((it, idx) => ({ coords: [[it.narrative, 1], [it.story, 0]], idx, lineStyle: { color: colorOf(it), opacity: it.id === selId ? 1 : 0.55, width: it.id === selId ? 3 : 1.5 } })),
        },
        {
          type: "scatter",
          z: 3,
          symbolSize: 11,
          markArea: { silent: true, data: areas as never },
          data: items.map((it, idx) => ({ value: [it.narrative, 1, idx], itemStyle: { color: colorOf(it), borderColor: it.id === selId ? colors.text : colors.surface, borderWidth: it.id === selId ? 3 : 1 } })),
        },
        {
          type: "scatter",
          z: 3,
          symbolSize: 11,
          // Hollow = no story position yet: it sits where the book tells it.
          data: items.map((it, idx) => ({
            value: [it.story, 0, idx],
            itemStyle: { color: it.explicit === null ? colors.surface : colorOf(it), borderColor: it.id === selId ? colors.text : colorOf(it), borderWidth: it.id === selId ? 3 : 2 },
          })),
        },
      ],
    };
  }, [items, pov, colors, selId]); // eslint-disable-line react-hooks/exhaustive-deps

  const place = async (it: Item, dir: -1 | 1 | 0) => {
    setBusy(true);
    try {
      await memory.timeline.setOrder(it.id, dir === 0 ? null : moveOrder(items, it.id, dir));
    } catch (e) {
      workbench.notify({ message: `Could not move the event: ${e instanceof Error ? e.message : String(e)}` });
    } finally {
      setBusy(false);
    }
  };
  const read = (it: Item) => {
    const src = data?.events.find((e) => e.event.id === it.id)?.event.source;
    if (src) void openSource(src);
    else if (it.sceneId) workbench.openDocument(it.sceneId);
  };

  if (error) return <InsightError message={error} />;
  const placed = items.filter((i) => i.explicit !== null).length;

  return (
    <div className="insight">
      <InsightHeader title="Timeline" intro="The top lane is the order the book tells events; the bottom lane is the order they happen. Lines join the same event." />
      {data && items.length === 0 ? (
        <InsightEmpty what="No events have been read from your scenes yet." />
      ) : (
        <>
          {pov.size > 0 && (
            <div className="chips legend" aria-label="Point of view">
              {[...pov.entries()].map(([name, color]) => (
                <span key={name} className="chip static">
                  <span className="swatch" style={{ background: color }} aria-hidden="true" /> {name}
                </span>
              ))}
            </div>
          )}
          <InsightPanel
            note={`${placed} of ${items.length} events have a story position you set. Hollow dots sit where the book tells them. Select an event to move it earlier or later in story time.`}
          >
            <Chart
              option={option}
              height={260}
              label="Two lanes of events: the order the book tells them above, the order they happen below, joined by lines"
              exportRef={chartRef}
              onClick={(p) => {
                const d = p.data as unknown;
                const idx = Array.isArray(d) ? Number(d[2]) : Number((d as { idx?: number; value?: number[] })?.idx ?? (d as { value?: number[] })?.value?.[2]);
                const it = items[idx];
                if (it) setSelId(it.id);
              }}
            />
            <div className="insight-detail" aria-live="polite">
              {sel ? (
                <>
                  <strong>{sel.title}</strong>{" "}
                  <span className="muted">
                    {sel.sceneTitle}
                    {sel.when ? ` · ${sel.when}` : ""}
                  </span>{" "}
                  <button onClick={() => read(sel)}>Read the passage</button>{" "}
                  <button disabled={busy} onClick={() => void place(sel, -1)} title="Happens earlier in story time">
                    ← Earlier
                  </button>
                  <button disabled={busy} onClick={() => void place(sel, 1)} title="Happens later in story time">
                    Later →
                  </button>
                  {sel.explicit !== null && (
                    <button disabled={busy} onClick={() => void place(sel, 0)} title="Go back to where the book tells it">
                      Reset
                    </button>
                  )}
                </>
              ) : (
                <span className="muted">Select an event on the chart or in the list.</span>
              )}
            </div>
            <ExportBar
              name="timeline"
              chart={chartRef}
              csv={() =>
                toCsv(
                  ["event", "scene", "story time (label)", "told #", "happens #", "your story position", "point of view", "people", "status"],
                  items.map((i) => [i.title, i.sceneTitle, i.when, i.narrative + 1, i.story + 1, i.explicit, i.pov, i.entities.join("; "), i.canon ? "canon" : "inferred"]),
                )
              }
            />
          </InsightPanel>
          <InsightPanel title="Events">
            <table className="insight-table">
              <thead>
                <tr>
                  <th>Told</th>
                  <th>Happens</th>
                  <th>Event</th>
                  <th>Scene</th>
                  <th>When</th>
                </tr>
              </thead>
              <tbody>
                {items.map((i) => (
                  <tr key={i.id} className={i.id === selId ? "selected" : ""} onClick={() => setSelId(i.id)}>
                    <td className="num">{i.narrative + 1}</td>
                    <td className="num">{i.story + 1}</td>
                    <td>{i.title}</td>
                    <td>{i.sceneTitle}</td>
                    <td className="muted">{i.when}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </InsightPanel>
        </>
      )}
    </div>
  );
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
