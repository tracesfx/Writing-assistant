/** Small pieces every Insights view shares: the header and the "nothing yet" notice. */
import type { ReactNode } from "react";

import { useMemoryStatus } from "./memory";

export function InsightHeader({ title, intro, children }: { title: string; intro?: string; children?: ReactNode }) {
  return (
    <header className="insight-header">
      <div>
        <h2>{title}</h2>
        {intro && <p className="muted">{intro}</p>}
      </div>
      {children && <div className="insight-controls">{children}</div>}
    </header>
  );
}

/**
 * Shown instead of a chart when memory has nothing to draw yet. Says why, and
 * how far the reading has got, rather than showing an empty frame.
 */
export function InsightEmpty({ what }: { what: string }) {
  const status = useMemoryStatus();
  const reading = status && status.pending > 0;
  return (
    <div className="insight-empty">
      <p>{what}</p>
      <p className="muted">
        {reading
          ? `Margin is reading your scenes while you rest: ${status.pending} left. This view fills in as it goes.`
          : status && status.total === 0
            ? "Write some scenes first. Margin reads them while you rest."
            : "Margin reads each scene once the model is running and you have paused. Check the AI status at the bottom of the window."}
      </p>
    </div>
  );
}

export function InsightError({ message }: { message: string }) {
  return (
    <div className="insight-error" role="alert">
      Could not load this view: {message}
    </div>
  );
}

/** One chart (or table) with a title and its own Export bar underneath. */
export function InsightPanel({ title, note, children }: { title?: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section className="insight-panel">
      {title && <h3>{title}</h3>}
      {note && <p className="muted insight-note">{note}</p>}
      {children}
    </section>
  );
}
