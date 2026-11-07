/**
 * The Insights workspace (SPEC §10): a list of the views that modules
 * contribute through `insightsViews`, and the one that is selected. Views stay
 * mounted once opened, so their filters survive switching; they fetch and draw
 * only while showing. A view that throws takes down only itself.
 */
import { Component, useState, type ReactNode } from "react";

import { useInsightsStore, useInsightsViews, type WorkspaceViewProps } from "@margin/sdk";

class ViewBoundary extends Component<{ title: string; children: ReactNode }, { error: Error | null }> {
  override state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  override componentDidCatch(error: Error) {
    console.error(`[margin] Insights view "${this.props.title}" failed`, error);
  }
  override render() {
    if (this.state.error) {
      return (
        <div className="insight-error" role="alert">
          <p>This view stopped working: {this.state.error.message}</p>
          <button onClick={() => this.setState({ error: null })}>Try again</button>
        </div>
      );
    }
    return this.props.children;
  }
}

export function InsightsHome({ active }: WorkspaceViewProps) {
  const views = useInsightsViews();
  const selected = useInsightsStore((s) => s.selected);
  const current = views.find((v) => v.id === selected) ?? views[0];
  const [visited, setVisited] = useState<string[]>([]);
  const currentId = current?.id;
  // Remember every view opened, so it stays mounted (its filters survive).
  if (currentId && !visited.includes(currentId)) setVisited([...visited, currentId]);

  if (!current) {
    return (
      <div className="insights-home">
        <div className="insights-empty">
          <h2>Insights</h2>
          <p className="muted">No Insights views are switched on. Turn some on in Settings → Modules.</p>
        </div>
      </div>
    );
  }
  const shown = views.filter((v) => v.id === current.id || visited.includes(v.id));

  return (
    <div className="insights-home">
      <nav className="insights-nav" aria-label="Insights views">
        <h2>Insights</h2>
        <ul>
          {views.map((v) => (
            <li key={v.id}>
              <button
                className={v.id === current.id ? "active" : ""}
                aria-current={v.id === current.id ? "page" : undefined}
                onClick={() => useInsightsStore.setState({ selected: v.id })}
              >
                <span className="insights-nav-title">{v.title}</span>
                {v.description && <span className="insights-nav-desc">{v.description}</span>}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <div className="insights-body">
        {shown.map((v) => (
          <section key={v.id} className="insights-view" hidden={v.id !== current.id} aria-label={v.title}>
            <ViewBoundary title={v.title}>
              <v.component active={active && v.id === current.id} />
            </ViewBoundary>
          </section>
        ))}
      </div>
    </div>
  );
}
