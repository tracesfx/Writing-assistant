/** Settings → Memory: indexing, conflict flagging and the embedding server. */
import { useEffect, useState } from "react";

import { memory, useMemoryStatus, workbench, type MemoryConfig } from "@margin/sdk";

import { fail } from "./common";

export function MemoryPage() {
  const [cfg, setCfg] = useState<MemoryConfig | null>(null);
  const status = useMemoryStatus();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void memory.config.get().then(setCfg).catch(() => undefined);
  }, []);
  if (!cfg) return <p className="muted">Loading…</p>;

  const save = async (patch: Partial<MemoryConfig>) => {
    const next = { ...cfg, ...patch };
    setCfg(next);
    try {
      setCfg(await memory.config.set(next));
    } catch (e) {
      fail("Could not save", e);
    }
  };
  const act = async (what: string, f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      fail(what, e);
    } finally {
      setBusy(false);
    }
  };
  const emb = status?.embeddings;

  return (
    <div className="settings-page memory-page">
      <h3>Reading your scenes</h3>
      <p className="muted">
        While you rest, Margin reads each changed scene once with the local model and fills the Story Bible with what the text says. It never edits your
        words, and it never changes what you marked as canon.
      </p>
      <label className="setting-row">
        <input type="checkbox" checked={cfg.indexing} onChange={(e) => void save({ indexing: e.target.checked })} /> Read scenes in the background
      </label>
      <label className="setting-row">
        Wait until a scene has been unchanged for{" "}
        <input
          type="number"
          min={0}
          max={3600}
          value={cfg.settleSeconds}
          onChange={(e) => setCfg({ ...cfg, settleSeconds: Number(e.target.value) })}
          onBlur={() => void save({})}
          style={{ width: "5em" }}
        />{" "}
        seconds
      </label>
      <label className="setting-row">
        <input type="checkbox" checked={cfg.flagConflicts} onChange={(e) => void save({ flagConflicts: e.target.checked })} /> Flag text that contradicts
        canon (shown only inside the bible entry)
      </label>
      <div className="setting-row">
        <button
          disabled={busy}
          title="Forget everything the model inferred and read every scene again. Canon stays."
          onClick={() =>
            void act("Could not rebuild", async () => {
              const n = await memory.rebuildAll();
              workbench.notify({ message: `Memory will be rebuilt: ${n} scene${n === 1 ? "" : "s"} to read again. Canon is untouched.` });
            })
          }
        >
          Rebuild all memory
        </button>
        <span className="muted"> {status ? `${status.pending} scene${status.pending === 1 ? "" : "s"} waiting` : ""}</span>
      </div>

      <h3>Finding passages (embeddings)</h3>
      <p className="muted">
        Ask my book finds passages by keywords. A small embedding model running on the CPU finds them by meaning too. It needs a GGUF embedding model
        file (for example EmbeddingGemma) that you download yourself; Margin never downloads models.
      </p>
      <label className="setting-row">
        <input type="checkbox" checked={cfg.embeddingEnabled} onChange={(e) => void save({ embeddingEnabled: e.target.checked })} /> Use an embedding
        model
      </label>
      <label className="setting-row stack">
        Embedding model file
        <input
          value={cfg.embeddingModel}
          placeholder="D:/MODELS/embed/embeddinggemma-300m-Q8_0.gguf"
          onChange={(e) => setCfg({ ...cfg, embeddingModel: e.target.value })}
          onBlur={() => void save({})}
        />
      </label>
      <label className="setting-row stack">
        llama-server program
        <input value={cfg.embeddingBinary} onChange={(e) => setCfg({ ...cfg, embeddingBinary: e.target.value })} onBlur={() => void save({})} />
      </label>
      <label className="setting-row">
        Port{" "}
        <input
          type="number"
          min={1}
          max={65535}
          value={cfg.embeddingPort}
          onChange={(e) => setCfg({ ...cfg, embeddingPort: Number(e.target.value) })}
          onBlur={() => void save({})}
          style={{ width: "6em" }}
        />
      </label>
      <label className="setting-row">
        <input type="checkbox" checked={cfg.embeddingAutoStart} onChange={(e) => void save({ embeddingAutoStart: e.target.checked })} /> Start it when a
        project opens
      </label>
      <div className="setting-row">
        <button disabled={busy || !cfg.embeddingEnabled} onClick={() => void act("Could not start the embedding server", () => memory.embedding.start())}>
          Start
        </button>
        <button disabled={busy} onClick={() => void act("Could not stop it", () => memory.embedding.stop())}>
          Stop
        </button>
        <span className={emb?.ready ? "" : "muted"}>
          {emb ? `${emb.detail}${emb.ready ? ` · ${emb.vectors} passages` : ""}` : ""}
        </span>
      </div>
    </div>
  );
}
