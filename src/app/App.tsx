import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useMemo, useState } from "react";

import {
  lifecycle,
  settings,
  story,
  useProjectStore,
  useSettings,
  useWorkbench,
  type AppSettings,
  type BinderTree,
  type ProjectInfo,
  setMarginTabs,
  setInsights,
} from "@margin/sdk";

import { wireProjectEvents } from "../sdk/project";
import type { RecentProject } from "../sdk/bindings";
import { firstPartyModules } from "../modules";
import { buildKeymap, resolve } from "./keymap";
import { activateModules, buildRegistry } from "./registry";
import { endSession, saveNow, startSession, type RestoredLayout } from "./session";
import shellModule from "./shellModule";
import { useShellUi } from "./shellUi";
import { Welcome } from "./Welcome";
import { Workspace } from "./Workspace";

type Phase = { kind: "booting" } | { kind: "welcome"; error: string | null } | ({ kind: "workspace" } & RestoredLayout);

function allDocIds(tree: BinderTree): Set<string> {
  const ids = new Set<string>();
  const walk = (nodes: { id: string; children: typeof nodes }[]) => {
    for (const n of nodes) {
      ids.add(n.id);
      walk(n.children);
    }
  };
  for (const s of tree.sections) walk(s.nodes);
  return ids;
}

const FONTS: Record<AppSettings["editorFont"], string> = {
  literata: '"Literata Variable", Georgia, serif',
  georgia: "Georgia, serif",
  palatino: '"Palatino Linotype", Palatino, serif',
  "system-serif": "Cambria, Georgia, serif",
  sans: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
};

/** Apply appearance settings to the document root (SPEC §11.2). */
function applyAppearance(s: AppSettings): void {
  const root = document.documentElement;
  if (s.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = s.theme;
  root.style.setProperty("--editor-font", FONTS[s.editorFont]);
  root.style.setProperty("--editor-size", `${s.editorSize}px`);
  root.style.setProperty("--editor-leading", String(s.lineHeight));
  root.style.setProperty("--editor-width", `${s.editorWidth}em`);
  root.style.zoom = s.uiScale === 1 ? "" : String(s.uiScale);
}

const MODULES = [shellModule, ...firstPartyModules];

export function App() {
  const [phase, setPhase] = useState<Phase>({ kind: "booting" });
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const disabled = useSettings((s) => s.disabledModules);
  const keymapOverrides = useSettings((s) => s.keymap);
  const appearance = useSettings((s) => s);
  const registry = useMemo(() => buildRegistry(MODULES, (m) => !disabled.includes(m.id)), [disabled]);

  useEffect(() => applyAppearance(appearance), [appearance]);

  // Tell the workbench which view opens documents and where panels live.
  useEffect(() => {
    useShellUi.setState({ registry });
    setMarginTabs(registry.marginTabs);
    setInsights(registry.insightsViews, registry.memoryExtractors);
    useWorkbench.setState({
      viewFor: (k) => registry.viewFor(k),
      panelSide: (id) => registry.panels.find((p) => p.id === id)?.side,
    });
  }, [registry]);

  // Module activation follows enable/disable.
  useEffect(() => activateModules(registry.modules), [registry]);

  // Global command keybindings (bubble phase, after the editor's own keymap).
  useEffect(() => {
    const keymap = buildKeymap(registry.commands.values(), keymapOverrides);
    const onKey = (e: KeyboardEvent) => {
      const cmd = resolve(keymap, e);
      if (!cmd) return;
      if (!cmd.allowDefault) e.preventDefault();
      Promise.resolve(cmd.run({ target: e.target })).catch((err: unknown) =>
        console.error(`[margin] command ${cmd.id} failed`, err),
      );
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [registry, keymapOverrides]);

  // Flush unsaved text, record the session and back up before the window closes.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested(async () => {
        await lifecycle.flushAll();
        await saveNow();
        await story.app.beforeClose().catch((e: unknown) => console.warn("[margin] before-close work failed", e));
      })
      .then((u) => {
        unlisten = u;
      })
      .catch((e: unknown) => console.warn("[margin] close handler unavailable", e));
    return () => unlisten?.();
  }, []);

  // Modules can open a project (e.g. a restored backup).
  useEffect(() => {
    useWorkbench.setState({ openProject: (root) => void openProject(root) });
  });

  // "Open another project…"
  useEffect(() => {
    const onSwitch = () => {
      void (async () => {
        await lifecycle.flushAll();
        await saveNow();
        await story.app.beforeClose().catch(() => undefined);
        endSession();
        await story.project.close().catch(() => undefined);
        const session = await story.app.session().catch(() => null);
        if (session) setRecent(session.recentProjects ?? []);
        setPhase({ kind: "welcome", error: null });
      })();
    };
    window.addEventListener("margin:switch-project", onSwitch);
    return () => window.removeEventListener("margin:switch-project", onSwitch);
  }, []);

  async function enter(info: ProjectInfo) {
    useProjectStore.getState().setInfo(info);
    await useProjectStore.getState().refreshTree();
    const tree = useProjectStore.getState().tree;
    const session = await story.app.session();
    setRecent(session.recentProjects ?? []);
    const restored = startSession(info.id, session.ui?.[info.id], tree ? allDocIds(tree) : new Set());
    setPhase({ kind: "workspace", ...restored });
  }

  async function openProject(root: string) {
    try {
      await lifecycle.flushAll();
      endSession();
      await enter(await story.project.open(root));
    } catch (e) {
      setPhase({ kind: "welcome", error: `Could not open ${root}: ${e instanceof Error ? e.message : String(e)}` });
    }
  }

  // Boot: load settings, then reopen the last project (session restore).
  useEffect(() => {
    wireProjectEvents();
    void (async () => {
      try {
        const session = await story.app.session();
        settings.load(session.settings);
        setRecent(session.recentProjects ?? []);
        const last = session.recentProjects?.[0];
        if (last) {
          try {
            await enter(await story.project.open(last.root));
            return;
          } catch (e) {
            setPhase({ kind: "welcome", error: `Could not reopen ${last.title}: ${e instanceof Error ? e.message : String(e)}` });
            return;
          }
        }
      } catch (e) {
        console.error("[margin] could not load app session", e);
      }
      setPhase({ kind: "welcome", error: null });
    })();
  }, []);

  if (phase.kind === "booting") return <div className="booting" aria-busy="true" />;
  if (phase.kind === "welcome") {
    return (
      <Welcome
        recent={recent}
        error={phase.error}
        onOpen={(root) => void openProject(root)}
        onCreate={(req) =>
          void story.project
            .create(req)
            .then(enter)
            .catch((e: unknown) =>
              setPhase({ kind: "welcome", error: `Could not create the project: ${e instanceof Error ? e.message : String(e)}` }),
            )
        }
        onForget={(root) =>
          void story.app.forgetRecent(root).then(() => setRecent((r) => r.filter((x) => x.root !== root)))
        }
      />
    );
  }
  return (
    <Workspace
      registry={registry}
      {...(phase.sidebarWidth !== undefined ? { initialSidebarWidth: phase.sidebarWidth } : {})}
      {...(phase.rightWidth !== undefined ? { initialRightWidth: phase.rightWidth } : {})}
    />
  );
}
