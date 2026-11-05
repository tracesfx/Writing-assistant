import { useEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent } from "react";

const LIST_ID = "binder-tree";
const listEl = () => document.getElementById(LIST_ID);

import {
  useActiveDocumentId,
  useBinderTree,
  workbench,
  type BinderNode,
  type BinderTarget,
  type Section,
  type SidebarPanelProps,
} from "@margin/sdk";

import {
  createNode,
  duplicateNodes,
  mergeNodes,
  moveNodes,
  openInSplit,
  openOutline,
  renameNode,
  restoreNodes,
  trashNodes,
} from "./actions";
import { useBinderUi } from "./state";

const SECTION_TITLES: Record<Section, string> = {
  manuscript: "Manuscript",
  frontMatter: "Front matter",
  backMatter: "Back matter",
  notes: "Notes",
  research: "Research",
};

const KIND_GLYPH: Record<string, string> = {
  part: "▤",
  chapter: "▦",
  scene: "▭",
  folder: "▱",
  note: "▯",
};

interface Row {
  node: BinderNode;
  depth: number;
  section: Section | "trash";
}

type DropPos = "before" | "after" | "inside";

const DRAG_MIME = "application/x-margin-binder";

export function BinderPanel(_props: SidebarPanelProps) {
  const tree = useBinderTree();
  const ui = useBinderUi();
  const activeDoc = useActiveDocumentId();
  const [drop, setDrop] = useState<{ id: string; pos: DropPos } | null>(null);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; row: Row } | null>(null);

  // Visible rows in order, for keyboard navigation and range selection.
  const rows = useMemo(() => {
    const out: Row[] = [];
    if (!tree) return out;
    const walk = (nodes: BinderNode[], depth: number, section: Row["section"]) => {
      for (const n of nodes) {
        out.push({ node: n, depth, section });
        if (n.children.length > 0 && !ui.collapsed[n.id]) walk(n.children, depth + 1, section);
      }
    };
    for (const s of tree.sections) if (!ui.collapsed[`section:${s.section}`]) walk(s.nodes, 0, s.section);
    if (!ui.collapsed["section:trash"]) walk(tree.trash.map((t) => t.node), 0, "trash");
    return out;
  }, [tree, ui.collapsed]);

  if (!tree) return <div className="binder" aria-busy="true" />;

  const rowIndex = (id: string) => rows.findIndex((r) => r.node.id === id);

  function onRowClick(e: React.MouseEvent, row: Row) {
    const id = row.node.id;
    if (e.shiftKey && anchor) {
      const a = rowIndex(anchor);
      const b = rowIndex(id);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        ui.select(rows.slice(lo, hi + 1).map((r) => r.node.id));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      ui.toggleSelect(id);
      setAnchor(id);
      return;
    }
    ui.select([id]);
    setAnchor(id);
    if (row.section !== "trash") workbench.openDocument(id);
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (ui.renaming) return;
    const current = ui.selection.at(-1);
    const idx = current ? rowIndex(current) : -1;
    const row = idx >= 0 ? rows[idx] : undefined;
    const go = (i: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, i))];
      if (!r) return;
      if (e.shiftKey && anchor) {
        const a = rowIndex(anchor);
        const [lo, hi] = a < i ? [a, i] : [i, a];
        ui.select(rows.slice(Math.max(lo, 0), hi + 1).map((x) => x.node.id));
      } else {
        ui.select([r.node.id]);
        setAnchor(r.node.id);
      }
      listEl()?.querySelector<HTMLElement>(`[data-id="${r.node.id}"]`)?.scrollIntoView({ block: "nearest" });
    };
    switch (e.key) {
      case "ArrowDown":
        go(idx + 1);
        break;
      case "ArrowUp":
        go(idx < 0 ? 0 : idx - 1);
        break;
      case "ArrowRight":
        if (row && row.node.children.length > 0) {
          if (ui.collapsed[row.node.id]) ui.toggleCollapsed(row.node.id);
          else go(idx + 1);
        }
        break;
      case "ArrowLeft":
        if (row && row.node.children.length > 0 && !ui.collapsed[row.node.id]) ui.collapse(row.node.id);
        else if (row) {
          // Jump to parent.
          for (let i = idx - 1; i >= 0; i--) {
            if ((rows[i]?.depth ?? 0) < row.depth) {
              go(i);
              break;
            }
          }
        }
        break;
      case "Enter":
        if (row && row.section !== "trash") workbench.openDocument(row.node.id);
        break;
      case "F2":
        if (row && row.section !== "trash") ui.startRename(row.node.id);
        break;
      case "Delete":
        if (row?.section === "trash") return;
        void trashNodes(ui.selection.filter((id) => rows.find((r) => r.node.id === id)?.section !== "trash"));
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  // --- Drag and drop -------------------------------------------------------

  function onDragStart(e: DragEvent, row: Row) {
    if (row.section === "trash") {
      e.preventDefault();
      return;
    }
    const ids = ui.selection.includes(row.node.id) ? ui.selection : [row.node.id];
    if (!ui.selection.includes(row.node.id)) ui.select(ids);
    e.dataTransfer.setData(DRAG_MIME, JSON.stringify(ids));
    e.dataTransfer.effectAllowed = "move";
  }

  function dropPosition(e: DragEvent, row: Row): DropPos {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const y = (e.clientY - rect.top) / rect.height;
    const container = row.node.kind === "part" || row.node.kind === "chapter" || row.node.kind === "folder";
    if (y < 0.25) return "before";
    if (y > 0.75) return container && !ui.collapsed[row.node.id] && row.node.children.length > 0 ? "inside" : "after";
    return container || row.node.children.length > 0 ? "inside" : y < 0.5 ? "before" : "after";
  }

  function onDragOver(e: DragEvent, row: Row) {
    if (!e.dataTransfer.types.includes(DRAG_MIME) || row.section === "trash") return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const pos = dropPosition(e, row);
    if (drop?.id !== row.node.id || drop.pos !== pos) setDrop({ id: row.node.id, pos });
  }

  function onDrop(e: DragEvent, row: Row) {
    e.preventDefault();
    setDrop(null);
    const raw = e.dataTransfer.getData(DRAG_MIME);
    if (!raw || row.section === "trash") return;
    const ids = JSON.parse(raw) as string[];
    const pos = dropPosition(e, row);
    let target: BinderTarget;
    if (pos === "inside") {
      target = { section: row.section, parent: row.node.id, index: 0 };
    } else {
      const parentRow = parentOf(row);
      const siblings = parentRow ? parentRow.node.children : (tree?.sections.find((s) => s.section === row.section)?.nodes ?? []);
      const i = siblings.findIndex((n) => n.id === row.node.id);
      target = { section: row.section, parent: parentRow?.node.id ?? null, index: pos === "before" ? i : i + 1 };
    }
    void moveNodes(ids, target);
  }

  function onSectionDrop(e: DragEvent, section: Section) {
    e.preventDefault();
    setDrop(null);
    const raw = e.dataTransfer.getData(DRAG_MIME);
    if (raw) void moveNodes(JSON.parse(raw) as string[], { section, parent: null, index: null });
  }

  function parentOf(row: Row): Row | undefined {
    const i = rowIndex(row.node.id);
    for (let j = i - 1; j >= 0; j--) if ((rows[j]?.depth ?? 0) < row.depth) return rows[j];
    return undefined;
  }

  // --- Render --------------------------------------------------------------

  const renderRows = (section: Row["section"]) =>
    rows
      .filter((r) => r.section === section)
      .map((row) => {
        const n = row.node;
        const selected = ui.selection.includes(n.id);
        const hasKids = n.children.length > 0;
        const dropCls = drop?.id === n.id ? ` drop-${drop.pos}` : "";
        return (
          <div
            key={n.id}
            data-id={n.id}
            role="treeitem"
            aria-level={row.depth + 1}
            aria-selected={selected}
            aria-expanded={hasKids ? !ui.collapsed[n.id] : undefined}
            className={`binder-row${selected ? " selected" : ""}${activeDoc === n.id ? " active" : ""}${n.include ? "" : " excluded"}${dropCls}`}
            style={{ paddingLeft: 8 + row.depth * 14 }}
            draggable={row.section !== "trash" && ui.renaming !== n.id}
            onDragStart={(e) => onDragStart(e, row)}
            onDragOver={(e) => onDragOver(e, row)}
            onDragLeave={() => setDrop(null)}
            onDrop={(e) => onDrop(e, row)}
            onClick={(e) => onRowClick(e, row)}
            onDoubleClick={() => row.section !== "trash" && ui.startRename(n.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              if (!ui.selection.includes(n.id)) ui.select([n.id]);
              setMenu({ x: e.clientX, y: e.clientY, row });
            }}
          >
            <span
              className={`twisty${hasKids ? "" : " empty"}`}
              onClick={(e) => {
                e.stopPropagation();
                if (hasKids) ui.toggleCollapsed(n.id);
              }}
              aria-hidden="true"
            >
              {hasKids ? (ui.collapsed[n.id] ? "▸" : "▾") : ""}
            </span>
            <span className={`kind kind-${n.kind}`} aria-hidden="true">
              {KIND_GLYPH[n.kind]}
            </span>
            {ui.renaming === n.id ? (
              <RenameInput
                initial={n.title}
                onDone={(value) => {
                  ui.stopRename();
                  if (value !== null && value !== n.title) void renameNode(n.id, value);
                  listEl()?.focus();
                }}
              />
            ) : (
              <span className="title">{n.title}</span>
            )}
            {row.section === "trash" ? (
              <button
                className="row-action"
                onClick={(e) => {
                  e.stopPropagation();
                  void restoreNodes([n.id]);
                }}
              >
                Restore
              </button>
            ) : (
              <span className="count">{(hasKids ? n.totalWordCount : n.wordCount).toLocaleString("en-US")}</span>
            )}
          </div>
        );
      });

  const sectionHeader = (key: string, title: string, count: number | null, section?: Section) => (
    <div
      className="binder-section"
      role="button"
      tabIndex={-1}
      aria-expanded={!ui.collapsed[key]}
      onClick={() => ui.toggleCollapsed(key)}
      onDragOver={(e) => {
        if (section && e.dataTransfer.types.includes(DRAG_MIME)) e.preventDefault();
      }}
      onDrop={(e) => section && onSectionDrop(e, section)}
    >
      <span aria-hidden="true">{ui.collapsed[key] ? "▸" : "▾"}</span> {title}
      {count !== null && <span className="count">{count.toLocaleString("en-US")}</span>}
    </div>
  );

  return (
    <div className="binder">
      <div className="panel-toolbar">
        <span className="panel-title">Binder</span>
        <button title="New scene (Ctrl+N)" onClick={() => void createNode("scene")}>
          + Scene
        </button>
        <button title="New chapter (Ctrl+Alt+N)" onClick={() => void createNode("chapter")}>
          + Chapter
        </button>
      </div>
      <div className="binder-list" role="tree" aria-label="Binder" aria-multiselectable="true" tabIndex={0} id={LIST_ID} onKeyDown={onKeyDown}>
        {tree.sections.map((s) => (
          <div key={s.section} role="group">
            {sectionHeader(`section:${s.section}`, SECTION_TITLES[s.section], s.section === "manuscript" ? s.totalWordCount : null, s.section)}
            {!ui.collapsed[`section:${s.section}`] && renderRows(s.section)}
          </div>
        ))}
        <div role="group">
          {sectionHeader("section:trash", `Trash${tree.trash.length ? ` (${tree.trash.length})` : ""}`, null)}
          {!ui.collapsed["section:trash"] && renderRows("trash")}
        </div>
      </div>
      {menu && <RowMenu menu={menu} selection={ui.selection} onClose={() => setMenu(null)} />}
    </div>
  );
}

