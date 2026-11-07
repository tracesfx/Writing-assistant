/** One bible entry (SPEC §9.6): facts with status and source, relations, mentions, history. */
import { useEffect, useState } from "react";

import {
  BIBLE_ENTRY_VIEW,
  memory,
  openSource,
  useMemoryVersion,
  useWorkbench,
  workbench,
  type EntityDetail,
  type EntitySummary,
  type Fact,
  type TypeInfo,
  type WorkspaceViewProps,
} from "@margin/sdk";

import { EntryLink, SourceLink, StatusMark, fail } from "./common";

type Patch = Parameters<typeof memory.entities.update>[1];

type Raw = EntityDetail["entity"];
type Entry = Omit<Raw, "aliases" | "fields" | "facts" | "notes"> & {
  aliases: NonNullable<Raw["aliases"]>;
  fields: NonNullable<Raw["fields"]>;
  facts: (Omit<Fact, "history"> & { history: NonNullable<Fact["history"]> })[];
  notes: string;
};
type Detail = Omit<EntityDetail, "entity"> & { entity: Entry };

/** The backend leaves empty lists out of its JSON; the view works with real arrays. */
function normalise(d: EntityDetail): Detail {
  const e = d.entity;
  return {
    ...d,
    entity: {
      ...e,
      aliases: e.aliases ?? [],
      fields: e.fields ?? {},
      facts: (e.facts ?? []).map((f) => ({ ...f, history: f.history ?? [] })),
      notes: e.notes ?? "",
    },
  };
}

export function EntryView({ resourceId }: WorkspaceViewProps) {
  const version = useMemoryVersion();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [types, setTypes] = useState<TypeInfo[]>([]);
  const [missing, setMissing] = useState(false);
  const [tab, setTab] = useState<"overview" | "history">("overview");

  useEffect(() => {
    if (!resourceId) return;
    let live = true;
    memory.entities
      .get(resourceId)
      .then((d) => {
        if (!live) return;
        setDetail(normalise(d));
        setMissing(false);
      })
      .catch(() => live && setMissing(true));
    return () => {
      live = false;
    };
  }, [resourceId, version]);
  useEffect(() => {
    void memory.entities
      .types()
      .then(setTypes)
      .catch(() => undefined);
  }, []);

  if (missing)
    return (
      <div className="bible-entry">
        <p className="muted">This entry no longer exists.</p>
      </div>
    );
  if (!detail)
    return (
      <div className="bible-entry">
        <p className="muted">Loading…</p>
      </div>
    );
  const e = detail.entity;

  const update = (patch: Patch) => memory.entities.update(e.id, patch).catch((err: unknown) => fail("Could not save", err));

  async function remove() {
    try {
      const gone = await memory.entities.delete(e.id);
      useWorkbench.getState().closeResource(e.id);
      workbench.notify({
        message: `Deleted ${gone.name}.`,
        action: { label: "Undo", run: () => void memory.entities.restore(gone).then(() => workbench.openView(BIBLE_ENTRY_VIEW, gone.id)) },
        timeoutMs: 10000,
      });
    } catch (err) {
      fail("Could not delete", err);
    }
  }

  const typeOptions = types.some((t) => t.id === e.type) ? types : [...types, { id: e.type, title: e.type }];
  return (
    <div className="bible-entry">
      <header className="bible-head">
        <input
          key={e.id + e.name}
          className="bible-name"
          defaultValue={e.name}
          aria-label="Name"
          onBlur={(ev) => ev.target.value.trim() && ev.target.value.trim() !== e.name && void update({ name: ev.target.value })}
          onKeyDown={(ev) => ev.key === "Enter" && ev.currentTarget.blur()}
        />
        <select value={e.type} aria-label="Type" onChange={(ev) => void update({ type: ev.target.value })}>
          {typeOptions.map((t) => (
            <option key={t.id} value={t.id}>
              {t.title}
            </option>
          ))}
        </select>
        <StatusMark status={e.status} />
        {e.status === "inferred" && (
          <button
            className="primary"
            onClick={() => void memory.makeCanon([{ kind: "entity", id: e.id, entity: null }]).catch((err: unknown) => fail("Could not confirm", err))}
          >
            ✓ make canon
          </button>
        )}
        <span className="grow" />
        <button className="link danger" onClick={() => void remove()}>
          Delete
        </button>
      </header>
      <nav className="ai-tabs" role="tablist">
        <button role="tab" aria-selected={tab === "overview"} onClick={() => setTab("overview")}>
          Overview
        </button>
        <button role="tab" aria-selected={tab === "history"} onClick={() => setTab("history")}>
          History
        </button>
      </nav>
      {tab === "history" ? <History entry={e} /> : <Overview detail={detail} update={update} />}
    </div>
  );
}

