/**
 * Settings → Bake-off (SPEC §6.6): speed test, blind quality test and
 * "Use this model". Results are saved in the project's `.margin/bakeoff/`.
 */
import { useCallback, useEffect, useState } from "react";

import {
  ai,
  events,
  workbench,
  type BakeoffRun,
  type BakeoffState,
  type BlindQuestion,
  type Candidate,
  type QualityScore,
  type RunSummary,
  type SpeedResult,
} from "@margin/sdk";

import { errMessage, refreshConfig } from "./store";

/** Generated bindings type f64 as `number | null` (NaN serializes to null). */
const n0 = (n: number | null) => Math.round(n ?? 0).toLocaleString("en-US");
const f = (n: number | null, digits: number) => (n ?? 0).toFixed(digits);
const gb = (mb: number) => (mb / 1024).toFixed(1);

export function BakeoffPage() {
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [tune, setTune] = useState(true);
  const [state, setState] = useState<BakeoffState | null>(null);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [run, setRun] = useState<BakeoffRun | null>(null);

  const refresh = useCallback(
    () =>
      Promise.all([ai.bakeoff.state(), ai.bakeoff.runs()])
        .then(([s, r]) => {
          setState(s);
          setRuns(r);
          if (s.run) setRun(s.run);
        })
        .catch(() => undefined),
    [],
  );

  useEffect(() => {
    void ai.bakeoff.candidates().then((c) => {
      setCandidates(c);
      setChosen(new Set(c.filter((x) => x.available).map((x) => x.presetId)));
    });
    void refresh();
    const off = events.on("ai.bakeoffProgress", () => void refresh());
    return off;
  }, [refresh]);

  const start = async () => {
    try {
      await ai.bakeoff.start([...chosen], tune);
      await refresh();
    } catch (e) {
      workbench.notify({ message: errMessage(e) });
    }
  };

  const running = state?.running ?? false;
  return (
    <section>
      <h2>Model bake-off</h2>
      <p className="muted">
        Loads each model in turn, times it on your text (or a public-domain sample while the project is short), then writes ten continuations for you to compare without knowing which model wrote which. Close other GPU programs
        first: the numbers are only fair when the GPU is free.
      </p>

      <h3>Models to test</h3>
      <ul className="ai-candidates">
        {candidates.map((c) => (
          <li key={c.presetId}>
            <label>
              <input
                type="checkbox"
                disabled={!c.available || running}
                checked={chosen.has(c.presetId)}
                onChange={(e) => {
                  const next = new Set(chosen);
                  if (e.target.checked) next.add(c.presetId);
                  else next.delete(c.presetId);
                  setChosen(next);
                }}
              />{" "}
              {c.name}
            </label>
            {!c.available && <small className="muted"> not available: {c.missing}</small>}
          </li>
        ))}
      </ul>
      <label className="ai-option">
        <input type="checkbox" checked={tune} disabled={running} onChange={(e) => setTune(e.target.checked)} /> Tune <code>--n-cpu-moe</code> for models that offload experts (loads the model several times)
      </label>
      <div className="ai-actions">
        <button disabled={running || chosen.size === 0} onClick={() => void start()}>
          Run bake-off
        </button>
        {running && <button onClick={() => void ai.bakeoff.cancel()}>Cancel</button>}
        {running && (
          <span className="muted" role="status">
            {state?.presetId ? `${candidates.find((c) => c.presetId === state.presetId)?.name ?? state.presetId}: ` : ""}
            {state?.stage}
          </span>
        )}
        {!running && state?.stage === "cancelled" && <span className="muted">Cancelled. Finished models are kept.</span>}
      </div>

      {runs.length > 0 && (
        <div className="settings-row">
          <div className="settings-label">
            <span>Earlier runs</span>
          </div>
          <div className="settings-control">
            <select
              value={run?.runId ?? ""}
              onChange={(e) => e.target.value && ai.bakeoff.load(e.target.value).then(setRun).catch((err: unknown) => workbench.notify({ message: errMessage(err) }))}
            >
              <option value="">Choose a run…</option>
              {runs.map((r) => (
                <option key={r.runId} value={r.runId}>
                  {new Date(r.startedAt).toLocaleString()} · {r.models.join(", ")} · {r.source}
                </option>
              ))}
            </select>
          </div>
        </div>
      )}

      {run && run.candidates.length > 0 && <SpeedTable run={run} />}
      {run && run.questions.length > 0 && !running && <BlindTest runId={run.runId} onUsed={() => void refreshConfig()} run={run} />}
    </section>
  );
}