function RowMenu({ menu, selection, onClose }: { menu: { x: number; y: number; row: Row }; selection: string[]; onClose: () => void }) {
  const { row } = menu;
  const n = row.node;
  const ids = selection.includes(n.id) ? selection : [n.id];
  const container = n.kind === "part" || n.kind === "chapter" || n.kind === "folder";
  const items: { label: string; hint?: string; run: () => void; show?: boolean }[] =
    row.section === "trash"
      ? [{ label: "Restore", run: () => void restoreNodes(ids) }]
      : [
          { label: "Open", run: () => workbench.openDocument(n.id), show: ids.length === 1 },
          { label: "Open in split pane", run: () => openInSplit(n.id), show: ids.length === 1 },
          { label: "Corkboard", run: () => openOutline(n.id), show: n.children.length > 0 },
          { label: "Rename", hint: "F2", run: () => useBinderUi.getState().startRename(n.id), show: ids.length === 1 },
          { label: "New scene here", run: () => void createNode("scene"), show: ids.length === 1 && (container || n.kind === "scene") },
          { label: ids.length > 1 ? `Duplicate ${ids.length} items` : "Duplicate", hint: "Ctrl+D", run: () => void duplicateNodes(ids) },
          { label: `Merge ${ids.length} documents`, hint: "Ctrl+Shift+M", run: () => void mergeNodes(ids), show: ids.length > 1 },
          { label: "Move to Trash", hint: "Del", run: () => void trashNodes(ids) },
        ];
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".binder-menu")) onClose();
    };
    const esc = (e: globalThis.KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [onClose]);
  return (
    <div className="popover menu binder-menu" role="menu" style={{ left: menu.x, top: menu.y }}>
      {items
        .filter((i) => i.show !== false)
        .map((i) => (
          <button
            key={i.label}
            role="menuitem"
            className="menu-item"
            onClick={() => {
              onClose();
              i.run();
            }}
          >
            <span>{i.label}</span>
            {i.hint && <kbd>{i.hint}</kbd>}
          </button>
        ))}
    </div>
  );
}

function RenameInput({ initial, onDone }: { initial: string; onDone: (value: string | null) => void }) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  const finish = (v: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(v);
  };
  return (
    <input
      className="rename"
      autoFocus
      value={value}
      aria-label="New title"
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(value);
        if (e.key === "Escape") finish(null);
      }}
      onBlur={() => finish(value)}
    />
  );
}