function Overview({ detail, update }: { detail: Detail; update: (p: Patch) => Promise<unknown> }) {
  const e = detail.entity;
  const [alias, setAlias] = useState("");
  const [fieldKey, setFieldKey] = useState("");
  const [fieldVal, setFieldVal] = useState("");

  const aliases = e.aliases.map((a) => a.text);
  return (
    <div className="bible-body">
      <section>
        <h3>Also called</h3>
        <div className="bible-chips">
          {e.aliases.map((a) => (
            <span
              key={a.text}
              className={`bible-chip${a.status === "inferred" ? " mem-inferred-text" : ""}`}
              title={a.status === "inferred" ? "Proposed by the model. Editing this list confirms it." : "Canon"}
            >
              {a.text}
              <button aria-label={`Remove ${a.text}`} onClick={() => void update({ aliases: aliases.filter((x) => x !== a.text) })}>
                ×
              </button>
            </span>
          ))}
          <form
            onSubmit={(ev) => {
              ev.preventDefault();
              if (alias.trim()) void update({ aliases: [...aliases, alias.trim()] }).then(() => setAlias(""));
            }}
          >
            <input className="bible-add" placeholder="Add a name…" value={alias} onChange={(ev) => setAlias(ev.target.value)} />
          </form>
        </div>
      </section>

      {e.thread && (
        <section>
          <h3>Plot thread</h3>
          <select
            value={e.thread.status}
            aria-label="Thread status"
            onChange={(ev) => void update({ threadStatus: ev.target.value as "open" | "resolved" | "dropped" })}
          >
            <option value="open">Open</option>
            <option value="resolved">Resolved</option>
            <option value="dropped">Dropped</option>
          </select>{" "}
          <span className="muted">{e.thread.statusCanon ? "Set by you" : "Read from the text"}</span>
        </section>
      )}

      <section>
        <h3>Fields</h3>
        <dl className="bible-fields">
          {Object.entries(e.fields).map(([k, v]) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd>
                <input
                  defaultValue={v}
                  aria-label={k}
                  onBlur={(ev) => ev.target.value !== v && void update({ fields: { ...e.fields, [k]: ev.target.value } })}
                />
                <button
                  className="link"
                  aria-label={`Remove ${k}`}
                  onClick={() => void update({ fields: Object.fromEntries(Object.entries(e.fields).filter(([x]) => x !== k)) })}
                >
                  ×
                </button>
              </dd>
            </div>
          ))}
        </dl>
        <form
          className="bible-row"
          onSubmit={(ev) => {
            ev.preventDefault();
            if (fieldKey.trim() && fieldVal.trim())
              void update({ fields: { ...e.fields, [fieldKey.trim()]: fieldVal.trim() } }).then(() => {
                setFieldKey("");
                setFieldVal("");
              });
          }}
        >
          <input placeholder="Field (age, job…)" value={fieldKey} onChange={(ev) => setFieldKey(ev.target.value)} />
          <input placeholder="Value" value={fieldVal} onChange={(ev) => setFieldVal(ev.target.value)} />
          <button type="submit" disabled={!fieldKey.trim() || !fieldVal.trim()}>
            Add
          </button>
        </form>
      </section>

      <section>
        <h3>Your notes</h3>
        <textarea
          key={e.id + e.updated}
          className="bible-notes"
          defaultValue={e.notes}
          rows={5}
          placeholder="Anything you want to remember about this."
          onBlur={(ev) => ev.target.value.trim() !== e.notes.trim() && void update({ notes: ev.target.value })}
        />
      </section>

      <Facts entry={e} />
      <Relations detail={detail} />

      <section>
        <h3>Where it appears</h3>
        {detail.mentions.length === 0 ? (
          <p className="muted">Not named in any scene yet.</p>
        ) : (
          <ul className="bible-mentions">
            {detail.mentions.map((m) => (
              <li key={m.sceneId}>
                <button
                  className="link"
                  title="Open the first mention"
                  onClick={() =>
                    m.first
                      ? void openSource({ doc: m.first.doc, para: m.first.para ?? null, block: m.first.block ?? null, quote: m.first.quote.slice(0, 60) })
                      : workbench.openDocument(m.sceneId)
                  }
                >
                  {m.sceneTitle}
                </button>{" "}
                <span className="muted">
                  {m.count} mention{m.count === 1 ? "" : "s"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function Facts({ entry }: { entry: Entry }) {
  const [text, setText] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  async function add() {
    if (!text.trim()) return;
    try {
      await memory.facts.add(entry.id, text.trim());
      setText("");
    } catch (err) {
      fail("Could not add the fact", err);
    }
  }
  async function save(f: Fact) {
    setEditing(null);
    if (draft.trim() && draft.trim() !== f.text)
      await memory.facts.update(entry.id, f.id, draft, f.field ?? null).catch((err: unknown) => fail("Could not save", err));
  }

  return (
    <section>
      <h3>Facts</h3>
      {entry.facts.length === 0 && <p className="muted">No facts yet.</p>}
      <ul className="bible-facts">
        {entry.facts.map((f) => (
          <li key={f.id} className={f.status}>
            <div className="bible-fact-main">
              {editing === f.id ? (
                <input
                  autoFocus
                  value={draft}
                  onChange={(ev) => setDraft(ev.target.value)}
                  onBlur={() => void save(f)}
                  onKeyDown={(ev) => {
                    if (ev.key === "Enter") void save(f);
                    if (ev.key === "Escape") setEditing(null);
                  }}
                />
              ) : (
                <button
                  className={`bible-fact-text${f.status === "inferred" ? " mem-inferred-text" : ""}`}
                  title="Click to edit. Editing confirms it as canon."
                  onClick={() => {
                    setDraft(f.text);
                    setEditing(f.id);
                  }}
                >
                  {f.text}
                </button>
              )}
              {f.field && <span className="muted bible-field-tag">{f.field}</span>}
              {f.conflictsWith && (
                <span className="mem-conflict" title="This disagrees with a canon fact about the same thing.">
                  conflicts with canon
                </span>
              )}
            </div>
            <div className="bible-fact-meta">
              <StatusMark status={f.status} />
              {f.source && <SourceLink source={f.source} />}
              {f.status === "inferred" && (
                <button
                  className="link"
                  onClick={() => void memory.makeCanon([{ kind: "fact", id: f.id, entity: entry.id }]).catch((err: unknown) => fail("Could not confirm", err))}
                >
                  ✓ make canon
                </button>
              )}
              <button
                className="link danger"
                onClick={() => {
                  void memory.facts.delete(entry.id, f.id).then(() =>
                    workbench.notify({
                      message: "Fact deleted.",
                      action: { label: "Undo", run: () => void memory.facts.add(entry.id, f.text, f.field ?? null, f.source ?? null) },
                    }),
                  );
                }}
              >
                Delete
              </button>
            </div>
          </li>
        ))}
      </ul>
      <form
        className="bible-row"
        onSubmit={(ev) => {
          ev.preventDefault();
          void add();
        }}
      >
        <input placeholder="Add a fact you know to be true…" value={text} onChange={(ev) => setText(ev.target.value)} />
        <button type="submit" disabled={!text.trim()}>
          Add
        </button>
      </form>
    </section>
  );
}

function Relations({ detail }: { detail: Detail }) {
  const e = detail.entity;
  const [kind, setKind] = useState("");
  const [target, setTarget] = useState("");
  const [options, setOptions] = useState<EntitySummary[]>([]);
  useEffect(() => {
    void memory.entities.list().then((l) => setOptions(l.filter((x) => x.id !== e.id)));
  }, [e.id, detail]);

  return (
    <section>
      <h3>Relations</h3>
      {detail.relationsOut.length + detail.relationsIn.length === 0 && <p className="muted">No relations yet.</p>}
      <ul className="bible-facts">
        {detail.relationsOut.map((r) => (
          <li key={r.id} className={r.status}>
            <div className="bible-fact-main">
              <span className={r.status === "inferred" ? "mem-inferred-text" : ""}>
                {r.kind} {r.directed ? "→" : "↔"} <EntryLink id={r.to} name={r.toName} />
              </span>
            </div>
            <div className="bible-fact-meta">
              <StatusMark status={r.status} />
              {r.source && <SourceLink source={r.source} />}
              {r.status === "inferred" && (
                <button className="link" onClick={() => void memory.makeCanon([{ kind: "relation", id: r.id, entity: e.id }])}>
                  ✓ make canon
                </button>
              )}
              <button className="link danger" onClick={() => void memory.relations.delete(e.id, r.id)}>
                Delete
              </button>
            </div>
          </li>
        ))}
        {detail.relationsIn.map((r) => (
          <li key={r.id} className={r.status}>
            <div className="bible-fact-main">
              <span className={r.status === "inferred" ? "mem-inferred-text" : ""}>
                <EntryLink id={r.from} name={r.fromName} /> {r.directed ? "→" : "↔"} {r.kind} (this entry)
              </span>
            </div>
            <div className="bible-fact-meta">
              <StatusMark status={r.status} />
              {r.source && <SourceLink source={r.source} />}
            </div>
          </li>
        ))}
      </ul>
      <form
        className="bible-row"
        onSubmit={(ev) => {
          ev.preventDefault();
          if (kind.trim() && target)
            void memory.relations
              .add(e.id, target, kind)
              .then(() => setKind(""))
              .catch((err: unknown) => fail("Could not add the relation", err));
        }}
      >
        <input placeholder="Relation (sibling, mentor, owns…)" value={kind} onChange={(ev) => setKind(ev.target.value)} list="relation-kinds" />
        <datalist id="relation-kinds">
          {["sibling", "parent", "ally", "rival", "romantic", "mentor", "member-of", "located-in", "owns"].map((k) => (
            <option key={k} value={k} />
          ))}
        </datalist>
        <select value={target} onChange={(ev) => setTarget(ev.target.value)} aria-label="Related entry">
          <option value="">to…</option>
          {options.map((o) => (
            <option key={o.id} value={o.id}>
              {o.name}
            </option>
          ))}
        </select>
        <button type="submit" disabled={!kind.trim() || !target}>
          Add
        </button>
      </form>
    </section>
  );
}

/** Every earlier reading of every fact in this entry, newest first. */
function History({ entry }: { entry: Entry }) {
  const rows = entry.facts.flatMap((f) => f.history.map((h) => ({ ...h, current: f.text }))).sort((a, b) => b.at.localeCompare(a.at));
  return (
    <div className="bible-body">
      <p className="muted">
        Created {entry.created.slice(0, 10)} by {entry.createdBy === "ai" ? "the model" : "you"}. Updated {entry.updated.slice(0, 10)}.
      </p>
      {rows.length === 0 ? (
        <p className="muted">No earlier versions: nothing here has been replaced yet.</p>
      ) : (
        <ul className="bible-facts">
          {rows.map((h, i) => (
            <li key={i}>
              <div className="bible-fact-main">
                <s className="muted">{h.text}</s> → {h.current}
              </div>
              <div className="bible-fact-meta">
                <span className="muted">
                  {h.at.slice(0, 10)} · {h.note}
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
