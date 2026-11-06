/**
 * Settings (SPEC §15) as a workspace tab. Built-in pages cover appearance,
 * writing, the project, modules and shortcuts; modules add pages through
 * the `settingsPages` extension point (e.g. Backups, Goals).
 */
import { useEffect, useMemo, useState } from "react";

import {
  settings,
  story,
  useProject,
  useSettings,
  workbench,
  type AppSettings,
  type EditorFont,
  type FocusMode,
  type Locale,
  type ProjectType,
  type Theme,
  type WorkspaceViewProps,
} from "@margin/sdk";

import { ErrorBoundary } from "./ErrorBoundary";
import { chordOf, normalizeChord, prettyChord } from "./keymap";
import { bindingOf } from "./Overlays";
import { useShellUi } from "./shellUi";

const BUILTIN = [
  { id: "editor", title: "Appearance" },
  { id: "writing", title: "Writing" },
  { id: "project", title: "Project" },
  { id: "modules", title: "Modules" },
  { id: "shortcuts", title: "Shortcuts" },
];

export function SettingsView(_props: WorkspaceViewProps) {
  const registry = useShellUi((s) => s.registry);
  const page = useShellUi((s) => s.settingsPage);
  const pages = [...BUILTIN, ...(registry?.settingsPages ?? []).map((p) => ({ id: p.id, title: p.title }))];
  const modulePage = registry?.settingsPages.find((p) => p.id === page);
  return (
    <div className="settings">
      <nav className="settings-nav" aria-label="Settings pages">
        {pages.map((p) => (
          <button
            key={p.id}
            className={`settings-nav-item${p.id === page ? " active" : ""}`}
            aria-current={p.id === page ? "page" : undefined}
            onClick={() => useShellUi.setState({ settingsPage: p.id })}
          >
            {p.title}
          </button>
        ))}
      </nav>
      <div className="settings-body">
        {page === "editor" && <AppearancePage />}
        {page === "writing" && <WritingPage />}
        {page === "project" && <ProjectPage />}
        {page === "modules" && <ModulesPage />}
        {page === "shortcuts" && <ShortcutsPage />}
        {modulePage && (
          <ErrorBoundary label={modulePage.title}>
            <modulePage.component />
          </ErrorBoundary>
        )}
      </div>
    </div>
  );
}

function Row({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="settings-row">
      <div className="settings-label">
        <span>{label}</span>
        {hint && <span className="settings-hint">{hint}</span>}
      </div>
      <div className="settings-control">{children}</div>
    </div>
  );
}

function Toggle({ k, label, hint }: { k: keyof AppSettings; label: string; hint?: string }) {
  const value = useSettings((s) => s[k]) as boolean;
  return (
    <Row label={label} {...(hint ? { hint } : {})}>
      <input type="checkbox" aria-label={label} checked={value} onChange={(e) => settings.update({ [k]: e.target.checked } as Partial<AppSettings>)} />
    </Row>
  );
}

function AppearancePage() {
  const s = useSettings((x) => x);
  return (
    <section>
      <h2>Appearance</h2>
      <Row label="Theme">
        <select aria-label="Theme" value={s.theme} onChange={(e) => settings.update({ theme: e.target.value as Theme })}>
          <option value="system">Match Windows</option>
          <option value="light">Light</option>
          <option value="sepia">Sepia</option>
          <option value="dark">Dark</option>
          <option value="contrast">High contrast</option>
        </select>
      </Row>
      <Row label="Editor font" hint="Independent of the export template.">
        <select aria-label="Editor font" value={s.editorFont} onChange={(e) => settings.update({ editorFont: e.target.value as EditorFont })}>
          <option value="literata">Literata (bundled)</option>
          <option value="georgia">Georgia</option>
          <option value="palatino">Palatino Linotype</option>
          <option value="system-serif">Cambria</option>
          <option value="sans">Segoe UI</option>
        </select>
      </Row>
      <Row label="Text size">
        <input type="range" min={14} max={28} step={1} aria-label="Text size" value={s.editorSize} onChange={(e) => settings.update({ editorSize: Number(e.target.value) })} />
        <span className="muted">{s.editorSize}px</span>
      </Row>
      <Row label="Line height">
        <input type="range" min={1.2} max={2.2} step={0.05} aria-label="Line height" value={s.lineHeight} onChange={(e) => settings.update({ lineHeight: Number(e.target.value) })} />
        <span className="muted">{s.lineHeight.toFixed(2)}</span>
      </Row>
      <Row label="Text width">
        <input type="range" min={28} max={60} step={1} aria-label="Text width" value={s.editorWidth} onChange={(e) => settings.update({ editorWidth: Number(e.target.value) })} />
        <span className="muted">{s.editorWidth}em</span>
      </Row>
      <Row label="Interface scale" hint="Makes the whole window larger or smaller.">
        <select aria-label="Interface scale" value={s.uiScale} onChange={(e) => settings.update({ uiScale: Number(e.target.value) })}>
          {[0.9, 1, 1.1, 1.25, 1.5].map((v) => (
            <option key={v} value={v}>
              {Math.round(v * 100)}%
            </option>
          ))}
        </select>
      </Row>
    </section>
  );
}

