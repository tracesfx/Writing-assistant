/**
 * The Insights extension point (SPEC §10): the Insights workspace lists the
 * views modules contribute (`insightsViews`), and offers the extra metrics
 * that `memoryExtractors` declare. A view never imports another module; it
 * reads data through `story` and `memory` and re-reads it with
 * `useInsightsData` whenever the project changes.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";

import { commands } from "./bindings";
import type { InsightsViewContribution, MemoryExtractorContribution } from "./extensions";
import { events } from "./events";
import { useMemoryVersion } from "./memory";
import { unwrap } from "./story";
import { workbench } from "./workbench";

export type RegisteredInsightsView = InsightsViewContribution & { moduleId: string };
export type RegisteredExtractor = MemoryExtractorContribution & { moduleId: string };

/** The workspace view that hosts the Insights views. */
export const INSIGHTS_VIEW = "insights.home";

interface InsightsStore {
  views: RegisteredInsightsView[];
  extractors: RegisteredExtractor[];
  /** The view showing in the Insights workspace. */
  selected: string | null;
  /** Bumps on `stats.updated`. */
  statsVersion: number;
}

export const useInsightsStore = create<InsightsStore>(() => ({
  views: [],
  extractors: [],
  selected: null,
  statsVersion: 0,
}));

/** Called by the shell with what the enabled modules contribute. */
export function setInsights(views: RegisteredInsightsView[], extractors: RegisteredExtractor[]): void {
  useInsightsStore.setState({
    views: [...views].sort((a, b) => (a.order ?? 0) - (b.order ?? 0)),
    extractors,
  });
}

export const useInsightsViews = () => useInsightsStore((s) => s.views);
export const useMemoryExtractors = () => useInsightsStore((s) => s.extractors);

/** Open the Insights workspace on one view. */
export function openInsight(viewId: string): void {
  useInsightsStore.setState({ selected: viewId });
  workbench.openView(INSIGHTS_VIEW);
}

let followingStats = false;
function followStats(): void {
  if (followingStats) return;
  followingStats = true;
  events.on("stats.updated", () => useInsightsStore.setState((s) => ({ statsVersion: s.statsVersion + 1 })));
}

export interface InsightsData<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
}

/**
 * Load a view's data, and load it again when memory, the binder or the stats
 * change (`memory.updated`, `binder.changed`, `stats.updated`). Nothing is
 * fetched while the view is hidden; a change that happened meanwhile is
 * picked up when it shows again. Changes are batched over 250 ms. If the request
 * depends on a choice (a metric, a filter), pass it as `requestKey`.
 */
export function useInsightsData<T>(load: () => Promise<T>, active: boolean, requestKey = ""): InsightsData<T> {
  followStats();
  const mem = useMemoryVersion();
  const stats = useInsightsStore((s) => s.statsVersion);
  const [state, setState] = useState<InsightsData<T>>({ data: null, error: null, loading: true });
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });
  const loadedFor = useRef<string | null>(null);
  const key = `${mem}:${stats}:${requestKey}`;
  useEffect(() => {
    if (!active || loadedFor.current === key) return;
    let cancelled = false;
    const t = setTimeout(
      () => {
        loadedFor.current = key;
        loadRef
          .current()
          .then((data) => {
            if (!cancelled) setState({ data, error: null, loading: false });
          })
          .catch((e: unknown) => {
            if (!cancelled) setState((s) => ({ data: s.data, error: e instanceof Error ? e.message : String(e), loading: false }));
          });
      },
      loadedFor.current === null ? 0 : 250,
    );
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [active, key]);
  return state;
}

// --- Export (SPEC §10: every view exports its data as CSV and its chart as PNG/SVG) -----

type Cell = string | number | boolean | null | undefined;

/** RFC 4180 CSV with a header row; spreadsheet-formula starts are defused. */
export function toCsv(header: string[], rows: Cell[][]): string {
  const esc = (v: Cell): string => {
    let s = v === null || v === undefined ? "" : String(v);
    // A cell that begins with = + - @ would run as a formula in a spreadsheet.
    if (typeof v === "string" && /^[=+\-@\t\r]/.test(s)) s = `'${s}`;
    return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  return [header, ...rows].map((r) => r.map(esc).join(",")).join("\r\n") + "\r\n";
}

/** A file name safe on Windows: letters, digits, dashes. */
export function exportName(base: string, ext: "csv" | "png" | "svg"): string {
  const safe = base.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "insight";
  return `${safe}.${ext}`;
}

let lastDir: string | null = null;

/**
 * Ask for a folder (the native picker, not a confirmation) and save one file
 * there. Returns the path, or null if the author cancelled.
 */
export async function saveInsightFile(name: string, data: string | Uint8Array): Promise<string | null> {
  // `__marginExportDir` lets the CDP driver answer for the native picker (it cannot be automated).
  const forced = (window as { __marginExportDir?: string }).__marginExportDir;
  const dir = forced ?? (await open({ directory: true, multiple: false, defaultPath: lastDir ?? undefined, title: `Save ${name} in…` }));
  if (typeof dir !== "string") return null;
  lastDir = dir;
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  const path = await unwrap(commands.insightExport(dir, name, Array.from(bytes)));
  workbench.notify({ message: `Saved ${path}` });
  return path;
}
