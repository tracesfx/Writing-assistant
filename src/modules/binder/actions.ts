import {
  editorHost,
  findNode,
  locateNode,
  story,
  useProjectStore,
  useWorkbench,
  workbench,
  type BinderTarget,
  type DocKind,
  type PmNode,
} from "@margin/sdk";

import { useBinderUi } from "./state";

const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Where a new node of `kind` should go, relative to the current selection. */
export function targetForNew(kind: DocKind): BinderTarget {
  const tree = useProjectStore.getState().tree;
  const anchor = useBinderUi.getState().selection.at(-1) ?? workbench.activeDocumentId();
  const node = anchor ? findNode(tree, anchor) : undefined;
  const loc = anchor ? locateNode(tree, anchor) : undefined;
  if (node && loc) {
    if (kind === "scene" || kind === "note") {
      // Inside a selected container, after a selected leaf.
      if (node.kind === "chapter" || node.kind === "folder") {
        return { section: loc.section, parent: node.id, index: null };
      }
      return { section: loc.section, parent: loc.parent, index: loc.index + 1 };
    }
    if (kind === "chapter" && loc.section === "manuscript") {
      if (node.kind === "part") return { section: "manuscript", parent: node.id, index: null };
      if (node.kind === "chapter") return { section: "manuscript", parent: loc.parent, index: loc.index + 1 };
      // A scene: after its chapter.
      const chapter = loc.parent ? locateNode(tree, loc.parent) : undefined;
      if (chapter) return { section: "manuscript", parent: chapter.parent, index: chapter.index + 1 };
    }
  }
  if (kind === "scene") {
    // Into the last chapter of the manuscript, if there is one.
    const ms = tree?.sections.find((s) => s.section === "manuscript");
    const last = ms?.nodes.at(-1);
    if (last && (last.kind === "chapter" || last.kind === "part")) {
      const lastChapter = last.kind === "part" ? (last.children.at(-1) ?? last) : last;
      return { section: "manuscript", parent: lastChapter.id, index: null };
    }
  }
  const section = kind === "note" ? "notes" : "manuscript";
  return { section, parent: null, index: null };
}

export async function createNode(kind: DocKind, target = targetForNew(kind)): Promise<void> {
  try {
    const meta = await story.documents.create(kind, "", target);
    useBinderUi.getState().expand(target.parent);
    useBinderUi.getState().select([meta.id]);
    useBinderUi.getState().startRename(meta.id);
    if (kind !== "chapter" && kind !== "part" && kind !== "folder") workbench.openDocument(meta.id);
  } catch (e) {
    workbench.notify({ message: `Could not create: ${errorMessage(e)}` });
  }
}

/** Ids of a node and everything inside it. */
function subtreeIds(id: string): string[] {
  const node = findNode(useProjectStore.getState().tree, id);
  if (!node) return [id];
  const out: string[] = [];
  const walk = (n: typeof node) => {
    out.push(n.id);
    n.children.forEach(walk);
  };
  walk(node);
  return out;
}

/**
 * Move to the Trash, with an Undo notice. Tabs showing trashed documents
 * are closed (the Trash is for recovery, not editing); undo does not
 * reopen them.
 */
export async function trashNodes(ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  try {
    await editorHost.flushAll();
    const affected = ids.flatMap(subtreeIds);
    await story.binder.trash(ids);
    for (const id of affected) useWorkbench.getState().closeResource(id);
    useBinderUi.getState().select([]);
    const n = ids.length;
    workbench.notify({
      message: n === 1 ? "Moved to Trash" : `Moved ${n} items to Trash`,
      action: { label: "Undo", run: () => void restoreNodes(ids) },
      timeoutMs: 10_000,
    });
  } catch (e) {
    workbench.notify({ message: `Could not move to Trash: ${errorMessage(e)}` });
  }
}

export async function restoreNodes(ids: string[]): Promise<void> {
  try {
    await story.binder.restore(ids);
    useBinderUi.getState().select(ids);
  } catch (e) {
    workbench.notify({ message: `Could not restore: ${errorMessage(e)}` });
  }
}

