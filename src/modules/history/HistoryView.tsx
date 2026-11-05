/**
 * Per-document timeline (SPEC §11.3): every snapshot in which the document
 * changed, a word diff against the current text or any other version
 * ("Compare with…"), restore of the whole document (undoable), and restore
 * of a selection by inserting it at the cursor.
 */
import { useEffect, useMemo, useState } from "react";

import {
  editorHost,
  events,
  story,
  workbench,
  type DocAtSnapshot,
  type DocDiff,
  type DocVersion,
  type PmNode,
  type WorkspaceViewProps,
} from "@margin/sdk";

import { takeSnapshot } from "./actions";

const CURRENT = "current";
const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

const KIND_LABEL: Record<string, string> = { session: "Session start", auto: "Automatic", named: "Named", safety: "Before a change" };

function when(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString("en-US", { month: "short", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit" });
}

/** Paragraph texts of a document, for the read-only preview. */
function paragraphs(n: PmNode): { kind: string; text: string }[] {
  const out: { kind: string; text: string }[] = [];
  const text = (x: PmNode): string =>
    x.text ?? (x.type === "hardBreak" ? "\n" : x.type === "footnote" ? "" : (x.content ?? []).map(text).join(""));
  const walk = (x: PmNode) => {
    if (x.type === "paragraph" || x.type === "heading") out.push({ kind: x.type, text: text(x) });
    else if (x.type === "sceneBreak") out.push({ kind: "break", text: "" });
    else for (const c of x.content ?? []) walk(c);
  };
  walk(n);
  return out;
}

export function HistoryView({ resourceId }: WorkspaceViewProps) {
  const docId = resourceId ?? "";
  const [versions, setVersions] = useState<DocVersion[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [against, setAgainst] = useState<string>(CURRENT);
  const [mode, setMode] = useState<"diff" | "text">("diff");
  const [diff, setDiff] = useState<DocDiff | null>(null);
  const [preview, setPreview] = useState<DocAtSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => events.on("history.snapshot", () => setTick((t) => t + 1)), []);

  useEffect(() => {
    if (!docId) return;
    let live = true;
    void editorHost
      .flushAll()
      .then(() => story.history.versions(docId))
      .then((v) => {
        if (!live) return;
        setVersions(v);
        setSelected((s) => s ?? v[0]?.snapshot.id ?? null);
      })
      .catch((e: unknown) => live && setError(errorMessage(e)));
    return () => {
      live = false;
    };
  }, [docId, tick]);

  useEffect(() => {
    if (!selected || !docId) return;
    let live = true;
    const ok = () => live && setError(null);
    if (mode === "diff") {
      // Old on the left of the comparison: the selected version, then the other one.
      const to = against === CURRENT ? null : against;
      story.history
        .diff(docId, selected, to)
        .then((d) => {
          ok();
          if (live) setDiff(d);
        })
        .catch((e: unknown) => live && setError(errorMessage(e)));
    } else {
      story.history
        .at(docId, selected)
        .then((p) => {
          ok();
          if (live) setPreview(p);
        })
        .catch((e: unknown) => live && setError(errorMessage(e)));
    }
    return () => {
      live = false;
    };
  }, [docId, selected, against, mode, tick]);

  const sel = useMemo(() => versions?.find((v) => v.snapshot.id === selected), [versions, selected]);

  async function restore() {
    if (!sel) return;
    try {
      await editorHost.flushAll();
      const safety = await story.history.restore(sel.snapshot.id, [docId], `Before restoring “${sel.title}” to ${when(sel.snapshot.time)}`);
      workbench.notify({
        message: `Restored the version from ${when(sel.snapshot.time)}`,
        ...(safety
          ? {
              action: {
                label: "Undo",
                run: () =>
                  void story.history
                    .restore(safety.id, [docId], "Undo restore")
                    .catch((e: unknown) => workbench.notify({ message: `Could not undo: ${errorMessage(e)}` })),
              },
            }
          : {}),
        timeoutMs: 15_000,
      });
    } catch (e) {
      workbench.notify({ message: `Could not restore: ${errorMessage(e)}` });
    }
  }

  function insertSelection() {
    const text = window.getSelection()?.toString() ?? "";
    if (!text.trim()) {
      workbench.notify({ message: "Select some text in the old version first." });
      return;
    }
    const target = editorHost.forDoc(docId)[0];
    if (!target) {
      workbench.openDocument(docId);
      workbench.notify({ message: "The document is open now. Put the cursor where the text should go, then try again." });
      return;
    }
    target.insertText(text);
    workbench.notify({ message: "Inserted at the cursor. Ctrl+Z in the document undoes it." });
  }

  if (!docId) return null;
  return (
    <div className="history-view">
      <aside className="history-list" aria-label="Versions">
        <div className="panel-toolbar">
          <span className="panel-title">Versions</span>
          <button onClick={() => void takeSnapshot()} title="Take a named snapshot (Ctrl+Alt+S)">
            Snapshot…
          </button>
        </div>
        {versions === null && !error && <p className="muted pad">Loading…</p>}
        {versions?.length === 0 && <p className="muted pad">No snapshots yet. One is taken every session and every 15 minutes of writing.</p>}
        <ol>
          {versions?.map((v) => (
            <li key={v.snapshot.id}>
              <button
                className={`version${v.snapshot.id === selected ? " selected" : ""}`}
                aria-current={v.snapshot.id === selected}
                onClick={() => setSelected(v.snapshot.id)}
              >
                <span className="version-time">{when(v.snapshot.time)}</span>
                <span className="version-name">
                  {v.snapshot.kind === "named" || v.snapshot.kind === "safety" ? v.snapshot.name : KIND_LABEL[v.snapshot.kind]}
                </span>
                <span className="muted small">{v.wordCount.toLocaleString("en-US")} words</span>
              </button>
            </li>
          ))}
        </ol>
      </aside>
      <section className="history-detail">
        {sel && (
          <div className="view-toolbar">
            <div className="segmented" role="radiogroup" aria-label="Show">
              <button role="radio" aria-checked={mode === "diff"} className={mode === "diff" ? "active" : ""} onClick={() => setMode("diff")}>
                Changes
              </button>
              <button role="radio" aria-checked={mode === "text"} className={mode === "text" ? "active" : ""} onClick={() => setMode("text")}>
                Text
              </button>
            </div>
            {mode === "diff" && (
              <label className="small">
                Compare with{" "}
                <select value={against} onChange={(e) => setAgainst(e.target.value)} aria-label="Compare with">
                  <option value={CURRENT}>the current text</option>
                  {versions
                    ?.filter((v) => v.snapshot.id !== selected)
                    .map((v) => (
                      <option key={v.snapshot.id} value={v.snapshot.id}>
                        {when(v.snapshot.time)}
                        {v.snapshot.kind === "named" ? ` — ${v.snapshot.name}` : ""}
                      </option>
                    ))}
                </select>
              </label>
            )}
            <span className="spacer" />
            {mode === "text" && <button onClick={insertSelection}>Insert selection at cursor</button>}
            <button className="primary" onClick={() => void restore()}>
              Restore this version
            </button>
          </div>
        )}
        {error && <p className="error pad">{error}</p>}
        {mode === "diff" && diff && sel && (
          <>
            <p className="muted pad small">
              From {when(sel.snapshot.time)} to {against === CURRENT ? "now" : when(versions?.find((v) => v.snapshot.id === against)?.snapshot.time ?? "")}:{" "}
              <ins>+{diff.wordsAdded.toLocaleString("en-US")}</ins> <del>−{diff.wordsRemoved.toLocaleString("en-US")}</del> words
            </p>
            <div className="diff-text page-like">
              {diff.chunks.length === 0 || diff.chunks.every((c) => c.kind === "equal") ? (
                <p className="muted">No differences.</p>
              ) : null}
              {diff.chunks.map((c, i) =>
                c.kind === "equal" ? <span key={i}>{c.text}</span> : c.kind === "insert" ? <ins key={i}>{c.text}</ins> : <del key={i}>{c.text}</del>,
              )}
            </div>
          </>
        )}
        {mode === "text" && preview && (
          <div className="history-preview page-like">
            <h2>{preview.title}</h2>
            {paragraphs(preview.content).map((p, i) =>
              p.kind === "break" ? (
                <hr key={i} className="scene-break" />
              ) : p.kind === "heading" ? (
                <h3 key={i}>{p.text}</h3>
              ) : (
                <p key={i}>{p.text}</p>
              ),
            )}
          </div>
        )}
      </section>
    </div>
  );
}
