/**
 * Context Inspector, "Why this suggestion?" (SPEC §9.6): the exact prompt that
 * was sent for one request, split into its labelled sections, with token
 * counts, the raw output and the latency.
 */
import { useEffect, useState } from "react";

import { ai, type ContextLogEntry, type WorkspaceViewProps } from "@margin/sdk";

import { errMessage } from "./store";

const ms = (n: number | null | undefined) => (n == null ? "–" : `${Math.round(n)} ms`);

export function ContextView({ resourceId }: WorkspaceViewProps) {
  const [entry, setEntry] = useState<ContextLogEntry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [raw, setRaw] = useState(false);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        // No id: show the most recent request.
        const id = resourceId ?? (await ai.contextLog.list(1))[0]?.id;
        if (!id) {
          if (live) setError("Nothing has been sent to the model yet.");
          return;
        }
        const e = await ai.contextLog.get(id);
        if (live) {
          setEntry(e);
          setError(null);
        }
      } catch (e) {
        if (live) setError(`That request is no longer in the log (${errMessage(e)}). The log keeps the most recent requests.`);
      }
    })();
    return () => {
      live = false;
    };
  }, [resourceId]);

  if (error) return <div className="ai-view"><p className="muted">{error}</p></div>;
  if (!entry) return <div className="ai-view"><p className="muted">Loading…</p></div>;
  const t = entry.timings;
  const total = entry.sections.reduce((n, s) => n + s.tokens, 0);

  return (
    <div className="ai-view context-view">
      <h2>Why this suggestion?</h2>
      <p className="muted">
        What was sent to the model for this {entry.role} request, exactly as it was. Nothing else left your computer.
      </p>
      <dl className="ai-facts">
        <dt>When</dt>
        <dd>{new Date(entry.timestamp).toLocaleString("en-US")}</dd>
        <dt>Task</dt>
        <dd>{entry.role}</dd>
        <dt>Model</dt>
        <dd>{entry.model || "unknown"}</dd>
        <dt>Prompt</dt>
        <dd>
          {t.promptTokens.toLocaleString("en-US")} tokens ({t.cachedTokens.toLocaleString("en-US")} reused from the model's cache) · estimated from the sections: ~{total.toLocaleString("en-US")}
        </dd>
        <dt>Speed</dt>
        <dd>
          first token {ms(t.ttftMs)} · total {ms(t.totalMs)} · {t.outputTokens} tokens out
          {t.decodeTps != null ? ` · ${Math.round(t.decodeTps)} tokens/s` : ""}
        </dd>
        <dt>Result</dt>
        <dd>{entry.outcome}{entry.error ? `: ${entry.error}` : ""}</dd>
      </dl>
      <div className="ai-actions">
        <button onClick={() => setRaw((r) => !r)}>{raw ? "Show the sections" : "Show the raw prompt"}</button>
      </div>
      {raw ? (
        <pre className="ai-log">{entry.rawPrompt}</pre>
      ) : (
        entry.sections.map((s, i) => (
          <details key={i} className="context-section" open={i !== 4 || s.text.length < 1500}>
            <summary>
              <strong>{s.label}</strong> <span className="muted">~{s.tokens} tokens</span>
            </summary>
            <pre className="ai-log">{s.text || "(empty)"}</pre>
          </details>
        ))
      )}
      <h3>The model's reply</h3>
      <pre className="ai-log">{entry.rawOutput || "(nothing)"}</pre>
    </div>
  );
}
