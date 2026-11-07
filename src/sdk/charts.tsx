/**
 * Chart plumbing shared by the Insights views: a lazy-loaded Apache ECharts
 * canvas that follows the app theme, and the Export bar (CSV, PNG, SVG).
 * ECharts is bundled (no CDN) and only loads when a chart is first drawn.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { exportName, saveInsightFile, toCsv } from "./insights";
import { workbench } from "./workbench";

type ECharts = import("echarts/core").ECharts;
type EChartsOption = import("echarts").EChartsOption;
export type { EChartsOption };

/** Categorical colours: Okabe-Ito, readable by people with colour-vision differences. */
export const CATEGORY_COLORS = ["#0072B2", "#E69F00", "#009E73", "#CC79A7", "#56B4E9", "#D55E00", "#8C6BB1", "#7F7F7F"];

export interface ChartColors {
  text: string;
  muted: string;
  border: string;
  surface: string;
  accent: string;
  danger: string;
  /** Light to dark ramp for heatmaps and calendars. */
  heat: string[];
}

function cssVar(style: CSSStyleDeclaration, name: string, fallback: string): string {
  return style.getPropertyValue(name).trim() || fallback;
}

/** The current theme's colours, read from the CSS variables the app sets. */
export function readChartColors(): ChartColors {
  const s = getComputedStyle(document.documentElement);
  return {
    text: cssVar(s, "--text", "#2a2723"),
    muted: cssVar(s, "--muted", "#7a7266"),
    border: cssVar(s, "--border", "#e2ddd2"),
    surface: cssVar(s, "--bg-elev", "#fdfcf9"),
    accent: cssVar(s, "--accent", "#8a5a2b"),
    danger: cssVar(s, "--danger", "#a33a2a"),
    heat: [0, 1, 2, 3, 4].map((n) => cssVar(s, `--heat-${n}`, "#ccc")),
  };
}

/** Re-render when the theme changes (the `data-theme` attribute or Windows' setting). */
export function useChartColors(): ChartColors {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const bump = () => setTick((t) => t + 1);
    const mo = new MutationObserver(bump);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style", "class"] });
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    mq?.addEventListener?.("change", bump);
    return () => {
      mo.disconnect();
      mq?.removeEventListener?.("change", bump);
    };
  }, []);
  // `tick` only forces a fresh read of the CSS variables.
  return useMemo(() => {
    void tick;
    return readChartColors();
  }, [tick]);
}

let echartsPromise: Promise<typeof import("echarts/core")> | null = null;

/** Load ECharts with only the parts the views use. */
export function loadEcharts(): Promise<typeof import("echarts/core")> {
  echartsPromise ??= (async () => {
    const [core, charts, components, renderers] = await Promise.all([
      import("echarts/core"),
      import("echarts/charts"),
      import("echarts/components"),
      import("echarts/renderers"),
    ]);
    core.use([
      charts.BarChart,
      charts.LineChart,
      charts.ScatterChart,
      charts.HeatmapChart,
      charts.LinesChart,
      charts.CustomChart,
      components.GridComponent,
      components.TooltipComponent,
      components.LegendComponent,
      components.VisualMapComponent,
      components.CalendarComponent,
      components.MarkLineComponent,
      components.MarkAreaComponent,
      components.DataZoomComponent,
      components.AriaComponent,
      renderers.CanvasRenderer,
      renderers.SVGRenderer,
    ]);
    return core;
  })();
  return echartsPromise;
}

function themeFor(c: ChartColors): Record<string, unknown> {
  const axis = {
    axisLine: { lineStyle: { color: c.border } },
    axisTick: { lineStyle: { color: c.border } },
    axisLabel: { color: c.muted },
    splitLine: { lineStyle: { color: c.border, opacity: 0.6 } },
    nameTextStyle: { color: c.muted },
  };
  return {
    color: CATEGORY_COLORS,
    backgroundColor: "transparent",
    textStyle: { color: c.text, fontFamily: "inherit" },
    categoryAxis: axis,
    valueAxis: axis,
    timeAxis: axis,
    logAxis: axis,
    legend: { textStyle: { color: c.text } },
    tooltip: {
      backgroundColor: c.surface,
      borderColor: c.border,
      textStyle: { color: c.text },
      extraCssText: "box-shadow:0 4px 16px rgba(0,0,0,.18);",
    },
    visualMap: { textStyle: { color: c.muted } },
    calendar: {
      itemStyle: { borderColor: c.border },
      dayLabel: { color: c.muted },
      monthLabel: { color: c.muted },
      yearLabel: { color: c.muted },
      splitLine: { lineStyle: { color: c.border } },
    },
  };
}

