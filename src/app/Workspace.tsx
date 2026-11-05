import { useEffect, useRef, useState } from "react";

import {
  story,
  useBinderTree,
  useProject,
  useWorkbench,
  workbench,
  findNode,
  type Pane,
  type RecoveryEntry,
} from "@margin/sdk";

import { ErrorBoundary } from "./ErrorBoundary";
import { Overlays } from "./Overlays";
import type { Registry } from "./registry";
import { setSidebarWidth } from "./session";
import { useShellUi } from "./shellUi";

interface Props {
  registry: Registry;
  initialSidebarWidth?: number;
  initialRightWidth?: number;
}

export function Workspace({ registry, initialSidebarWidth, initialRightWidth }: Props) {
  const panes = useWorkbench((s) => s.panes);
  const sidebars = useWorkbench((s) => s.sidebars);
  const distractionFree = useShellUi((s) => s.distractionFree);
  const [leftWidth, setLeftWidth] = useState(initialSidebarWidth ?? 260);
  const [rightWidth, setRightWidth] = useState(initialRightWidth ?? 300);
  const [reveal, setReveal] = useState(false);
  const revealTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Distraction-free: the chrome hides until the pointer reaches an edge.
  useEffect(() => {
    if (!distractionFree) return;
    const onMove = (e: MouseEvent) => {
      const edge = 10;
      const near = e.clientX < edge || e.clientY < edge || e.clientX > window.innerWidth - edge || e.clientY > window.innerHeight - edge;
      if (near) {
        setReveal(true);
        clearTimeout(revealTimer.current);
      } else if (!(e.target as HTMLElement).closest(".sidebar, .tabs, .statusbar")) {
        clearTimeout(revealTimer.current);
        revealTimer.current = setTimeout(() => setReveal(false), 600);
      }
    };
    window.addEventListener("mousemove", onMove);
    return () => {
      window.removeEventListener("mousemove", onMove);
      clearTimeout(revealTimer.current);
      setReveal(false);
    };
  }, [distractionFree]);

  const left = registry.panels.filter((p) => p.side === "left");
  const right = registry.panels.filter((p) => p.side === "right");
  const cls = `workspace${distractionFree ? " distraction-free" : ""}${distractionFree && reveal ? " df-reveal" : ""}`;

  return (
    <div className={cls}>
      <RecoveryBanner />
      <div className="workspace-main">
        {sidebars.left.visible && left.length > 0 && (
          <>
            <Sidebar side="left" panels={left} width={leftWidth} />
            <Splitter side="left" initial={leftWidth} onResize={setLeftWidth} onDone={(w) => setSidebarWidth(w, "left")} />
          </>
        )}
        <div className="panes">
          {panes.map((p) => (
            <PaneView key={p.id} pane={p} registry={registry} />
          ))}
        </div>
        {sidebars.right.visible && right.length > 0 && (
          <>
            <Splitter side="right" initial={rightWidth} onResize={setRightWidth} onDone={(w) => setSidebarWidth(w, "right")} />
            <Sidebar side="right" panels={right} width={rightWidth} />
          </>
        )}
      </div>
      <StatusBar registry={registry} />
      <Notices />
      <Overlays />
    </div>
  );
}

