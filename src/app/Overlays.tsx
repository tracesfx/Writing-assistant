/**
 * Transient, author-invoked overlays: command palette (Ctrl+K), quick open
 * (Ctrl+P), keyboard cheat sheet (Ctrl+/) and the one-line prompt. Each
 * closes with Esc or a click outside; none blocks typing elsewhere.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import { flattenTree, settings, useBinderTree, useSettings, useWorkbench, workbench, type CommandContribution } from "@margin/sdk";

import { normalizeChord, prettyChord } from "./keymap";
import { openSettings, shellUi, useShellUi } from "./shellUi";

export interface PickItem {
  id: string;
  label: string;
  detail?: string;
  hint?: string;
}

/** Subsequence match score (higher is better), or -1. */
export function fuzzyScore(query: string, text: string): number {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const direct = t.indexOf(q);
  if (direct >= 0) return 1000 - direct * 2 - (t.length - q.length) * 0.1 + (direct === 0 || t[direct - 1] === " " ? 50 : 0);
  let score = 0;
  let ti = 0;
  let last = -1;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return -1;
    score += found === last + 1 ? 5 : 1;
    if (found === 0 || t[found - 1] === " ") score += 4;
    last = found;
    ti = found + 1;
  }
  return score;
}

function Picker({
  items,
  placeholder,
  onPick,
  label,
}: {
  items: PickItem[];
  placeholder: string;
  label: string;
  onPick: (item: PickItem) => void;
}) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const shown = useMemo(() => {
    if (!q.trim()) return items.slice(0, 200);
    return items
      .map((it) => ({ it, s: Math.max(fuzzyScore(q, it.label), fuzzyScore(q, `${it.detail ?? ""} ${it.label}`) - 20) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 200)
      .map((x) => x.it);
  }, [q, items]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  return (
    <div className="overlay-backdrop" onMouseDown={() => shellUi.close()}>
      <div className="picker" role="dialog" aria-label={label} onMouseDown={(e) => e.stopPropagation()}>
        <input
          autoFocus
          className="picker-input"
          placeholder={placeholder}
          value={q}
          aria-label={placeholder}
          onChange={(e) => {
            setQ(e.target.value);
            setSel(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setSel((s) => Math.min(shown.length - 1, s + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSel((s) => Math.max(0, s - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              const it = shown[sel];
              if (it) {
                shellUi.close();
                onPick(it);
              }
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              shellUi.close();
            }
          }}
        />
        <div className="picker-list" role="listbox" ref={listRef}>
          {shown.length === 0 && <div className="picker-empty">Nothing matches.</div>}
          {shown.map((it, i) => (
            <div
              key={it.id}
              data-index={i}
              role="option"
              aria-selected={i === sel}
              className={`picker-item${i === sel ? " selected" : ""}`}
              onMouseMove={() => setSel(i)}
              onClick={() => {
                shellUi.close();
                onPick(it);
              }}
            >
              <span className="picker-label">{it.label}</span>
              {it.detail && <span className="picker-detail">{it.detail}</span>}
              {it.hint && <kbd className="picker-hint">{it.hint}</kbd>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function bindingOf(cmd: CommandContribution, overrides: Record<string, string | null>): string | undefined {
  const b = cmd.id in overrides ? overrides[cmd.id] : cmd.keybinding;
  return b ? prettyChord(normalizeChord(b)) : undefined;
}

function CommandPalette() {
  const registry = useShellUi((s) => s.registry);
  const keymap = useSettings((s) => s.keymap);
  const items = useMemo(() => {
    if (!registry) return [];
    return [...registry.commands.values()]
      .filter((c) => !c.when || c.when())
      .filter((c) => c.id !== "shell.palette")
      .map((c) => {
        const item: PickItem = { id: c.id, label: c.category ? `${c.category}: ${c.title}` : c.title };
        const hint = bindingOf(c, keymap);
        if (hint) item.hint = hint;
        return item;
      })
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [registry, keymap]);
  return (
    <Picker
      label="Command palette"
      placeholder="Type a command"
      items={items}
      onPick={(it) => {
        const cmd = registry?.commands.get(it.id);
        if (!cmd) return;
        // Let the palette close and focus return before running.
        setTimeout(() => {
          Promise.resolve(cmd.run({ target: document.activeElement })).catch((err: unknown) =>
            console.error(`[margin] command ${cmd.id} failed`, err),
          );
        }, 0);
      }}
    />
  );
}

const SECTION_LABEL: Record<string, string> = {
  manuscript: "Manuscript",
  frontMatter: "Front matter",
  backMatter: "Back matter",
  notes: "Notes",
  research: "Research",
};

function QuickOpen() {
  const tree = useBinderTree();
  const items = useMemo(
    () =>
      flattenTree(tree).map((f) => ({
        id: f.node.id,
        label: f.node.title,
        detail: [SECTION_LABEL[f.section] ?? f.section, ...f.path].join(" › "),
      })),
    [tree],
  );
  return <Picker label="Quick open" placeholder="Go to a document" items={items} onPick={(it) => workbench.openDocument(it.id)} />;
}

function CheatSheet() {
  const registry = useShellUi((s) => s.registry);
  const keymap = useSettings((s) => s.keymap);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        shellUi.close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
  const groups = useMemo(() => {
    const g = new Map<string, { title: string; key: string }[]>();
    for (const c of registry?.commands.values() ?? []) {
      const key = bindingOf(c, keymap);
      if (!key) continue;
      const cat = c.category ?? "Other";
      g.set(cat, [...(g.get(cat) ?? []), { title: c.title, key }]);
    }
    return [...g.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [registry, keymap]);
  return (
    <div className="overlay-backdrop" onMouseDown={() => shellUi.close()}>
      <div className="cheatsheet" role="dialog" aria-label="Keyboard shortcuts" onMouseDown={(e) => e.stopPropagation()}>
        <div className="cheatsheet-head">
          <h2>Keyboard shortcuts</h2>
          <button className="link" onClick={() => {
            shellUi.close();
            openSettings("shortcuts");
          }}>
            Change shortcuts
          </button>
        </div>
        <div className="cheatsheet-grid">
          {groups.map(([cat, list]) => (
            <section key={cat}>
              <h3>{cat}</h3>
              <dl>
                {list.map((x) => (
                  <div key={x.title} className="cheat-row">
                    <dt>{x.title}</dt>
                    <dd>
                      <kbd>{x.key}</kbd>
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
          <section>
            <h3>Editing</h3>
            <dl>
              {[
                ["Italic", "Ctrl+I"],
                ["Bold", "Ctrl+B"],
                ["Undo", "Ctrl+Z"],
                ["Redo", "Ctrl+Y"],
                ["Restore literal text after smart typography", "Backspace"],
              ].map(([t, k]) => (
                <div key={t} className="cheat-row">
                  <dt>{t}</dt>
                  <dd>
                    <kbd>{k}</kbd>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        </div>
        <p className="muted">Esc closes this sheet. {settings.get().focusMode !== "off" ? "Focus mode is on (F9)." : ""}</p>
      </div>
    </div>
  );
}

function PromptBar() {
  const prompt = useWorkbench((s) => s.prompt);
  if (!prompt) return null;
  return <PromptBox key={prompt.id} prompt={prompt} />;
}

function PromptBox({ prompt }: { prompt: NonNullable<ReturnType<typeof useWorkbench.getState>["prompt"]> }) {
  const [value, setValue] = useState(prompt.initial ?? "");
  return (
    <div className="overlay-backdrop light" onMouseDown={() => prompt.resolve(null)}>
      <div className="picker prompt" role="dialog" aria-label={prompt.title} onMouseDown={(e) => e.stopPropagation()}>
        <label className="prompt-title" htmlFor="margin-prompt">
          {prompt.title}
        </label>
        <input
          id="margin-prompt"
          autoFocus
          className="picker-input"
          placeholder={prompt.placeholder}
          value={value}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              prompt.resolve(value);
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              prompt.resolve(null);
            }
          }}
        />
        <div className="prompt-hint muted">Enter to confirm · Esc to cancel</div>
      </div>
    </div>
  );
}

export function Overlays() {
  const overlay = useShellUi((s) => s.overlay);
  return (
    <>
      {overlay === "palette" && <CommandPalette />}
      {overlay === "quickOpen" && <QuickOpen />}
      {overlay === "cheatSheet" && <CheatSheet />}
      <PromptBar />
    </>
  );
}
