/**
 * Model panel (SPEC §6.7): server state, VRAM, slots, last latency, log tail,
 * restart, plus a "Try it" box and the context log (SPEC §9.6).
 */
import { useEffect, useState } from "react";

import { ai, workbench, type Completion, type ContextLogEntry, type ContextLogSummary } from "@margin/sdk";

import { STATUS_LABEL, errMessage, refreshPanel, useAi } from "./store";

const ms = (n: number | null | undefined) => (n == null ? "–" : `${Math.round(n)} ms`);

export function ModelView() {
  const [tab, setTab] = useState<"server" | "log">("server");
  return (
    <div className="ai-view">
      <header className="ai-tabs" role="tablist">
        <button role="tab" aria-selected={tab === "server"} onClick={() => setTab("server")}>
          Server
        </button>
        <button role="tab" aria-selected={tab === "log"} onClick={() => setTab("log")}>
          Context log
        </button>
      </header>
      {tab === "server" ? <ServerTab /> : <ContextLogTab />}
    </div>
  );
}

function ServerTab() {
  const panel = useAi((s) => s.panel);
  const status = useAi((s) => s.status);
  const config = useAi((s) => s.config);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void refreshPanel();
    const t = setInterval(() => void refreshPanel(), 2000);
    return () => clearInterval(t);
  }, []);

  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      workbench.notify({ message: errMessage(e) });
    } finally {
      setBusy(false);
      void refreshPanel();
    }
  };

  if (!panel) return <p className="muted">Loading…</p>;
  const running = status === "ready" || status === "thinking" || status === "pausedTyping" || status === "starting";
  const vramPct = panel.vram ? Math.round((panel.vram.usedMb / panel.vram.totalMb) * 100) : 0;
  const preset = config?.presets.find((p) => p.id === panel.presetId);
  const c = panel.capabilities;

  return (
    <div className="ai-body">
      <section className="ai-card">
        <div className="ai-line">
          <span className={`ai-pill ai-${status}`}>{STATUS_LABEL[status]}</span>
          <span className="muted">{panel.detail}</span>
        </div>
        <div className="ai-actions">
          {!running && (
            <button disabled={busy || status === "off"} onClick={() => act(() => ai.connect())}>
              Start
            </button>
          )}
          {running && (
            <button
              disabled={busy}
              onClick={() =>
                act(async () => {
                  await ai.disconnect();
                  await ai.connect();
                })
              }
            >
              Restart
            </button>
          )}
          {running && (
            <button disabled={busy} onClick={() => act(() => ai.disconnect())}>
              Stop
            </button>
          )}
          {status === "off" && <span className="muted">AI is turned off (Ctrl+Shift+Space).</span>}
        </div>
        <dl className="ai-facts">
          <dt>Mode</dt>
          <dd>{panel.mode === "managed" ? "Managed (Margin runs llama-server)" : "External"}</dd>
          <dt>Endpoint</dt>
          <dd>{panel.endpoint}</dd>
          <dt>Preset</dt>
          <dd>{preset?.name ?? panel.presetId}</dd>
          {c.model && (
            <>
              <dt>Model</dt>
              <dd>{c.model}</dd>
            </>
          )}
          {c.buildInfo && (
            <>
              <dt>Build</dt>
              <dd>{c.buildInfo}</dd>
            </>
          )}
          {c.reachable && (
            <>
              <dt>Capabilities</dt>
              <dd>
                {[c.nativeCompletion && "native /completion", c.openaiChat && "OpenAI chat", c.tokenize && "tokenize", c.slotsEndpoint && "slots"]
                  .filter(Boolean)
                  .join(" · ") || "none detected"}
                {c.totalSlots > 0 && ` · ${c.totalSlots} slot${c.totalSlots === 1 ? "" : "s"}`}
                {c.nCtx > 0 && ` · ${c.nCtx.toLocaleString("en-US")} context`}
              </dd>
            </>
          )}
          <dt>Last request</dt>
          <dd>
            {panel.lastRequest
              ? `${panel.lastRequest.role} · ${panel.lastRequest.outcome} · first token ${ms(panel.lastRequest.ttftMs)} · total ${ms(panel.lastRequest.totalMs)}`
              : "none yet"}
          </dd>
          {panel.restarts > 0 && (
            <>
              <dt>Restarts</dt>
              <dd>{panel.restarts} of 3</dd>
            </>
          )}
        </dl>
        {c.templateWarning && <p className="ai-warn">{c.templateWarning}</p>}
      </section>

      <section className="ai-card">
        <h3>GPU memory</h3>
        {panel.vram ? (
          <>
            <div className="ai-bar" role="img" aria-label={`${vramPct}% of GPU memory in use`}>
              <span style={{ width: `${vramPct}%` }} />
            </div>
            <p className="muted">
              {(panel.vram.usedMb / 1024).toFixed(1)} of {(panel.vram.totalMb / 1024).toFixed(1)} GB used (the whole GPU, not only Margin)
            </p>
          </>
        ) : (
          <p className="muted">No NVIDIA GPU reading available.</p>
        )}
        {panel.slots.length > 0 && (
          <ul className="ai-slots">
            {panel.slots.map((s) => (
              <li key={s.id}>
                Slot {s.id}: {s.busy ? "busy" : "idle"} ({s.nCtx.toLocaleString("en-US")} tokens)
              </li>
            ))}
          </ul>
        )}
      </section>

      <TryIt disabled={status !== "ready"} />

      <section className="ai-card">
        <h3>Server log</h3>
        {panel.logTail.length === 0 ? (
          <p className="muted">No log yet. Managed mode shows llama-server output here.</p>
        ) : (
          <pre className="ai-log">{panel.logTail.join("\n")}</pre>
        )}
      </section>
    </div>
  );
}

