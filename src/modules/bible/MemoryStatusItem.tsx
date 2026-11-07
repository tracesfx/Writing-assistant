import { MEMORY_INSPECTOR_VIEW, useMemoryStatus, workbench, type IndexState } from "@margin/sdk";

const WORDS: Record<IndexState, string> = {
  off: "off",
  upToDate: "up to date",
  pending: "waiting",
  working: "reading",
  paused: "paused (typing)",
  noModel: "model not running",
};

/** SPEC §9.4 step 5: "Memory: up to date / 3 scenes pending / paused (typing)". */
export function MemoryStatusItem() {
  const s = useMemoryStatus();
  if (!s) return null;
  let text = WORDS[s.state];
  if (s.state === "pending" || s.state === "paused" || s.state === "noModel") text = `${s.pending} scene${s.pending === 1 ? "" : "s"} pending${s.state === "paused" ? " (typing)" : s.state === "noModel" ? " (model off)" : ""}`;
  if (s.state === "working") text = `reading${s.pending > 1 ? ` · ${s.pending} pending` : ""}`;
  return (
    <button
      className={`status-memory link mem-state-${s.state}`}
      title={`Story Memory: ${WORDS[s.state]}. ${s.entries} entries. Click for the Memory Inspector.${s.lastError ? ` Last problem: ${s.lastError}` : ""}`}
      onClick={() => workbench.openView(MEMORY_INSPECTOR_VIEW)}
    >
      Memory: {text}
    </button>
  );
}