function WritingPage() {
  const focusMode = useSettings((x) => x.focusMode);
  return (
    <section>
      <h2>Writing</h2>
      <h3>Smart typography</h3>
      <Toggle k="smartQuotes" label="Curly quotes “ ” ‘ ’" />
      <Toggle k="smartDashes" label="Two hyphens become an em dash —" />
      <Toggle k="smartEllipsis" label="Three periods become an ellipsis …" />
      <p className="settings-hint">Press Backspace right after a replacement to get the characters you typed.</p>
      <h3>Focus</h3>
      <Row label="Focus mode" hint="F9 cycles through the modes.">
        <select aria-label="Focus mode" value={focusMode} onChange={(e) => settings.update({ focusMode: e.target.value as FocusMode })}>
          <option value="off">Off</option>
          <option value="sentence">Current sentence</option>
          <option value="paragraph">Current paragraph</option>
        </select>
      </Row>
      <Toggle k="typewriter" label="Typewriter scrolling" hint="Keeps the line you are writing at the same height. Shift+F9." />
      <h3>Spelling</h3>
      <Toggle k="spellcheck" label="Check spelling" hint="The dictionary (US or Canadian) is set per project." />
      <GhostSettings />
    </section>
  );
}

function GhostSettings() {
  const g = useSettings((x) => ({
    length: x.ghostLength,
    debounce: x.ghostDebounce,
    opacity: x.ghostOpacity,
  }));
  return (
    <>
      <h3>Ghost text</h3>
      <Toggle
        k="ghostEnabled"
        label="Suggest the next few words"
        hint="A dimmed suggestion appears after you pause. Tab accepts it, Esc dismisses it. Nothing is added unless you accept. Needs the model server (Settings → Models)."
      />
      <Toggle k="flowMode" label="Flow mode" hint="Ghost text off, everything else on. Ctrl+Alt+F." />
      <Row label="Length" hint="How much to suggest. Alt+] and Alt+[ cycle through alternatives.">
        <select
          aria-label="Ghost text length"
          value={g.length}
          onChange={(e) => settings.update({ ghostLength: e.target.value as AppSettings["ghostLength"] })}
        >
          <option value="word">A word</option>
          <option value="phrase">A phrase</option>
          <option value="sentence">A sentence</option>
        </select>
      </Row>
      <Row label="Pause before suggesting" hint="250 to 2000 ms. Typing again cancels the request.">
        <input
          type="number"
          min={250}
          max={2000}
          step={50}
          aria-label="Pause before suggesting, in milliseconds"
          value={g.debounce}
          onChange={(e) => {
            const v = Number(e.target.value);
            if (Number.isFinite(v)) settings.update({ ghostDebounce: Math.min(2000, Math.max(250, Math.round(v))) });
          }}
        />
        <span className="muted">ms</span>
      </Row>
      <Row label="Brightness" hint="How strongly the suggestion shows against your text.">
        <input
          type="range"
          min={20}
          max={80}
          step={5}
          aria-label="Ghost text brightness"
          value={Math.round(g.opacity * 100)}
          onChange={(e) => settings.update({ ghostOpacity: Number(e.target.value) / 100 })}
        />
        <span className="muted">{Math.round(g.opacity * 100)}%</span>
      </Row>
      <Toggle k="ghostItalic" label="Italic suggestions" />
      <Toggle
        k="ghostNeverPush"
        label="Never push text"
        hint="Clip the suggestion to the rest of the current line, so nothing below it moves. Accepting still inserts all of it."
      />
      <Toggle
        k="highlightAi"
        label="Highlight AI-assisted text"
        hint="Tint text you accepted from suggestions. Retyping it makes it yours. Nothing is written into your files."
      />
    </>
  );
}

function ProjectPage() {
  const project = useProject();
  if (!project) return null;
  return <ProjectForm key={project.id} project={project} />;
}

