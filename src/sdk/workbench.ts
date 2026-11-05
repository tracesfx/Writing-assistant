/**
 * Workbench state shared by the shell and modules: open tabs, panes,
 * the active document, and quiet notices. Modules use this instead of
 * reaching into the shell.
 */
import { create } from "zustand";

export interface Tab {
  id: string;
  viewId: string;
  resourceId?: string;
}

export type PaneId = "left" | "right";

export interface Pane {
  id: PaneId;
  tabs: Tab[];
  activeTabId?: string;
}

export interface Notice {
  id: number;
  message: string;
  action?: { label: string; run: () => void };
  /** Auto-dismiss after this many ms (default 6000). 0 = sticky. */
  timeoutMs?: number;
  /** A new notice with the same key replaces the previous one (e.g. a toggle's state). */
  key?: string;
}

/** A one-line text prompt shown by the shell (never a modal dialog). */
export interface PromptRequest {
  id: number;
  title: string;
  placeholder?: string;
  initial?: string;
  resolve: (value: string | null) => void;
}

export interface SidebarState {
  visible: boolean;
  /** Id of the panel shown (sidebars with several panels show tabs). */
  active?: string;
}

export interface WorkbenchState {
  panes: Pane[];
  focusedPane: PaneId;
  notices: Notice[];
  prompt: PromptRequest | null;
  sidebars: { left: SidebarState; right: SidebarState };
  /** Open another project folder; installed by the shell. */
  openProject: (root: string) => void;
  /** Which side each panel lives on; installed by the shell. */
  panelSide: (panelId: string) => "left" | "right" | undefined;
  /** Show a sidebar panel (and its sidebar). */
  showPanel(panelId: string): void;
  /** Show the panel, or hide its sidebar if it is already showing. */
  togglePanel(panelId: string): void;
  setSidebar(side: "left" | "right", patch: Partial<SidebarState>): void;
  /** Ask for a line of text; resolves `null` if dismissed. */
  ask(opts: { title: string; placeholder?: string; initial?: string }): Promise<string | null>;
  /** Resolves which workspace view opens a resource kind; installed by the shell. */
  viewFor: (kind: "document") => string | undefined;

  openView(viewId: string, resourceId?: string, opts?: { pane?: PaneId; newTab?: boolean }): void;
  openDocument(id: string, opts?: { pane?: PaneId; newTab?: boolean }): void;
  activateTab(tabId: string): void;
  closeTab(tabId: string): void;
  closeResource(resourceId: string): void;
  focusPane(pane: PaneId): void;
  toggleSplit(): void;
  notify(n: Omit<Notice, "id">): number;
  dismiss(id: number): void;
  /** Replace the tab layout (session restore). */
  restore(panes: Pane[], focusedPane: PaneId): void;
}

let nextTab = 1;
let nextNotice = 1;
const newTabId = () => `t${Date.now().toString(36)}${(nextTab++).toString(36)}`;

function updatePane(panes: Pane[], id: PaneId, f: (p: Pane) => Pane): Pane[] {
  return panes.map((p) => (p.id === id ? f(p) : p));
}

