/**
 * Session restore (SPEC §11.4): open tabs, split, focused pane, sidebars
 * and module UI state (cursor positions, bookmarks, …) per project, saved
 * through the backend app state.
 */
import { story, uiState, useWorkbench, type Pane, type PaneId } from "@margin/sdk";

import type { SidebarState } from "../sdk/workbench";
import { useUiStateStore } from "../sdk/uiState";

interface UiSession {
  v: 1;
  panes: Pane[];
  focusedPane: PaneId;
  sidebarWidth?: number;
  rightWidth?: number;
  sidebars?: { left: SidebarState; right: SidebarState };
  state?: Record<string, unknown>;
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
let unsubscribe: (() => void)[] = [];
let currentProject: string | null = null;
let sidebarWidth: number | undefined;
let rightWidth: number | undefined;

export function setSidebarWidth(w: number, side: "left" | "right" = "left"): void {
  if (side === "left") sidebarWidth = w;
  else rightWidth = w;
  scheduleSave();
}

function snapshot(): UiSession {
  const s = useWorkbench.getState();
  const out: UiSession = { v: 1, panes: s.panes, focusedPane: s.focusedPane, sidebars: s.sidebars, state: uiState.all() };
  if (sidebarWidth !== undefined) out.sidebarWidth = sidebarWidth;
  if (rightWidth !== undefined) out.rightWidth = rightWidth;
  return out;
}

function scheduleSave(): void {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveNow(), 800);
}

export async function saveNow(): Promise<void> {
  clearTimeout(saveTimer);
  if (!currentProject) return;
  try {
    await story.app.saveUi(currentProject, JSON.stringify(snapshot()));
  } catch (err) {
    console.warn("[margin] could not save session", err);
  }
}

export interface RestoredLayout {
  sidebarWidth?: number;
  rightWidth?: number;
}

/** Restore the layout for a project and start tracking changes. */
export function startSession(projectId: string, raw: string | undefined, validDocs: Set<string>): RestoredLayout {
  for (const u of unsubscribe) u();
  currentProject = projectId;
  sidebarWidth = undefined;
  rightWidth = undefined;
  let restored: RestoredLayout = {};
  uiState.replaceAll({});
  if (raw) {
    try {
      const ui = JSON.parse(raw) as UiSession;
      if (ui.v === 1 && Array.isArray(ui.panes)) {
        const panes = ui.panes.map((p) => {
          const tabs = p.tabs.filter((t) => !t.resourceId || validDocs.has(t.resourceId) || !t.viewId.startsWith("editor."));
          const active = tabs.some((t) => t.id === p.activeTabId) ? p.activeTabId : tabs[0]?.id;
          return active ? { id: p.id, tabs, activeTabId: active } : { id: p.id, tabs };
        });
        useWorkbench.getState().restore(panes.filter((p) => p.id === "left" || p.tabs.length > 0), ui.focusedPane);
        if (ui.sidebars) useWorkbench.setState({ sidebars: ui.sidebars });
        if (ui.state && typeof ui.state === "object") uiState.replaceAll(ui.state);
        if (typeof ui.sidebarWidth === "number") {
          sidebarWidth = ui.sidebarWidth;
          restored = { ...restored, sidebarWidth: ui.sidebarWidth };
        }
        if (typeof ui.rightWidth === "number") {
          rightWidth = ui.rightWidth;
          restored = { ...restored, rightWidth: ui.rightWidth };
        }
      }
    } catch {
      // A corrupt session blob is not worth bothering the author about.
    }
  } else {
    useWorkbench.getState().restore([{ id: "left", tabs: [] }], "left");
  }
  unsubscribe = [
    useWorkbench.subscribe((s, prev) => {
      if (s.panes !== prev.panes || s.focusedPane !== prev.focusedPane || s.sidebars !== prev.sidebars) scheduleSave();
    }),
    useUiStateStore.subscribe(scheduleSave),
  ];
  return restored;
}

export function endSession(): void {
  for (const u of unsubscribe) u();
  unsubscribe = [];
  currentProject = null;
}