function SpeedTable({ run }: { run: BakeoffRun }) {
  const rows: SpeedResult[] = run.candidates.map((c) => c.speed);
  return (
    <>
      <h3>Speed</h3>
      <div className="ai-scroll">
        <table className="ai-table">
          <thead>
            <tr>
              <th>Model</th>
              <th>Go?</th>
              <th>Warm first token p50 / p95</th>
              <th>Cold 6K prompt</th>
              <th>Decode</th>
              <th>Sentence</th>
              <th>VRAM used / headroom</th>
              <th>Load</th>
              <th>n-cpu-moe</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.presetId}>
                <td>{r.name}</td>
                {r.error ? (
                  <td colSpan={8} className="ai-warn">
                    {r.error}
                  </td>
                ) : (
                  <>
                    <td title={r.misses.join("\n")}>{r.misses.length === 0 ? "Go" : `No: ${r.misses.length} missed`}</td>
                    <td>
                      {n0(r.warmTtftP50Ms)} / {n0(r.warmTtftP95Ms)} ms
                    </td>
                    <td>{f((r.coldTtftMs ?? 0) / 1000, 1)} s</td>
                    <td>{f(r.decodeTps, 1)} tok/s</td>
                    <td>{f((r.sentenceP50Ms ?? 0) / 1000, 2)} s</td>
                    <td>
                      {gb(r.vramUsedMb)} GB / {gb(r.headroomMb)} GB
                    </td>
                    <td>{f(r.loadSeconds, 0)} s</td>
                    <td>{r.nCpuMoe ?? "–"}</td>
                  </>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {rows.some((r) => r.misses.length > 0) && (
        <ul className="muted">
          {rows
            .filter((r) => r.misses.length > 0)
            .map((r) => (
              <li key={r.presetId}>
                {r.name}: {r.misses.join("; ")}
              </li>
            ))}
        </ul>
      )}
      {rows.some((r) => r.templateWarning) && (
        <ul className="ai-warn">
          {rows
            .filter((r) => r.templateWarning)
            .map((r) => (
              <li key={r.presetId}>
                {r.name}: {r.templateWarning}
              </li>
            ))}
        </ul>
      )}
    </>
  );
}

function BlindTest({ runId, run, onUsed }: { runId: string; run: BakeoffRun; onUsed: () => void }) {
  const [qs, setQs] = useState<BlindQuestion[]>([]);
  const [scores, setScores] = useState<QualityScore[] | null>(null);

  const load = useCallback(async () => {
    const q = await ai.bakeoff.questions(runId);
    setQs(q);
    if (q.length > 0 && q.every((x) => x.answered)) setScores(await ai.bakeoff.scores(runId).catch(() => null));
    else setScores(null);
  }, [runId]);
  useEffect(() => {
    let live = true;
    void ai.bakeoff
      .questions(runId)
      .then(async (q) => {
        const s = q.length > 0 && q.every((x) => x.answered) ? await ai.bakeoff.scores(runId).catch(() => null) : null;
        if (live) {
          setQs(q);
          setScores(s);
        }
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [runId, run.candidates.length]);

  const next = qs.find((q) => !q.answered);
  const answered = qs.filter((q) => q.answered).length;
  const answer = async (index: number, label: string | null) => {
    try {
      await ai.bakeoff.answer(runId, index, label);
      await load();
    } catch (e) {
      workbench.notify({ message: errMessage(e) });
    }
  };

  if (qs.length === 0) return null;
  return (
    <>
      <h3>
        Blind quality test{" "}
        <span className="muted">
          {answered} of {qs.length} answered
        </span>
      </h3>
      {next ? (
        <div className="ai-card">
          <p className="ai-cut">…{next.cutText}</p>
          <p className="muted">Which continuation would you most want to see here?</p>
          <ul className="ai-options">
            {next.options.map((o) => (
              <li key={o.label}>
                <button onClick={() => void answer(next.index, o.label)}>
                  <strong>{o.label}</strong> {o.text}
                </button>
              </li>
            ))}
          </ul>
          <button className="link" onClick={() => void answer(next.index, null)}>
            None of these
          </button>
        </div>
      ) : (
        scores && (
          <>
            <table className="ai-table">
              <thead>
                <tr>
                  <th>Model</th>
                  <th>Picked</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {scores.map((s) => (
                  <tr key={s.presetId}>
                    <td>{s.name}</td>
                    <td>
                      {s.wins} of {s.answered}
                    </td>
                    <td>
                      <button
                        onClick={() =>
                          ai.bakeoff
                            .useModel(runId, s.presetId)
                            .then(() => {
                              onUsed();
                              workbench.notify({ message: `${s.name} is now the active model.` });
                            })
                            .catch((e: unknown) => workbench.notify({ message: errMessage(e) }))
                        }
                      >
                        Use this model
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted">"Ask my book" questions join this test in M4, when Story Memory exists.</p>
          </>
        )
      )}
    </>
  );
}
