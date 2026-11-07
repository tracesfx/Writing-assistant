/** The Story Bible browser (SPEC §9.6): a left sidebar panel. */
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  BIBLE_ENTRY_VIEW,
  memory,
  useMemoryVersion,
  useWorkbench,
  workbench,
  type EntitySummary,
  type SidebarPanelProps,
  type TypeInfo,
} from "@margin/sdk";

import { fail } from "./common";

export function BiblePanel({ visible }: SidebarPanelProps) {
  const version = useMemoryVersion();
  const [entries, setEntries] = useState<EntitySummary[]>([]);
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);
  const [newType, setNewType] = useState("character");
  const [newName, setNewName] = useState("");
  const active = useWorkbench((s) => {
    const pane = s.panes.find((p) => p.id === s.focusedPane);
    const tab = pane?.tabs.find((t) => t.id === pane.activeTabId);
    return tab?.viewId === BIBLE_ENTRY_VIEW ? tab.resourceId : undefined;
  });

  const load = useCallback(async () => {
    try {
      const [e, t] = await Promise.all([memory.entities.list({ query }), memory.entities.types()]);
      setEntries(e);
      setTypes(t);
    } catch {
      setEntries([]);
    }
  }, [query]);

  useEffect(() => {
    if (!visible) return;
    const h = setTimeout(() => void load(), query ? 150 : 0);
    return () => clearTimeout(h);
  }, [visible, version, load, query]);

  const groups = useMemo(() => {
    const by = new Map<string, EntitySummary[]>();
    for (const e of entries) by.set(e.type, [...(by.get(e.type) ?? []), e]);
    return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [entries]);
  const label = (id: string) => types.find((t) => t.id === id)?.title ?? id;

  async function create() {
    const name = newName.trim();
    if (!name) return;
    try {
      const e = await memory.entities.create(newType, name);
      setNewName("");
      setAdding(false);
      workbench.openView(BIBLE_ENTRY_VIEW, e.id);
    } catch (err) {
      fail("Could not add the entry", err);
    }
  }

  return (
    <div className="bible-panel">
      <div className="panel-toolbar">
        <span className="panel-title">Story Bible</span>
        <button onClick={() => setAdding((a) => !a)} aria-expanded={adding} title="New entry">
          + New
        </button>
      </div>
      {adding && (
        <form
          className="bible-new"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <select value={newType} onChange={(e) => setNewType(e.target.value)} aria-label="Entry type">
            {types.map((t) => (
              <option key={t.id} value={t.id}>
                {t.title}
              </option>
            ))}
          </select>
          <input autoFocus placeholder="Name" value={newName} onChange={(e) => setNewName(e.target.value)} />
          <button className="primary" type="submit" disabled={!newName.trim()}>
            Add
          </button>
        </form>
      )}
      <input className="bible-search" type="search" placeholder="Search names and facts" value={query} onChange={(e) => setQuery(e.target.value)} />
      <div className="bible-list">
        {entries.length === 0 && (
          <p className="muted bible-empty">
            {query ? "Nothing matches." : "No entries yet. Add one, or let Margin read your scenes while you rest: it fills this in as inferred entries you can confirm."}
          </p>
        )}
        {groups.map(([type, list]) => (
          <section key={type}>
            <h4 className="bible-group">
              {label(type)} <span className="muted">{list.length}</span>
            </h4>
            <ul>
              {list.map((e) => (
                <li key={e.id}>
                  <button className={`bible-item${e.id === active ? " active" : ""}`} onClick={() => workbench.openView(BIBLE_ENTRY_VIEW, e.id)}>
                    <span className={e.status === "inferred" ? "mem-inferred-text" : ""}>{e.name}</span>
                    <span className="muted bible-count" title={`${e.mentionCount} mentions · ${e.factCount} facts`}>
                      {e.mentionCount > 0 ? e.mentionCount : ""}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
