/**
 * Settings → Models (SPEC §15): master switch, connection mode, endpoints
 * and the preset editor. Applied with one Save button so a half-typed path
 * never reaches the server supervisor.
 */
import { useEffect, useState } from "react";

import { ai, workbench, type AiConfig, type Preset } from "@margin/sdk";

import { errMessage, refreshConfig, refreshPanel, saveConfig, useAi } from "./store";

export function ModelsPage() {
  const saved = useAi((s) => s.config);
  const [version, setVersion] = useState(0);
  // Read the latest saved settings before showing the form, so it never starts from a stale copy.
  const [fresh, setFresh] = useState(false);
  useEffect(() => {
    void refreshConfig().then(() => setFresh(true));
  }, []);
  if (!saved || !fresh) return <p className="muted">Loading…</p>;
  return <ModelsForm key={version} saved={saved} reset={() => setVersion((v) => v + 1)} />;
}

function ModelsForm({ saved, reset }: { saved: AiConfig; reset: () => void }) {
  const [draft, setDraft] = useState<AiConfig>(saved);
  const [presetId, setPresetId] = useState<string>(saved.presetId);
  // Argument text is kept as typed, so a new line is not dropped before the next argument.
  const [argDrafts, setArgDrafts] = useState<Record<string, string>>({});
  const editing = draft.presets.find((p) => p.id === presetId);
  const argsText = argDrafts[presetId] ?? editing?.args.join("\n") ?? "";
  const set = (patch: Partial<AiConfig>) => setDraft({ ...draft, ...patch });
  const setPreset = (patch: Partial<Preset>) =>
    setDraft({ ...draft, presets: draft.presets.map((p) => (p.id === presetId ? { ...p, ...patch } : p)) });
  const dirty = JSON.stringify(saved) !== JSON.stringify(draft) || Object.keys(argDrafts).length > 0;

  const save = async () => {
    const next: AiConfig = {
      ...draft,
      presets: draft.presets.map((p) =>
        p.id in argDrafts ? { ...p, args: (argDrafts[p.id] ?? "").split("\n").map((a) => a.trim()).filter(Boolean) } : p,
      ),
    };
    try {
      const done = await ai.config.set(next);
      useAi.setState({ config: done });
      setArgDrafts({});
      setDraft(done);
      workbench.notify({ message: "Model settings saved. Restart the server to apply them.", key: "ai.saved" });
      void refreshPanel();
    } catch (e) {
      workbench.notify({ message: `Could not save: ${errMessage(e)}` });
    }
  };

  const row = (label: string, control: React.ReactNode, hint?: string) => (
    <div className="settings-row">
      <div className="settings-label">
        <span>{label}</span>
        {hint && <small className="muted">{hint}</small>}
      </div>
      <div className="settings-control">{control}</div>
    </div>
  );

  return (
    <section>
      <h2>Models</h2>
      {row(
        "AI",
        <label>
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={(e) => {
              set({ enabled: e.target.checked });
              void saveConfig({ enabled: e.target.checked });
            }}
          />{" "}
          On (the kill switch, Ctrl+Shift+Space, does the same)
        </label>,
      )}
      {row(
        "Connection",
        <select value={draft.mode} onChange={(e) => set({ mode: e.target.value as AiConfig["mode"] })}>
          <option value="managed">Managed: Margin runs llama-server</option>
          <option value="external">External: connect to a running server</option>
        </select>,
      )}
      {draft.mode === "managed"
        ? row("Port", <input inputMode="numeric" value={draft.port} onChange={(e) => set({ port: Number(e.target.value.replace(/\D/g, "")) || 0 })} />, "Margin's own server. 8080 by default.")
        : row("Endpoint", <input value={draft.externalUrl} onChange={(e) => set({ externalUrl: e.target.value })} />, "Must be on this machine (127.0.0.1).")}
      {row(
        "Start with Margin",
        <label>
          <input type="checkbox" checked={draft.autoStart} onChange={(e) => set({ autoStart: e.target.checked })} /> Start the model server when the app opens
        </label>,
      )}
      {row(
        "Idle work starts after",
        <>
          <input inputMode="numeric" value={draft.idleSeconds} onChange={(e) => set({ idleSeconds: Number(e.target.value.replace(/\D/g, "")) || 0 })} /> <span className="muted">seconds without typing</span>
        </>,
      )}
      {row(
        "Context log keeps",
        <>
          <input inputMode="numeric" value={draft.contextLogLimit} onChange={(e) => set({ contextLogLimit: Number(e.target.value.replace(/\D/g, "")) || 0 })} /> <span className="muted">requests</span>
        </>,
      )}

      <h3>Active model</h3>
      {row(
        "Preset",
        <select value={draft.presetId} onChange={(e) => set({ presetId: e.target.value })}>
          {draft.presets.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>,
        "Used when the server starts.",
      )}

      <h3>Edit a preset</h3>
      {row(
        "Preset to edit",
        <select value={presetId ?? ""} onChange={(e) => setPresetId(e.target.value)}>
          {draft.presets.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>,
        editing?.notes,
      )}
      {editing && (
        <>
          {row("llama-server binary", <input className="wide" value={editing.binary} onChange={(e) => setPreset({ binary: e.target.value })} />, "Bonsai needs the PrismML build.")}
          {row("Model file", <input className="wide" value={editing.model} onChange={(e) => setPreset({ model: e.target.value })} />)}
          {row("Context size", <input inputMode="numeric" value={editing.contextSize} onChange={(e) => setPreset({ contextSize: Number(e.target.value.replace(/\D/g, "")) || 0 })} />, "Total across all slots (-c).")}
          {row("Slots", <input inputMode="numeric" value={editing.parallel} onChange={(e) => setPreset({ parallel: Number(e.target.value.replace(/\D/g, "")) || 1 })} />, "0 is ghost text, 1 is everything else (--parallel).")}
          {row(
            "Template",
            <select value={editing.template} onChange={(e) => setPreset({ template: e.target.value as Preset["template"] })}>
              <option value="chatMl">ChatML</option>
              <option value="gemma">Gemma</option>
              <option value="mistral">Mistral</option>
            </select>,
          )}
          {row(
            "Thinking off by",
            <select value={editing.thinking} onChange={(e) => setPreset({ thinking: e.target.value as Preset["thinking"] })}>
              <option value="none">Nothing (model does not think)</option>
              <option value="prefill">Empty think block in the prompt</option>
              <option value="templateKwarg">enable_thinking=false (chat calls)</option>
            </select>,
          )}
          {row("Extra arguments", <textarea className="wide" rows={6} value={argsText} onChange={(e) => setArgDrafts({ ...argDrafts, [presetId]: e.target.value })} spellCheck={false} />, "One per line, e.g. -ngl, then 99 on the next line.")}
        </>
      )}
      <div className="settings-row">
        <div className="settings-label" />
        <div className="settings-control">
          <button disabled={!dirty} onClick={() => void save()}>
            Save
          </button>
          {dirty && (
            <button onClick={reset}>Discard changes</button>
          )}
        </div>
      </div>
    </section>
  );
}