export const useWorkbench = create<WorkbenchState>((set, get) => ({
  panes: [{ id: "left", tabs: [] }],
  focusedPane: "left",
  notices: [],
  prompt: null,
  sidebars: { left: { visible: true }, right: { visible: false } },
  panelSide: () => undefined,
  openProject: () => undefined,
  viewFor: () => undefined,

  showPanel(panelId) {
    const side = get().panelSide(panelId);
    if (!side) return;
    get().setSidebar(side, { visible: true, active: panelId });
  },

  togglePanel(panelId) {
    const side = get().panelSide(panelId);
    if (!side) return;
    const sb = get().sidebars[side];
    if (sb.visible && sb.active === panelId) get().setSidebar(side, { visible: false });
    else get().setSidebar(side, { visible: true, active: panelId });
  },

  setSidebar(side, patch) {
    const sidebars = get().sidebars;
    set({ sidebars: { ...sidebars, [side]: { ...sidebars[side], ...patch } } });
  },

  ask(opts) {
    get().prompt?.resolve(null);
    return new Promise<string | null>((resolve) => {
      const id = nextNotice++;
      set({
        prompt: {
          id,
          ...opts,
          resolve: (v) => {
            if (get().prompt?.id === id) set({ prompt: null });
            resolve(v);
          },
        },
      });
    });
  },

  openView(viewId, resourceId, opts) {
    const state = get();
    const paneId: PaneId =
      opts?.pane && state.panes.some((p) => p.id === opts.pane) ? opts.pane : state.focusedPane;
    // Already open in that pane? Activate it.
    const pane = state.panes.find((p) => p.id === paneId);
    const existing = pane?.tabs.find((t) => t.viewId === viewId && t.resourceId === resourceId);
    if (existing) {
      set({ panes: updatePane(state.panes, paneId, (p) => ({ ...p, activeTabId: existing.id })), focusedPane: paneId });
      return;
    }
    const tab: Tab = { id: newTabId(), viewId, ...(resourceId !== undefined ? { resourceId } : {}) };
    set({
      panes: updatePane(state.panes, paneId, (p) => {
        const tabs = [...p.tabs];
        const activeIdx = tabs.findIndex((t) => t.id === p.activeTabId);
        tabs.splice(activeIdx >= 0 ? activeIdx + 1 : tabs.length, 0, tab);
        return { ...p, tabs, activeTabId: tab.id };
      }),
      focusedPane: paneId,
    });
  },

  openDocument(id, opts) {
    const viewId = get().viewFor("document");
    if (viewId) get().openView(viewId, id, opts);
  },

  activateTab(tabId) {
    const pane = get().panes.find((p) => p.tabs.some((t) => t.id === tabId));
    if (!pane) return;
    set({ panes: updatePane(get().panes, pane.id, (p) => ({ ...p, activeTabId: tabId })), focusedPane: pane.id });
  },

  closeTab(tabId) {
    const state = get();
    const pane = state.panes.find((p) => p.tabs.some((t) => t.id === tabId));
    if (!pane) return;
    const idx = pane.tabs.findIndex((t) => t.id === tabId);
    const tabs = pane.tabs.filter((t) => t.id !== tabId);
    const activeTabId =
      pane.activeTabId === tabId ? (tabs[Math.min(idx, tabs.length - 1)]?.id ?? undefined) : pane.activeTabId;
    let panes = updatePane(state.panes, pane.id, (p) => {
      const next: Pane = { id: p.id, tabs };
      if (activeTabId) next.activeTabId = activeTabId;
      return next;
    });
    // Closing the last tab of the right pane closes the split.
    if (pane.id === "right" && tabs.length === 0) panes = panes.filter((p) => p.id !== "right");
    set({ panes, focusedPane: panes.some((p) => p.id === state.focusedPane) ? state.focusedPane : "left" });
  },

  closeResource(resourceId) {
    for (const p of get().panes) {
      for (const t of p.tabs) if (t.resourceId === resourceId) get().closeTab(t.id);
    }
  },

  focusPane(pane) {
    if (get().panes.some((p) => p.id === pane)) set({ focusedPane: pane });
  },

  toggleSplit() {
    const state = get();
    if (state.panes.some((p) => p.id === "right")) {
      // Merge right tabs back into the left pane.
      const right = state.panes.find((p) => p.id === "right");
      const left = state.panes.find((p) => p.id === "left");
      if (!left || !right) return;
      const extra = right.tabs.filter((t) => !left.tabs.some((l) => l.viewId === t.viewId && l.resourceId === t.resourceId));
      set({ panes: [{ ...left, tabs: [...left.tabs, ...extra] }], focusedPane: "left" });
      return;
    }
    // Open the split with a copy of the current tab.
    const left = state.panes.find((p) => p.id === "left");
    const active = left?.tabs.find((t) => t.id === left.activeTabId);
    const tab: Tab | undefined = active ? { ...active, id: newTabId() } : undefined;
    const right: Pane = tab ? { id: "right", tabs: [tab], activeTabId: tab.id } : { id: "right", tabs: [] };
    set({ panes: [...state.panes, right], focusedPane: "right" });
  },

  notify(n) {
    const id = nextNotice++;
    set({ notices: [...get().notices.filter((x) => x.message !== n.message && (n.key === undefined || x.key !== n.key)), { ...n, id }] });
    const timeout = n.timeoutMs ?? 6000;
    if (timeout > 0) setTimeout(() => get().dismiss(id), timeout);
    return id;
  },

  dismiss(id) {
    set({ notices: get().notices.filter((n) => n.id !== id) });
  },

  restore(panes, focusedPane) {
    const valid = panes.filter((p) => p.id === "left" || p.id === "right");
    if (!valid.some((p) => p.id === "left")) valid.unshift({ id: "left", tabs: [] });
    set({ panes: valid, focusedPane: valid.some((p) => p.id === focusedPane) ? focusedPane : "left" });
  },
}));

/** Imperative access for non-React code. */
export const workbench = {
  openDocument: (id: string, opts?: { pane?: PaneId; newTab?: boolean }) =>
    useWorkbench.getState().openDocument(id, opts),
  openView: (viewId: string, resourceId?: string, opts?: { pane?: PaneId }) =>
    useWorkbench.getState().openView(viewId, resourceId, opts),
  notify: (n: Omit<Notice, "id">) => useWorkbench.getState().notify(n),
  ask: (opts: { title: string; placeholder?: string; initial?: string }) => useWorkbench.getState().ask(opts),
  showPanel: (panelId: string) => useWorkbench.getState().showPanel(panelId),
  openProject: (root: string) => useWorkbench.getState().openProject(root),
  togglePanel: (panelId: string) => useWorkbench.getState().togglePanel(panelId),
  activeTab(): Tab | undefined {
    const s = useWorkbench.getState();
    const pane = s.panes.find((p) => p.id === s.focusedPane);
    return pane?.tabs.find((t) => t.id === pane.activeTabId);
  },
  activeDocumentId(): string | undefined {
    return workbench.activeTab()?.resourceId;
  },
};

export function useActiveDocumentId(): string | undefined {
  return useWorkbench((s) => {
    const pane = s.panes.find((p) => p.id === s.focusedPane);
    return pane?.tabs.find((t) => t.id === pane.activeTabId)?.resourceId;
  });
}
