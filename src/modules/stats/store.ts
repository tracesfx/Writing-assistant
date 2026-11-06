/**
 * Writing stats for the open project, refreshed from domain events, plus the
 * active-time tracker (time counts only while the author is typing in a
 * document, in 15-second steps).
 */
import { create } from "zustand";

import { aiSession, events, lifecycle, story, type ProvenanceTotals, type StatsSummary } from "@margin/sdk";

interface StatsStore {
  summary: StatsSummary | null;
  /** Words from accepted AI suggestions, per document and in all (SPEC §7.6). */
  provenance: ProvenanceTotals | null;
}

export const useStats = create<StatsStore>(() => ({ summary: null, provenance: null }));

let provTimer: ReturnType<typeof setTimeout> | undefined;
export function refreshProvenance(delay = 300): void {
  clearTimeout(provTimer);
  provTimer = setTimeout(() => {
    story.provenance
      .totals()
      .then((provenance) => useStats.setState({ provenance }))
      .catch(() => useStats.setState({ provenance: null }));
  }, delay);
}

let timer: ReturnType<typeof setTimeout> | undefined;
export function refreshStats(delay = 300): void {
  clearTimeout(timer);
  timer = setTimeout(() => {
    story.stats
      .summary()
      .then((summary) => useStats.setState({ summary }))
      .catch(() => useStats.setState({ summary: null }));
  }, delay);
}

const STEP_S = 15;
const FLUSH_S = 60;

/** Start tracking; returns a cleanup function. */
export function startStats(): () => void {
  let lastKey = 0;
  let pending = 0;
  const onKey = (e: KeyboardEvent) => {
    if (e.target instanceof Element && e.target.closest(".ProseMirror")) lastKey = Date.now();
  };
  const flush = async () => {
    if (pending <= 0) return;
    const s = pending;
    pending = 0;
    await story.stats.activity(s).catch(() => {
      pending += s;
    });
  };
  window.addEventListener("keydown", onKey, { capture: true, passive: true });
  const step = setInterval(() => {
    if (Date.now() - lastKey < STEP_S * 1000) pending += STEP_S;
    if (pending >= FLUSH_S) void flush();
  }, STEP_S * 1000);
  const offs = [
    events.on("stats.updated", () => refreshStats()),
    events.on("doc.saved", () => refreshStats(1000)),
    events.on("project.opened", () => {
      aiSession.reset();
      refreshStats(500);
      refreshProvenance(500);
    }),
    events.on("ai.provenanceChanged", () => refreshProvenance()),
    events.on("project.settingsChanged", () => refreshStats()),
    events.on("project.closed", () => useStats.setState({ summary: null, provenance: null })),
    lifecycle.onBeforeClose(flush),
  ];
  refreshStats(0);
  refreshProvenance(0);
  return () => {
    window.removeEventListener("keydown", onKey, { capture: true });
    clearInterval(step);
    void flush();
    offs.forEach((o) => o());
  };
}