function TryIt({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState("Write one sentence about a lighthouse keeper.");
  const [out, setOut] = useState<Completion | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      setOut(await ai.try(text));
    } catch (e) {
      workbench.notify({ message: errMessage(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="ai-card">
      <h3>Try it</h3>
      <div className="ai-try">
        <input value={text} aria-label="Test prompt" onChange={(e) => setText(e.target.value)} />
        <button disabled={disabled || busy || !text.trim()} onClick={run}>
          {busy ? "Working…" : "Send"}
        </button>
      </div>
      {out && (
        <div className="ai-out">
          <p>{out.outcome === "completed" ? out.text.trim() || "(empty)" : (out.error ?? out.outcome)}</p>
          <p className="muted">
            first token {ms(out.timings.ttftMs)} · total {ms(out.timings.totalMs)} · {out.timings.outputTokens} tokens
            {out.timings.cachedTokens > 0 && ` · ${out.timings.cachedTokens} of ${out.timings.promptTokens} prompt tokens cached`}
          </p>
        </div>
      )}
    </section>
  );
}

function ContextLogTab() {
  const [rows, setRows] = useState<ContextLogSummary[]>([]);
  const [open, setOpen] = useState<ContextLogEntry | null>(null);
  const limit = useAi((s) => s.config?.contextLogLimit ?? 500);
  useEffect(() => {
    const load = () => ai.contextLog.list(200).then(setRows).catch(() => setRows([]));
    void load();
    const t = setInterval(() => void load(), 3000);
    return () => clearInterval(t);
  }, []);
  const show = (id: string) =>
    ai.contextLog
      .get(id)
      .then(setOpen)
      .catch((e: unknown) => workbench.notify({ message: errMessage(e) }));

  if (open) {
    return (
      <div className="ai-body">
        <button className="link" onClick={() => setOpen(null)}>
          ← All requests
        </button>
        <section className="ai-card">
          <h3>
            {open.role} · {open.outcome}
          </h3>
          <p className="muted">
            {new Date(open.timestamp).toLocaleString()} · {open.model || "model"} · slot {open.slot} · {open.priority} · prompt {open.timings.promptTokens} tokens
            {open.timings.cachedTokens > 0 && ` (${open.timings.cachedTokens} cached)`} · output {open.timings.outputTokens} tokens · first token {ms(open.timings.ttftMs)} · total{" "}
            {ms(open.timings.totalMs)}
          </p>
          {open.error && <p className="ai-warn">{open.error}</p>}
        </section>
        {open.sections.map((s) => (
          <section className="ai-card" key={s.label}>
            <h3>
              {s.label} <span className="muted">~{s.tokens} tokens</span>
            </h3>
            <pre className="ai-log">{s.text}</pre>
          </section>
        ))}
        <section className="ai-card">
          <h3>Exact prompt sent</h3>
          <pre className="ai-log">{open.rawPrompt}</pre>
        </section>
        <section className="ai-card">
          <h3>Raw output</h3>
          <pre className="ai-log">{open.rawOutput || "(nothing)"}</pre>
        </section>
      </div>
    );
  }
  return (
    <div className="ai-body">
      <div className="ai-actions">
        <button
          onClick={() =>
            ai.contextLog
              .clear()
              .then(() => setRows([]))
              .catch((e: unknown) => workbench.notify({ message: errMessage(e) }))
          }
          disabled={rows.length === 0}
        >
          Clear log
        </button>
        <span className="muted">The last {limit} requests are kept in this project.</span>
      </div>
      {rows.length === 0 ? (
        <p className="muted">No requests yet. Every AI request will list the exact context it was sent.</p>
      ) : (
        <table className="ai-table">
          <thead>
            <tr>
              <th>Time</th>
              <th>Role</th>
              <th>Result</th>
              <th>Prompt</th>
              <th>Total</th>
              <th>Output</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.id} tabIndex={0} onClick={() => void show(r.id)} onKeyDown={(e) => e.key === "Enter" && void show(r.id)}>
                <td>{new Date(r.timestamp).toLocaleTimeString()}</td>
                <td>{r.role}</td>
                <td>{r.outcome}</td>
                <td>{r.promptTokens}</td>
                <td>{ms(r.totalMs)}</td>
                <td className="ai-preview">{r.preview}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
