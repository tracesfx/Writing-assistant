/**
 * Memory Inspector (SPEC §9.6): one searchable, filterable table of everything
 * the model tracks, with bulk actions and the memory exports.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  BIBLE_ENTRY_VIEW,
  CONTEXT_VIEW,
  memory,
  openSource,
  useMemoryStatus,
  useMemoryVersion,
  workbench,
  type ExportFormat,
  type InspectorRow,
  type ItemRef,
} from "@margin/sdk";

import { StatusMark, fail } from "./common";

const KINDS = ["entity", "fact", "relation", "event", "thread", "scene", "mention", "metric"] as const;
const SHOWN = 500;

export function InspectorView() {
  const version = useMemoryVersion();
  const status = useMemoryStatus();
  const [rows, setRows] = useState<InspectorRow[]>([]);
  const [kind, setKind] = useState<string>("");
  const [state, setState] = useState<"" | "canon" | "inferred">("");
  const [by, setBy] = useState<"" | "ai" | "author">("");
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);

  const load = useCallback(async () => {
    try {
      setRows(await memory.inspector.rows());
    } catch {
      setRows([]);
    }
  }, []);
  useEffect(() => {
    let live = true;
    memory.inspector
      .rows()
      .then((r) => live && setRows(r))
      .catch(() => live && setRows([]));
    return () => {
      live = false;
    };
  }, [version]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter(
      (r) =>
        (!kind || r.kind === kind) &&
        (!state || r.status === state) &&
        (!by || r.createdBy === by) &&
        (!q || `${r.label} ${r.detail} ${r.entityName ?? ""}`.toLowerCase().includes(q)),
    );
  }, [rows, kind, state, by, query]);

  const counts = useMemo(() => {
    const c = new Map<string, number>();
    for (const r of rows) c.set(r.kind, (c.get(r.kind) ?? 0) + 1);
    return c;
  }, [rows]);

  const key = (r: InspectorRow) => `${r.kind}:${r.id}`;
  const pickedRows = shown.filter((r) => picked.has(key(r)));
  const inferredItems: ItemRef[] = pickedRows.flatMap((r) => (r.item && r.status === "inferred" ? [r.item] : []));
  const sources = [...new Set(pickedRows.flatMap((r) => (r.source ? [r.source.doc] : [])))];

  async function run(what: string, f: () => Promise<unknown>) {
    setBusy(true);
    try {
      await f();
      setPicked(new Set());
    } catch (e) {
      fail(what, e);
    } finally {
      setBusy(false);
      void load();
    }
  }

  async function doExport(format: ExportFormat) {
    setExportOpen(false);
    const dir = await open({ directory: true, title: "Export memory to a folder" });
    if (typeof dir !== "string") return;
    await run("Could not export", async () => {
      const files = await memory.export(dir, format);
      workbench.notify({ message: `Exported ${files.length} file${files.length === 1 ? "" : "s"} to ${dir}.` });
    });
  }

  async function doImport() {
    setExportOpen(false);
    const f = await open({ filters: [{ name: "Margin memory export", extensions: ["json"] }], title: "Import a memory export" });
    if (typeof f !== "string") return;
    await run("Could not import", async () => {
      const n = await memory.import(f);
      workbench.notify({ message: `Imported ${n} entries.` });
    });
  }

  const open_ = (r: InspectorRow) => {
    if (r.kind === "scene" && r.source) workbench.openDocument(r.source.doc);
    else if (r.kind === "entity" || r.kind === "thread" || r.entityId) workbench.openView(BIBLE_ENTRY_VIEW, r.entityId ?? r.id);
  };

  return (
    <div className="inspector">
      <header className="inspector-head">
        <h2>Memory Inspector</h2>
        <span className="muted">
          {status ? `${status.entries} entries · ${rows.length} records` : `${rows.length} records`}
          {status && status.pending > 0 && ` · ${status.pending} scene${status.pending === 1 ? "" : "s"} waiting to be read`}
        </span>
        {status?.lastError && <span className="ai-warn" title={status.lastError}>Last problem: {status.lastError.slice(0, 80)}</span>}
        <span className="grow" />
        <div className="inspector-export">
          <button onClick={() => setExportOpen((o) => !o)} aria-expanded={exportOpen}>
            Export ▾
          </button>
          {exportOpen && (
            <div className="menu" role="menu">
              <button className="menu-item" role="menuitem" onClick={() => void doExport("json")}>JSON (full data)</button>
              <button className="menu-item" role="menuitem" onClick={() => void doExport("csv")}>CSV (one file per record type)</button>
              <button className="menu-item" role="menuitem" onClick={() => void doExport("markdown")}>Markdown (Story Bible)</button>
              <button className="menu-item" role="menuitem" onClick={() => void doExport("graphml")}>GraphML (relations)</button>
              <div className="menu-sep" />
              <button className="menu-item" role="menuitem" onClick={() => void doImport()}>Import a JSON export…</button>
            </div>
          )}
        </div>
      </header>

      <div className="inspector-filters">
        <input type="search" placeholder="Search everything" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Search" />
        <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="Kind">
          <option value="">All kinds</option>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {k} ({counts.get(k) ?? 0})
            </option>
          ))}
        </select>
        <select value={state} onChange={(e) => setState(e.target.value as typeof state)} aria-label="Status">
          <option value="">Canon and inferred</option>
          <option value="canon">Canon</option>
          <option value="inferred">Inferred</option>
        </select>
        <select value={by} onChange={(e) => setBy(e.target.value as typeof by)} aria-label="Created by">
          <option value="">By anyone</option>
          <option value="author">By you</option>
          <option value="ai">By the model</option>
        </select>
      </div>

      <div className="inspector-bulk" aria-live="polite">
        <span className="muted">{picked.size > 0 ? `${picked.size} selected` : "Select rows for bulk actions"}</span>
        <button disabled={busy || inferredItems.length === 0} onClick={() => void run("Could not make canon", () => memory.makeCanon(inferredItems))}>
          ✓ Make canon
        </button>
        <button
          disabled={busy || inferredItems.length === 0}
          onClick={() =>
            void run("Could not delete", async () => {
              const n = await memory.deleteInferred(inferredItems);
              workbench.notify({ message: `Deleted ${n} inferred record${n === 1 ? "" : "s"}. They return if the text still says so and the scene is read again.` });
            })
          }
        >
          Delete inferred
        </button>
        <button
          disabled={busy || sources.length === 0}
          title="Read the scenes these records came from again"
          onClick={() => void run("Could not re-read", () => Promise.all(sources.map((d) => memory.reindexScene(d))))}
        >
          Re-extract {sources.length > 0 ? `(${sources.length} scene${sources.length === 1 ? "" : "s"})` : ""}
        </button>
      </div>

      <div className="inspector-table ai-scroll">
        <table className="ai-table">
          <thead>
            <tr>
              <th>
                <input
                  type="checkbox"
                  aria-label="Select all shown"
                  checked={shown.length > 0 && shown.slice(0, SHOWN).every((r) => picked.has(key(r)))}
                  onChange={(e) => setPicked(e.target.checked ? new Set(shown.slice(0, SHOWN).map(key)) : new Set())}
                />
              </th>
              <th>Kind</th>
              <th>Record</th>
              <th>Detail</th>
              <th>Status</th>
              <th>Source</th>
              <th>Updated</th>
              <th>By</th>
            </tr>
          </thead>
          <tbody>
            {shown.slice(0, SHOWN).map((r) => (
              <tr key={key(r)} className={r.status === "inferred" ? "inferred" : ""}>
                <td>
                  <input
                    type="checkbox"
                    aria-label={`Select ${r.label}`}
                    checked={picked.has(key(r))}
                    onChange={(e) => {
                      const next = new Set(picked);
                      if (e.target.checked) next.add(key(r));
                      else next.delete(key(r));
                      setPicked(next);
                    }}
                  />
                </td>
                <td className="muted">{r.kind}</td>
                <td>
                  <button className={`link${r.status === "inferred" ? " mem-inferred-text" : ""}`} onClick={() => open_(r)}>
                    {r.label.length > 90 ? `${r.label.slice(0, 90)}…` : r.label}
                  </button>
                  {r.entityName && r.kind !== "entity" && r.kind !== "thread" && <span className="muted"> · {r.entityName}</span>}
                </td>
                <td className="ai-preview" title={r.detail}>
                  {r.detail}
                </td>
                <td>{r.status ? <StatusMark status={r.status} /> : <span className="muted">derived</span>}</td>
                <td>
                  {r.source ? (
                    <button
                      className="link"
                      title="Show the passage"
                      onClick={() => void openSource({ doc: r.source!.doc, para: r.source!.para ?? null, block: r.source!.block ?? null, quote: r.source!.quote })}
                    >
                      passage
                    </button>
                  ) : (
                    <span className="muted">–</span>
                  )}
                </td>
                <td className="muted">{r.updated.slice(0, 10)}</td>
                <td className="muted">{r.createdBy === "ai" ? "model" : r.createdBy === "author" ? "you" : "–"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {shown.length === 0 && <p className="muted">{rows.length === 0 ? "Nothing yet. Write, then let Margin read your scenes while you rest." : "Nothing matches these filters."}</p>}
        {shown.length > SHOWN && <p className="muted">Showing the first {SHOWN} of {shown.length}. Narrow the filters to see the rest.</p>}
      </div>
      <p className="muted inspector-foot">
        Every record here is a plain file in your project (<code>bible/</code> and <code>memory/</code>). Why did the model say something? Open the{" "}
        <button className="link" onClick={() => workbench.openView(CONTEXT_VIEW)}>
          context inspector
        </button>
        .
      </p>
    </div>
  );
}
