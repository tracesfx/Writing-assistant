/**
 * Reactive project data for modules: project info and the binder tree,
 * kept fresh from domain events.
 */
import { create } from "zustand";

import type { BinderNode, BinderTree, ProjectInfo, Section } from "./bindings";
import { events } from "./events";
import { story } from "./story";

interface ProjectState {
  info: ProjectInfo | null;
  tree: BinderTree | null;
  setInfo(info: ProjectInfo | null): void;
  refreshTree(): Promise<void>;
}

export const useProjectStore = create<ProjectState>((set) => ({
  info: null,
  tree: null,
  setInfo(info) {
    set({ info, tree: info ? useProjectStore.getState().tree : null });
  },
  async refreshTree() {
    try {
      const tree = await story.binder.tree();
      set({ tree });
    } catch (err) {
      console.error("[margin] could not load binder", err);
    }
  },
}));

let refreshTimer: ReturnType<typeof setTimeout> | undefined;
function scheduleRefresh(): void {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => void useProjectStore.getState().refreshTree(), 60);
}

let wired = false;
/** Called once by the shell. */
export function wireProjectEvents(): void {
  if (wired) return;
  wired = true;
  events.on("binder.changed", scheduleRefresh);
  events.on("doc.saved", scheduleRefresh);
  events.on("project.opened", () => {
    void story.project.get().then((info) => useProjectStore.getState().setInfo(info));
    scheduleRefresh();
  });
  events.on("project.closed", () => useProjectStore.setState({ info: null, tree: null }));
  events.on("project.settingsChanged", () => {
    void story.project.get().then((info) => useProjectStore.getState().setInfo(info));
  });
}

export const useProject = () => useProjectStore((s) => s.info);
export const useBinderTree = () => useProjectStore((s) => s.tree);

/** Find a node anywhere in the tree (sections and trash). */
export function findNode(tree: BinderTree | null, id: string): BinderNode | undefined {
  if (!tree) return undefined;
  const walk = (nodes: BinderNode[]): BinderNode | undefined => {
    for (const n of nodes) {
      if (n.id === id) return n;
      const f = walk(n.children);
      if (f) return f;
    }
    return undefined;
  };
  for (const s of tree.sections) {
    const f = walk(s.nodes);
    if (f) return f;
  }
  return walk(tree.trash.map((t) => t.node));
}

/** Section, parent id and index of a node. */
export function locateNode(
  tree: BinderTree | null,
  id: string,
): { section: Section; parent: string | null; index: number } | undefined {
  if (!tree) return undefined;
  const walk = (
    nodes: BinderNode[],
    section: Section,
    parent: string | null,
  ): { section: Section; parent: string | null; index: number } | undefined => {
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i]!;
      if (n.id === id) return { section, parent, index: i };
      const f = walk(n.children, section, n.id);
      if (f) return f;
    }
    return undefined;
  };
  for (const s of tree.sections) {
    const f = walk(s.nodes, s.section, null);
    if (f) return f;
  }
  return undefined;
}

export interface FlatNode {
  node: BinderNode;
  section: Section | "trash";
  depth: number;
  /** Titles of the ancestors, outermost first. */
  path: string[];
}

/** Every node in binder order (sections, then the Trash if asked). */
export function flattenTree(tree: BinderTree | null, includeTrash = false): FlatNode[] {
  const out: FlatNode[] = [];
  if (!tree) return out;
  const walk = (nodes: BinderNode[], section: FlatNode["section"], depth: number, path: string[]) => {
    for (const n of nodes) {
      out.push({ node: n, section, depth, path });
      walk(n.children, section, depth + 1, [...path, n.title]);
    }
  };
  for (const s of tree.sections) walk(s.nodes, s.section, 0, []);
  if (includeTrash) walk(tree.trash.map((t) => t.node), "trash", 0, []);
  return out;
}

export interface ChapterGroup {
  /** The chapter's id (or the part's, or the scene's own when it sits at the top). */
  id: string;
  title: string;
  /** Position among the groups, in binder order. */
  order: number;
}

/**
 * Which chapter each manuscript document belongs to: its nearest enclosing
 * chapter, else its part, else itself. For the Insights views that roll scenes
 * up into chapters.
 */
export function chapterGroups(tree: BinderTree | null): Map<string, ChapterGroup> {
  const out = new Map<string, ChapterGroup>();
  const groups = new Map<string, ChapterGroup>();
  const group = (n: BinderNode): ChapterGroup => {
    let g = groups.get(n.id);
    if (!g) {
      g = { id: n.id, title: n.title, order: groups.size };
      groups.set(n.id, g);
    }
    return g;
  };
  const walk = (nodes: BinderNode[], owner: BinderNode | null) => {
    for (const n of nodes) {
      out.set(n.id, group(n.kind === "chapter" ? n : (owner ?? n)));
      walk(n.children, n.kind === "chapter" || n.kind === "part" ? n : owner);
    }
  };
  const ms = tree?.sections.find((s) => s.section === "manuscript");
  if (ms) walk(ms.nodes, null);
  return out;
}
