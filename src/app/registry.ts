/**
 * Module registry: collects contributions from enabled module manifests.
 * The shell renders only what is registered here.
 */
import type {
  CommandContribution,
  ModuleManifest,
  SettingsPageContribution,
  SidebarPanelContribution,
  StatusBarItemContribution,
  WorkspaceViewContribution,
} from "@margin/sdk";

type Owned<T> = T & { moduleId: string };

export interface Registry {
  /** Every known module, enabled or not (for Settings → Modules). */
  all: ModuleManifest[];
  modules: ModuleManifest[];
  views: Map<string, Owned<WorkspaceViewContribution>>;
  panels: Owned<SidebarPanelContribution>[];
  statusItems: Owned<StatusBarItemContribution>[];
  settingsPages: Owned<SettingsPageContribution>[];
  commands: Map<string, Owned<CommandContribution>>;
  viewFor(kind: "document"): string | undefined;
}

export function buildRegistry(modules: ModuleManifest[], isEnabled: (m: ModuleManifest) => boolean): Registry {
  const enabled = modules.filter((m) => m.required || isEnabled(m));
  const views = new Map<string, Owned<WorkspaceViewContribution>>();
  const panels: Owned<SidebarPanelContribution>[] = [];
  const statusItems: Owned<StatusBarItemContribution>[] = [];
  const settingsPages: Owned<SettingsPageContribution>[] = [];
  const commands = new Map<string, Owned<CommandContribution>>();
  const seenModules = new Set<string>();
  const used: ModuleManifest[] = [];

  for (const m of enabled) {
    if (seenModules.has(m.id)) {
      console.error(`[margin] duplicate module id ${m.id}; ignoring the second one`);
      continue;
    }
    seenModules.add(m.id);
    used.push(m);
    const c = m.contributes;
    for (const v of c.workspaceViews ?? []) {
      if (views.has(v.id)) console.error(`[margin] duplicate workspace view ${v.id} from ${m.id}`);
      else views.set(v.id, { ...v, moduleId: m.id });
    }
    for (const p of c.sidebarPanels ?? []) panels.push({ ...p, moduleId: m.id });
    for (const s of c.statusBarItems ?? []) statusItems.push({ ...s, moduleId: m.id });
    for (const s of c.settingsPages ?? []) settingsPages.push({ ...s, moduleId: m.id });
    for (const cmd of c.commands ?? []) {
      if (commands.has(cmd.id)) console.error(`[margin] duplicate command ${cmd.id} from ${m.id}`);
      else commands.set(cmd.id, { ...cmd, moduleId: m.id });
    }
  }
  panels.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
  statusItems.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));

  return {
    all: modules,
    modules: used,
    views,
    panels,
    statusItems,
    settingsPages,
    commands,
    viewFor(kind) {
      for (const v of views.values()) if (v.opens === kind) return v.id;
      return undefined;
    },
  };
}

/** Run `activate` of every enabled module; returns a function that deactivates them. */
export function activateModules(modules: ModuleManifest[]): () => void {
  const cleanups: (() => void)[] = [];
  for (const m of modules) {
    try {
      const c = m.activate?.();
      if (typeof c === "function") cleanups.push(c);
    } catch (err) {
      console.error(`[margin] module ${m.id} failed to activate`, err);
    }
  }
  return () => {
    for (const c of cleanups.reverse()) {
      try {
        c();
      } catch (err) {
        console.error("[margin] module cleanup failed", err);
      }
    }
  };
}
