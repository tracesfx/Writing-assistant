import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import type { Locale, ProjectType } from "@margin/sdk";
import type { RecentProject } from "../sdk/bindings";

interface Props {
  recent: RecentProject[];
  error: string | null;
  onOpen(root: string): void;
  onCreate(req: { parentDir: string; title: string; author: string; projectType: ProjectType; locale: Locale }): void;
  onForget(root: string): void;
}

export function Welcome({ recent, error, onOpen, onCreate, onForget }: Props) {
  const [title, setTitle] = useState("");
  const [author, setAuthor] = useState("");
  const [projectType, setType] = useState<ProjectType>("fiction");
  const [locale, setLocale] = useState<Locale>("en-US");

  async function pickOpen() {
    const dir = await open({ directory: true, title: "Open a Margin project folder" });
    if (typeof dir === "string") onOpen(dir);
  }

  async function create(e: React.FormEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    const parentDir = await open({ directory: true, title: "Choose where to create the project folder" });
    if (typeof parentDir === "string") onCreate({ parentDir, title, author, projectType, locale });
  }

  return (
    <main className="welcome">
      <h1>Margin</h1>
      {error && (
        <p className="welcome-error" role="alert">
          {error}
        </p>
      )}
      <div className="welcome-columns">
        <section>
          <h2>New project</h2>
          <form onSubmit={(e) => void create(e)}>
            <label>
              Title
              <input value={title} onChange={(e) => setTitle(e.target.value)} autoFocus required />
            </label>
            <label>
              Author
              <input value={author} onChange={(e) => setAuthor(e.target.value)} />
            </label>
            <label>
              Type
              <select value={projectType} onChange={(e) => setType(e.target.value as ProjectType)}>
                <option value="fiction">Fiction</option>
                <option value="non-fiction">Non-fiction</option>
                <option value="both">Both</option>
              </select>
            </label>
            <label>
              Spelling
              <select value={locale} onChange={(e) => setLocale(e.target.value as Locale)}>
                <option value="en-US">English (US)</option>
                <option value="en-CA">English (Canada)</option>
              </select>
            </label>
            <button type="submit" className="primary" disabled={!title.trim()}>
              Create…
            </button>
          </form>
        </section>
        <section>
          <h2>Open</h2>
          <button onClick={() => void pickOpen()}>Open project folder…</button>
          {recent.length > 0 && (
            <ul className="recent">
              {recent.map((r) => (
                <li key={r.root}>
                  <button className="link" onClick={() => onOpen(r.root)} title={r.root}>
                    {r.title}
                  </button>
                  <span className="muted">{r.root}</span>
                  <button className="row-action" aria-label={`Remove ${r.title} from the list`} onClick={() => onForget(r.root)}>
                    ×
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </main>
  );
}