/** Lets an export bar draw the chart that is on screen. */
export interface ChartExport {
  /** PNG bytes at 2x, on the theme's surface colour. */
  png(): Promise<Uint8Array>;
  /** The chart as an SVG document. */
  svg(): Promise<string>;
}

export interface ChartProps {
  option: EChartsOption;
  height?: number;
  /** Plain-language description for screen readers. */
  label: string;
  /** Called with the clicked element's `data` (a series point) when it has one. */
  onClick?: (params: { data: unknown; seriesIndex: number; dataIndex: number; name: string; componentType: string }) => void;
  /** Filled in once the chart exists, for `ExportBar`. */
  exportRef?: { current: ChartExport | null };
}

/** An ECharts canvas that fills its container's width and follows the theme. */
export function Chart({ option, height = 320, label, onClick, exportRef }: ChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const chart = useRef<ECharts | null>(null);
  const colors = useChartColors();
  const latest = useRef({ option, onClick });
  useEffect(() => {
    latest.current = { option, onClick };
  });
  const [ready, setReady] = useState(0);

  // Create the chart (again when the theme changes: themes are fixed at init).
  useEffect(() => {
    let disposed = false;
    let ro: ResizeObserver | undefined;
    void loadEcharts().then((echarts) => {
      const el = host.current;
      if (disposed || !el) return;
      const name = `margin-${Math.random().toString(36).slice(2)}`;
      echarts.registerTheme(name, themeFor(colors));
      const c = echarts.init(el, name, { renderer: "canvas" });
      chart.current = c;
      // A handle for the app-driving tests (CDP), which need pixel positions of chart items.
      (el as HTMLDivElement & { __marginChart?: ECharts }).__marginChart = c;
      c.on("click", (p) => latest.current.onClick?.(p as never));
      ro = new ResizeObserver(() => {
        if (el.clientWidth > 0) c.resize();
      });
      ro.observe(el);
      if (exportRef) {
        exportRef.current = {
          png: async () => {
            const url = c.getDataURL({ type: "png", pixelRatio: 2, backgroundColor: colors.surface });
            const bin = atob(url.slice(url.indexOf(",") + 1));
            return Uint8Array.from(bin, (ch) => ch.charCodeAt(0));
          },
          svg: async () => {
            const w = el.clientWidth || 800;
            const h = el.clientHeight || height;
            const off = echarts.init(null, name, { renderer: "svg", ssr: true, width: w, height: h });
            off.setOption({ ...latest.current.option, animation: false, backgroundColor: colors.surface } as never);
            const svg = off.renderToSVGString();
            off.dispose();
            return svg;
          },
        };
      }
      (el as HTMLDivElement & { __marginExport?: { current: ChartExport | null } | undefined }).__marginExport = exportRef;
      setReady((n) => n + 1);
    });
    return () => {
      disposed = true;
      ro?.disconnect();
      chart.current?.dispose();
      chart.current = null;
      if (exportRef) exportRef.current = null;
    };
    // The chart is rebuilt only when the colours change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [colors]);

  useEffect(() => {
    chart.current?.setOption(option, { notMerge: true });
  }, [option, ready]);

  return <div ref={host} className="insight-chart" style={{ height }} role="img" aria-label={label} />;
}

export interface ExportBarProps {
  /** File name base, e.g. "character presence". */
  name: string;
  /** The data behind the view, as CSV text. */
  csv: () => string;
  chart?: { current: ChartExport | null };
  children?: ReactNode;
}

/** "Export CSV · PNG · SVG" for one view. */
export function ExportBar({ name, csv, chart, children }: ExportBarProps) {
  const [busy, setBusy] = useState(false);
  const run = useCallback(
    async (f: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await f();
      } catch (e) {
        workbench.notify({ message: `Could not export: ${e instanceof Error ? e.message : String(e)}` });
      } finally {
        setBusy(false);
      }
    },
    [],
  );
  return (
    <div className="insight-export" role="group" aria-label="Export">
      {children}
      <span className="muted">Export</span>
      <button disabled={busy} onClick={() => void run(() => saveInsightFile(exportName(name, "csv"), csv()))}>
        CSV
      </button>
      {chart && (
        <>
          <button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const c = chart.current;
                if (c) await saveInsightFile(exportName(name, "png"), await c.png());
              })
            }
          >
            PNG
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const c = chart.current;
                if (c) await saveInsightFile(exportName(name, "svg"), await c.svg());
              })
            }
          >
            SVG
          </button>
        </>
      )}
    </div>
  );
}

export { toCsv };