function Sidebar({ side, panels, width }: { side: "left" | "right"; panels: Registry["panels"]; width: number }) {
  const activeId = useWorkbench((s) => s.sidebars[side].active);
  const setSidebar = useWorkbench((s) => s.setSidebar);
  const active = panels.find((p) => p.id === activeId) ?? panels[0];
  return (
    <aside className={`sidebar sidebar-${side}`} style={{ width }} aria-label={side === "left" ? "Left sidebar" : "Right sidebar"}>
      {panels.length > 1 && (
        <div className="sidebar-tabs" role="tablist">
          {panels.map((p) => (
            <button
              key={p.id}
              role="tab"
              aria-selected={p.id === active?.id}
              className={`sidebar-tab${p.id === active?.id ? " active" : ""}`}
              onClick={() => setSidebar(side, { active: p.id })}
            >
              {p.title}
            </button>
          ))}
          {side === "right" && (
            <button className="sidebar-close" aria-label="Hide sidebar" onClick={() => setSidebar(side, { visible: false })}>
              ×
            </button>
          )}
        </div>
      )}
      {panels.map((p) => (
        <div key={p.id} className="sidebar-panel" hidden={p.id !== active?.id} role="tabpanel">
          <ErrorBoundary label={p.title}>
            <p.component visible={p.id === active?.id} />
          </ErrorBoundary>
        </div>
      ))}
    </aside>
  );
}

function Splitter({
  side,
  initial,
  onResize,
  onDone,
}: {
  side: "left" | "right";
  initial: number;
  onResize(w: number): void;
  onDone(w: number): void;
}) {
  const start = useRef<{ x: number; w: number } | null>(null);
  const last = useRef(initial);
  return (
    <div
      className="splitter"
      role="separator"
      aria-orientation="vertical"
      onPointerDown={(e) => {
        start.current = { x: e.clientX, w: last.current };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (!start.current) return;
        const dx = e.clientX - start.current.x;
        const w = Math.max(180, Math.min(560, start.current.w + (side === "left" ? dx : -dx)));
        last.current = w;
        onResize(w);
      }}
      onPointerUp={() => {
        start.current = null;
        onDone(last.current);
      }}
    />
  );
}

