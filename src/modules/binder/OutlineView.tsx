/**
 * Corkboard / Outline (SPEC §11.1): the children of a container as index
 * cards or rows, with the author's synopsis, status and word counts. The
 * synopsis is editable in place. (The AI scene summary joins in M4.)
 */
import { useEffect, useState } from "react";

import {
  findNode,
  story,
  uiState,
  useBinderTree,
  useProject,
  useUiState,
  workbench,
  type BinderNode,
  type WorkspaceViewProps,
} from "@margin/sdk";

type Mode = "cards" | "outline";

const fmt = (n: number) => n.toLocaleString("en-US");

export function OutlineView({ resourceId }: WorkspaceViewProps) {
  const tree = useBinderTree();
  const mode = useUiState<Mode>("binder.outlineMode") ?? "cards";
  let title = "Manuscript";
  let nodes: BinderNode[] = [];
  if (resourceId?.startsWith("section:")) {
    const s = tree?.sections.find((x) => `section:${x.section}` === resourceId);
    nodes = s?.nodes ?? [];
  } else if (resourceId) {
    const n = findNode(tree, resourceId);
    title = n?.title ?? "";
    nodes = n?.children ?? [];
  }
  return (
    <div className="outline-view">
      <div className="view-toolbar">
        <h2>{title}</h2>
        <div className="segmented" role="radiogroup" aria-label="Layout">
          {(["cards", "outline"] as const).map((m) => (
            <button key={m} role="radio" aria-checked={mode === m} className={mode === m ? "active" : ""} onClick={() => uiState.set("binder.outlineMode", m)}>
              {m === "cards" ? "Corkboard" : "Outline"}
            </button>
          ))}
        </div>
      </div>
      {nodes.length === 0 && <p className="muted pad">Nothing inside yet.</p>}
      {mode === "cards" ? (
        <div className="cards">
          {nodes.map((n) => (
            <Card key={n.id} node={n} />
          ))}
        </div>
      ) : (
        <table className="outline-table">
          <thead>
            <tr>
              <th>Title</th>
              <th>Synopsis</th>
              <th>Status</th>
              <th className="num">Words</th>
              <th className="num">Target</th>
            </tr>
          </thead>
          <tbody>
            {nodes.map((n) => (
              <OutlineRows key={n.id} node={n} depth={0} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function useSynopsis(id: string) {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    story.documents
      .meta(id)
      .then((m) => live && setText(m.synopsis ?? ""))
      .catch(() => live && setText(""));
    return () => {
      live = false;
    };
  }, [id]);
  const save = (value: string) =>
    story.documents.updateMeta(id, { synopsis: value }).catch((e: unknown) =>
      workbench.notify({ message: `Could not save the synopsis: ${e instanceof Error ? e.message : String(e)}` }),
    );
  return [text, setText, save] as const;
}

function Progress({ node }: { node: BinderNode }) {
  const words = node.children.length > 0 ? node.totalWordCount : node.wordCount;
  if (!node.target) return <span className="muted">{fmt(words)} words</span>;
  const pct = Math.min(100, Math.round((words / node.target) * 100));
  return (
    <span className="card-progress" title={`${pct}% of ${fmt(node.target)}`}>
      <span className="bar">
        <span style={{ width: `${pct}%` }} />
      </span>
      {fmt(words)} / {fmt(node.target)}
    </span>
  );
}

function Card({ node }: { node: BinderNode }) {
  const [text, setText, save] = useSynopsis(node.id);
  const project = useProject();
  const label = project?.labels.find((l) => node.labels.includes(l.name));
  return (
    <article className="card" style={label ? { borderTopColor: label.color } : undefined}>
      <header>
        <button className="card-title link" onClick={() => workbench.openDocument(node.id)}>
          {node.title}
        </button>
        {node.status && <span className="pill">{node.status}</span>}
      </header>
      <textarea
        className="card-synopsis"
        aria-label={`Synopsis of ${node.title}`}
        placeholder="Synopsis…"
        value={text ?? ""}
        disabled={text === null}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => void save(e.target.value)}
      />
      <footer>
        <Progress node={node} />
        {node.children.length > 0 && <span className="muted">{node.children.length} inside</span>}
      </footer>
    </article>
  );
}

function OutlineRows({ node, depth }: { node: BinderNode; depth: number }) {
  const [text, setText, save] = useSynopsis(node.id);
  return (
    <>
      <tr>
        <td style={{ paddingLeft: 8 + depth * 16 }}>
          <button className="link" onClick={() => workbench.openDocument(node.id)}>
            {node.title}
          </button>
        </td>
        <td>
          <input
            className="outline-synopsis"
            aria-label={`Synopsis of ${node.title}`}
            value={text ?? ""}
            placeholder="—"
            onChange={(e) => setText(e.target.value)}
            onBlur={(e) => void save(e.target.value)}
          />
        </td>
        <td>{node.status ?? ""}</td>
        <td className="num">{fmt(node.children.length > 0 ? node.totalWordCount : node.wordCount)}</td>
        <td className="num">{node.target ? fmt(node.target) : ""}</td>
      </tr>
      {node.children.map((c) => (
        <OutlineRows key={c.id} node={c} depth={depth + 1} />
      ))}
    </>
  );
}