/** Move nodes, with an Undo notice that puts each back where it was. */
export async function moveNodes(ids: string[], target: BinderTarget): Promise<void> {
  const tree = useProjectStore.getState().tree;
  const origins = ids
    .map((id) => ({ id, loc: locateNode(tree, id) }))
    .filter((x): x is { id: string; loc: NonNullable<typeof x.loc> } => x.loc !== undefined);
  try {
    await story.binder.move(ids, target);
    useBinderUi.getState().expand(target.parent);
    workbench.notify({
      message: ids.length === 1 ? "Moved" : `Moved ${ids.length} items`,
      action: {
        label: "Undo",
        run: () => {
          void (async () => {
            // Restore in original order so earlier indices are valid for later ones.
            const sorted = [...origins].sort((a, b) => a.loc.index - b.loc.index);
            for (const o of sorted) {
              // The backend reads `index` as a drop position counted before the
              // node is taken out, so a node now sitting earlier in the same
              // parent must aim one further to land at its old index.
              const now = locateNode(await story.binder.tree(), o.id);
              const sameParent = now !== undefined && now.section === o.loc.section && now.parent === o.loc.parent;
              const index = sameParent && now.index < o.loc.index ? o.loc.index + 1 : o.loc.index;
              await story.binder.move([o.id], { section: o.loc.section, parent: o.loc.parent, index });
            }
          })().catch((e: unknown) => workbench.notify({ message: `Could not undo the move: ${errorMessage(e)}` }));
        },
      },
      timeoutMs: 8000,
    });
  } catch (e) {
    workbench.notify({ message: `Could not move: ${errorMessage(e)}` });
  }
}

export async function renameNode(id: string, title: string): Promise<void> {
  const t = title.trim();
  if (!t) return;
  try {
    await story.documents.updateMeta(id, { title: t });
  } catch (e) {
    workbench.notify({ message: `Could not rename: ${errorMessage(e)}` });
  }
}

export async function duplicateNodes(ids: string[]): Promise<void> {
  if (ids.length === 0) return;
  try {
    await editorHost.flushAll();
    const created = await story.binder.duplicate(ids);
    useBinderUi.getState().select(created);
    workbench.notify({
      message: created.length === 1 ? "Duplicated" : `Duplicated ${created.length} items`,
      action: { label: "Undo", run: () => void story.binder.trash(created) },
      timeoutMs: 8000,
    });
  } catch (e) {
    workbench.notify({ message: `Could not duplicate: ${errorMessage(e)}` });
  }
}

/**
 * Merge the selected documents into the first one (SPEC §11.1). Undo puts
 * the first document's old text back and restores the others from the Trash.
 */
export async function mergeNodes(ids: string[]): Promise<void> {
  if (ids.length < 2) {
    workbench.notify({ message: "Select two or more documents to merge." });
    return;
  }
  try {
    await editorHost.flushAll();
    const before = new Map<string, PmNode>();
    for (const id of ids) before.set(id, (await story.documents.get(id)).content);
    const r = await story.documents.merge(ids);
    for (const id of r.trashed) useWorkbench.getState().closeResource(id);
    useBinderUi.getState().select([r.target]);
    workbench.notify({
      message: `Merged ${ids.length} documents`,
      action: {
        label: "Undo",
        run: () => {
          void (async () => {
            const old = before.get(r.target);
            if (old) await story.documents.replace(r.target, old);
            await story.binder.restore(r.trashed);
          })().catch((e: unknown) => workbench.notify({ message: `Could not undo the merge: ${errorMessage(e)}` }));
        },
      },
      timeoutMs: 12_000,
    });
  } catch (e) {
    workbench.notify({ message: `Could not merge: ${errorMessage(e)}` });
  }
}

export const OUTLINE_VIEW = "binder.outline";

/** Open the corkboard for a container (default: the selection, else the manuscript). */
export function openOutline(id?: string): void {
  const tree = useProjectStore.getState().tree;
  const sel = id ?? useBinderUi.getState().selection[0];
  const node = sel ? findNode(tree, sel) : undefined;
  const target = node && node.children.length > 0 ? node.id : "section:manuscript";
  workbench.openView(OUTLINE_VIEW, target);
}

/** Open a document in the split pane, opening the split if needed. */
export function openInSplit(id: string): void {
  const ws = useWorkbench.getState();
  if (!ws.panes.some((p) => p.id === "right")) ws.toggleSplit();
  workbench.openDocument(id, { pane: "right" });
}
