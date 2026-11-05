/**
 * Per-document properties (SPEC §11.1): status, labels, POV, word target,
 * author synopsis, include-in-export and the per-document AI switch. Text
 * fields save on blur; toggles save immediately.
 */
import { useEffect, useState } from "react";

import {
  events,
  story,
  useActiveDocumentId,
  useProject,
  workbench,
  type DocMeta,
  type DocMetaPatch,
  type SidebarPanelProps,
} from "@margin/sdk";

const KIND_LABEL: Record<string, string> = { part: "Part", chapter: "Chapter", scene: "Scene", folder: "Folder", note: "Note" };

export function PropertiesPanel({ visible }: SidebarPanelProps) {
  const docId = useActiveDocumentId();
  const project = useProject();
  const [loaded, setMeta] = useState<DocMeta | null>(null);
  const meta = visible && loaded && loaded.id === docId ? loaded : null;
  const [draft, setDraft] = useState({ pov: "", target: "", synopsis: "", label: "" });

  useEffect(() => {
    if (!docId || !visible) return;
    let live = true;
    const load = () =>
      story.documents
        .meta(docId)
        .then((m) => {
          if (!live) return;
          setMeta(m);
          setDraft({ pov: m.pov ?? "", target: m.target ? String(m.target) : "", synopsis: m.synopsis ?? "", label: "" });
        })
        .catch(() => live && setMeta(null));
    void load();
    const off = events.on("binder.changed", (e) => {
      if (e.docIds.includes(docId) && document.activeElement?.closest(".properties") === null) void load();
    });
    return () => {
      live = false;
      off();
    };
  }, [docId, visible]);

  if (!docId || !meta) {
    return (
      <div className="panel properties">
        <p className="muted pad">Open a document to see its properties.</p>
      </div>
    );
  }

  async function save(patch: Partial<DocMetaPatch>) {
    if (!docId) return;
    try {
      setMeta(await story.documents.updateMeta(docId, patch));
    } catch (e) {
      workbench.notify({ message: `Could not save: ${e instanceof Error ? e.message : String(e)}` });
    }
  }

  const statuses = project?.statuses ?? [];
  const labelDefs = project?.labels ?? [];
  const colorOf = (name: string) => labelDefs.find((l) => l.name === name)?.color;

  return (
    <div className="panel properties">
      <div className="panel-toolbar">
        <span className="panel-title">{KIND_LABEL[meta.kind] ?? "Document"}</span>
        <span className="muted">{meta.wordCount.toLocaleString("en-US")} words</span>
      </div>
      <div className="prop-grid">
        <label htmlFor="prop-status">Status</label>
        <select id="prop-status" value={meta.status ?? ""} onChange={(e) => void save({ status: e.target.value })}>
          <option value="">—</option>
          {statuses.map((s) => (
            <option key={s} value={s}>
              {s[0]?.toUpperCase() + s.slice(1)}
            </option>
          ))}
        </select>

        <label htmlFor="prop-pov">POV</label>
        <input
          id="prop-pov"
          value={draft.pov}
          placeholder="Point of view character"
          onChange={(e) => setDraft({ ...draft, pov: e.target.value })}
          onBlur={() => draft.pov !== (meta.pov ?? "") && void save({ pov: draft.pov })}
        />

        <label htmlFor="prop-target">Target</label>
        <input
          id="prop-target"
          inputMode="numeric"
          value={draft.target}
          placeholder="Words"
          onChange={(e) => setDraft({ ...draft, target: e.target.value.replace(/[^0-9]/g, "") })}
          onBlur={() => {
            const n = Number(draft.target || 0);
            if (n !== (meta.target ?? 0)) void save({ target: n });
          }}
        />
        {meta.target ? (
          <div className="prop-full">
            <span className="bar">
              <span style={{ width: `${Math.min(100, Math.round((meta.wordCount / meta.target) * 100))}%` }} />
            </span>
          </div>
        ) : null}

        <label>Labels</label>
        <div className="prop-labels">
          {meta.labels.map((l) => (
            <span key={l} className="pill" style={colorOf(l) ? { borderColor: colorOf(l) } : undefined}>
              {l}
              <button className="pill-x" aria-label={`Remove label ${l}`} onClick={() => void save({ labels: meta.labels.filter((x) => x !== l) })}>
                ×
              </button>
            </span>
          ))}
          <input
            className="prop-label-input"
            value={draft.label}
            placeholder="Add label"
            list="label-defs"
            aria-label="Add label"
            onChange={(e) => setDraft({ ...draft, label: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter" && draft.label.trim()) {
                const l = draft.label.trim();
                setDraft({ ...draft, label: "" });
                if (!meta.labels.includes(l)) void save({ labels: [...meta.labels, l] });
              }
            }}
          />
          <datalist id="label-defs">
            {labelDefs.map((l) => (
              <option key={l.name} value={l.name} />
            ))}
          </datalist>
        </div>

        <label htmlFor="prop-synopsis" className="prop-full-label">
          Synopsis
        </label>
        <textarea
          id="prop-synopsis"
          className="prop-full"
          rows={5}
          value={draft.synopsis}
          placeholder="What happens here, in a sentence or two"
          onChange={(e) => setDraft({ ...draft, synopsis: e.target.value })}
          onBlur={() => draft.synopsis !== (meta.synopsis ?? "") && void save({ synopsis: draft.synopsis })}
        />

        <label className="prop-check prop-full">
          <input type="checkbox" checked={meta.include} onChange={(e) => void save({ include: e.target.checked })} />
          Include in export
        </label>
        <label className="prop-check prop-full">
          <input type="checkbox" checked={meta.ai === "off"} onChange={(e) => void save({ ai: e.target.checked ? "off" : "on" })} />
          AI off for this document
        </label>
      </div>
      <p className="muted pad small">File: {meta.file}</p>
    </div>
  );
}