function ProjectForm({ project }: { project: NonNullable<ReturnType<typeof useProject>> }) {
  const [title, setTitle] = useState(project.title);
  const [author, setAuthor] = useState(project.author ?? "");
  const [statuses, setStatuses] = useState(project.statuses.join(", "));
  const save = (patch: Parameters<typeof story.project.update>[0]) =>
    story.project.update(patch).catch((e: unknown) => workbench.notify({ message: `Could not save: ${e instanceof Error ? e.message : String(e)}` }));
  return (
    <section>
      <h2>Project</h2>
      <Row label="Title">
        <input aria-label="Project title" value={title} onChange={(e) => setTitle(e.target.value)} onBlur={() => title.trim() && title !== project.title && void save({ title })} />
      </Row>
      <Row label="Author">
        <input aria-label="Author" value={author} onChange={(e) => setAuthor(e.target.value)} onBlur={() => author !== project.author && void save({ author })} />
      </Row>
      <Row label="Type">
        <select aria-label="Project type" value={project.projectType} onChange={(e) => void save({ projectType: e.target.value as ProjectType })}>
          <option value="fiction">Fiction</option>
          <option value="non-fiction">Non-fiction</option>
          <option value="both">Both</option>
        </select>
      </Row>
      <Row label="Spelling" hint="Dictionary and spelling conventions.">
        <select aria-label="Spelling" value={project.locale} onChange={(e) => void save({ locale: e.target.value as Locale })}>
          <option value="en-US">US English</option>
          <option value="en-CA">Canadian English</option>
        </select>
      </Row>
      <Row label="Statuses" hint="Comma-separated, in order.">
        <input
          aria-label="Statuses"
          value={statuses}
          onChange={(e) => setStatuses(e.target.value)}
          onBlur={() => void save({ statuses: statuses.split(",").map((x) => x.trim()).filter(Boolean) })}
        />
      </Row>
      <p className="settings-hint">Location: {project.root}</p>
    </section>
  );
}

function ModulesPage() {
  const registry = useShellUi((s) => s.registry);
  const disabled = useSettings((s) => s.disabledModules);
  return (
    <section>
      <h2>Modules</h2>
      <p className="settings-hint">Every feature is a module. Turning one off hides its panels, views and commands.</p>
      {(registry?.all ?? []).map((m) => (
        <Row key={m.id} label={m.name} {...(m.description ? { hint: m.description } : {})}>
          <input
            type="checkbox"
            aria-label={`Enable ${m.name}`}
            disabled={m.required}
            checked={m.required || !disabled.includes(m.id)}
            onChange={(e) =>
              settings.update({
                disabledModules: e.target.checked ? disabled.filter((x) => x !== m.id) : [...disabled, m.id],
              })
            }
          />
          {m.required && <span className="muted">Always on</span>}
        </Row>
      ))}
    </section>
  );
}

function ShortcutsPage() {
  const registry = useShellUi((s) => s.registry);
  const keymap = useSettings((s) => s.keymap);
  const [capturing, setCapturing] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(null);
        return;
      }
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      settings.update({ keymap: { ...settings.get().keymap, [capturing]: prettyChord(chordOf(e)) } });
      setCapturing(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing]);

  const cmds = useMemo(
    () =>
      [...(registry?.commands.values() ?? [])]
        .filter((c) => `${c.category ?? ""} ${c.title}`.toLowerCase().includes(filter.toLowerCase()))
        .sort((a, b) => `${a.category}${a.title}`.localeCompare(`${b.category}${b.title}`)),
    [registry, filter],
  );
  // Chords used by more than one command.
  const conflicts = useMemo(() => {
    const seen = new Map<string, number>();
    for (const c of registry?.commands.values() ?? []) {
      const b = bindingOf(c, keymap);
      if (b) seen.set(normalizeChord(b), (seen.get(normalizeChord(b)) ?? 0) + 1);
    }
    return seen;
  }, [registry, keymap]);

  return (
    <section>
      <h2>Shortcuts</h2>
      <input className="settings-filter" placeholder="Filter commands" aria-label="Filter commands" value={filter} onChange={(e) => setFilter(e.target.value)} />
      <table className="keymap-table">
        <tbody>
          {cmds.map((c) => {
            const b = bindingOf(c, keymap);
            const custom = c.id in keymap;
            const clash = b ? (conflicts.get(normalizeChord(b)) ?? 0) > 1 : false;
            return (
              <tr key={c.id}>
                <td className="muted">{c.category}</td>
                <td>{c.title}</td>
                <td>
                  {capturing === c.id ? (
                    <span className="capture">Press keys… (Esc cancels)</span>
                  ) : b ? (
                    <kbd className={clash ? "clash" : ""} title={clash ? "Also used by another command; the first that applies wins." : undefined}>
                      {b}
                    </kbd>
                  ) : (
                    <span className="muted">—</span>
                  )}
                </td>
                <td className="keymap-actions">
                  <button onClick={() => setCapturing(c.id)}>Change</button>
                  {b && (
                    <button onClick={() => settings.update({ keymap: { ...keymap, [c.id]: null } })} aria-label={`Remove shortcut for ${c.title}`}>
                      Remove
                    </button>
                  )}
                  {custom && (
                    <button
                      onClick={() => {
                        const next = { ...keymap };
                        delete next[c.id];
                        settings.update({ keymap: next });
                      }}
                    >
                      Reset
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