function PaneView({ pane, registry }: { pane: Pane; registry: Registry }) {
  const focused = useWorkbench((s) => s.focusedPane === pane.id);
  const tree = useBinderTree();
  const { activateTab, closeTab, focusPane } = useWorkbench.getState();

  const titleOf = (viewId: string, resourceId?: string) => {
    const view = registry.views.get(viewId);
    const doc = resourceId ? findNode(tree, resourceId)?.title : undefined;
    if (view?.opens === "document") return doc ?? "Untitled";
    if (doc) return `${view?.title ?? "View"}: ${doc}`;
    return view?.title ?? "Untitled";
  };

  return (
    <section
      className={`pane${focused ? " focused" : ""}`}
      onFocusCapture={() => focusPane(pane.id)}
      onMouseDownCapture={() => focusPane(pane.id)}
      aria-label={pane.id === "left" ? "Main pane" : "Split pane"}
    >
      {pane.tabs.length > 0 && (
        <div className="tabs" role="tablist">
          {pane.tabs.map((t) => (
            <div
              key={t.id}
              role="tab"
              aria-selected={t.id === pane.activeTabId}
              className={`tab${t.id === pane.activeTabId ? " active" : ""}`}
              onClick={() => activateTab(t.id)}
              onAuxClick={(e) => e.button === 1 && closeTab(t.id)}
              title={titleOf(t.viewId, t.resourceId)}
            >
              <span className="tab-title">{titleOf(t.viewId, t.resourceId)}</span>
              <button
                className="tab-close"
                aria-label="Close tab"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.id);
                }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="pane-body">
        {pane.tabs.length === 0 && <EmptyPane />}
        {pane.tabs.map((t) => {
          const view = registry.views.get(t.viewId);
          const active = t.id === pane.activeTabId;
          return (
            <div key={t.id} className="tab-panel" hidden={!active} role="tabpanel">
              {view ? (
                <ErrorBoundary label={view.title}>
                  <view.component
                    tabId={t.id}
                    {...(t.resourceId !== undefined ? { resourceId: t.resourceId } : {})}
                    active={active}
                    focused={focused}
                  />
                </ErrorBoundary>
              ) : (
                <div className="editor-message">This view is not available (its module is disabled).</div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

function EmptyPane() {
  return (
    <div className="empty-pane">
      <p>Choose a document in the binder, or press Ctrl+N for a new scene.</p>
      <p className="muted">Ctrl+P quick open · Ctrl+K commands · Ctrl+/ shortcuts</p>
    </div>
  );
}

function StatusBar({ registry }: { registry: Registry }) {
  const project = useProject();
  const tree = useBinderTree();
  const manuscript = tree?.sections.find((s) => s.section === "manuscript")?.totalWordCount ?? 0;
  const items = (align: "left" | "right") =>
    registry.statusItems
      .filter((i) => i.align === align)
      .map((i) => (
        <ErrorBoundary key={i.id} label={i.id} silent>
          <i.component />
        </ErrorBoundary>
      ));
  return (
    <footer className="statusbar" role="status">
      <div className="status-left">{items("left")}</div>
      <div className="status-right">
        {items("right")}
        {project && project.problems.length > 0 && (
          <span className="status-warn" title={project.problems.join("\n")}>
            {project.problems.length} file{project.problems.length > 1 ? "s" : ""} could not be read
          </span>
        )}
        <span className="muted">Manuscript {manuscript.toLocaleString("en-US")} words</span>
      </div>
    </footer>
  );
}

function Notices() {
  const notices = useWorkbench((s) => s.notices);
  const dismiss = useWorkbench((s) => s.dismiss);
  if (notices.length === 0) return null;
  return (
    <div className="notices" aria-live="polite">
      {notices.map((n) => (
        <div key={n.id} className="notice">
          <span>{n.message}</span>
          {n.action && (
            <button
              onClick={() => {
                n.action?.run();
                dismiss(n.id);
              }}
            >
              {n.action.label}
            </button>
          )}
          <button className="notice-close" aria-label="Dismiss" onClick={() => dismiss(n.id)}>
            ×
          </button>
        </div>
      ))}
    </div>
  );
}

/** Non-modal banner offering edits recovered from a crash (SPEC §11.3). */
function RecoveryBanner() {
  const project = useProject();
  const [entries, setEntries] = useState<RecoveryEntry[]>([]);

  useEffect(() => {
    if (!project) return;
    story.recovery
      .list()
      .then(setEntries)
      .catch((e: unknown) => console.warn("[margin] recovery list failed", e));
  }, [project?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (entries.length === 0) return null;

  async function restoreAll() {
    const failed: string[] = [];
    for (const e of entries) {
      try {
        const content = await story.recovery.get(e.docId);
        // The author explicitly chose the recovered text.
        await story.documents.save(e.docId, content, true);
        // Re-open so the editor shows the recovered text.
        useWorkbench.getState().closeResource(e.docId);
        workbench.openDocument(e.docId);
      } catch {
        failed.push(e.title);
      }
    }
    setEntries([]);
    workbench.notify({
      message: failed.length
        ? `Recovered all but: ${failed.join(", ")}`
        : `Recovered unsaved text in ${entries.length} document${entries.length > 1 ? "s" : ""}`,
    });
  }

  // Discarding is undoable: the journal files are only deleted once the
  // notice expires (and survive if Margin closes first).
  function discardAll() {
    const discarded = entries;
    setEntries([]);
    let undone = false;
    workbench.notify({
      message: "Recovered edits discarded",
      action: {
        label: "Undo",
        run: () => {
          undone = true;
          setEntries(discarded);
        },
      },
      timeoutMs: 10_000,
    });
    setTimeout(() => {
      if (undone) return;
      for (const e of discarded) void story.recovery.discard(e.docId).catch(() => undefined);
    }, 10_500);
  }

  const names = entries.map((e) => e.title).join(", ");
  return (
    <div className="inline-bar recovery" role="status">
      <span>Margin closed before saving your latest edits to {names}. They were kept.</span>
      <button className="primary" onClick={() => void restoreAll()}>
        Restore
      </button>
      <button onClick={discardAll}>Discard</button>
    </div>
  );
}
