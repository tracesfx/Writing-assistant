/**
 * Find and replace across the project (SPEC §11.6). Searches run on the
 * saved files (open editors are saved first). Replace-all writes every
 * chosen hit in one operation, recorded as a history snapshot, and is
 * undone from its notice in one step.
 */
import { useEffect, useMemo, useRef } from "react";

import {
  editorHost,
  findNode,
  story,
  useActiveDocumentId,
  useBinderTree,
  workbench,
  type SearchHit,
  type SearchQuery,
  type SidebarPanelProps,
} from "@margin/sdk";

import { hitKey, useSearch } from "./state";

const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

function query(): SearchQuery {
  const s = useSearch.getState();
  return {
    text: s.text,
    regex: s.regex,
    caseSensitive: s.caseSensitive,
    wholeWord: s.wholeWord,
    scope: s.scope,
    scopeId: s.scope === "document" || s.scope === "chapter" ? s.scopeId : null,
  };
}

async function runSearch(): Promise<void> {
  const s = useSearch.getState();
  if (!s.text) {
    useSearch.setState({ hits: [], truncated: false, error: null });
    return;
  }
  useSearch.setState({ busy: true });
  try {
    await editorHost.flushAll();
    const r = await story.search.text(query());
    useSearch.setState({ hits: r.hits, truncated: r.truncated, excluded: {}, error: null, busy: false });
  } catch (e) {
    useSearch.setState({ hits: [], error: errorMessage(e), busy: false });
  }
}

async function replaceAll(): Promise<void> {
  const s = useSearch.getState();
  const chosen = s.hits.filter((h) => !s.excluded[hitKey(h)]);
  if (chosen.length === 0) return;
  useSearch.setState({ busy: true });
  try {
    await editorHost.flushAll();
    const report = await story.search.replace(
      query(),
      s.replacement,
      chosen.map((h) => ({ docId: h.docId, block: h.block, start: h.start, end: h.end })),
    );
    const n = report.replaced;
    const docs = report.docIds.length;
    workbench.notify({
      message: `Replaced ${n} occurrence${n === 1 ? "" : "s"} in ${docs} document${docs === 1 ? "" : "s"}`,
      ...(report.snapshotId
        ? {
            action: {
              label: "Undo",
              run: () =>
                void story.history
                  .restore(report.snapshotId!, report.docIds, "Undo replace-all")
                  .then(() => runSearch())
                  .catch((e: unknown) => workbench.notify({ message: `Could not undo: ${errorMessage(e)}` })),
            },
          }
        : {}),
      timeoutMs: 20_000,
    });
    await runSearch();
  } catch (e) {
    useSearch.setState({ error: errorMessage(e), busy: false });
  }
}

export function SearchPanel({ visible }: SidebarPanelProps) {
  const s = useSearch();
  const tree = useBinderTree();
  const activeDoc = useActiveDocumentId();
  const findRef = useRef<HTMLInputElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    if (visible) setTimeout(() => findRef.current?.select(), 0);
  }, [s.focusAt, visible]);

  // Search as you type (debounced).
  useEffect(() => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void runSearch(), 250);
    return () => clearTimeout(timer.current);
  }, [s.text, s.regex, s.caseSensitive, s.wholeWord, s.scope, s.scopeId]);

  const groups = useMemo(() => {
    const m = new Map<string, SearchHit[]>();
    for (const h of s.hits) m.set(h.docId, [...(m.get(h.docId) ?? []), h]);
    return [...m.entries()];
  }, [s.hits]);

  const included = s.hits.filter((h) => !s.excluded[hitKey(h)]).length;
  const scopeTitle =
    (s.scope === "document" || s.scope === "chapter") && s.scopeId ? (findNode(tree, s.scopeId)?.title ?? "") : "";

  const setScope = (v: string) => {
    if (v === "document" || v === "chapter") useSearch.setState({ scope: v, scopeId: activeDoc ?? s.scopeId });
    else useSearch.setState({ scope: v as SearchQuery["scope"] });
  };

  const toggle = (k: "regex" | "caseSensitive" | "wholeWord", label: string, short: string) => (
    <button
      className={`toggle${s[k] ? " on" : ""}`}
      aria-pressed={s[k]}
      title={label}
      aria-label={label}
      onClick={() => useSearch.setState({ [k]: !s[k] } as Partial<typeof s>)}
    >
      {short}
    </button>
  );

  return (
    <div className="panel search-panel">
      <div className="search-fields">
        <div className="search-row">
          <button
            className="twisty-btn"
            aria-label={s.showReplace ? "Hide replace" : "Show replace"}
            aria-expanded={s.showReplace}
            onClick={() => useSearch.setState({ showReplace: !s.showReplace })}
          >
            {s.showReplace ? "▾" : "▸"}
          </button>
          <input
            ref={findRef}
            className="search-input"
            placeholder="Find"
            aria-label="Find"
            value={s.text}
            onChange={(e) => useSearch.setState({ text: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runSearch();
              if (e.key === "Escape") editorHost.active()?.focus();
            }}
          />
          {toggle("caseSensitive", "Match case", "Aa")}
          {toggle("wholeWord", "Whole word", "ab")}
          {toggle("regex", "Regular expression", ".*")}
        </div>
        {s.showReplace && (
          <div className="search-row">
            <span className="twisty-spacer" />
            <input
              className="search-input"
              placeholder={s.regex ? "Replace ($1 for groups)" : "Replace"}
              aria-label="Replace with"
              value={s.replacement}
              onChange={(e) => useSearch.setState({ replacement: e.target.value })}
            />
            <button className="primary" disabled={included === 0 || s.busy} onClick={() => void replaceAll()} title="Replace every checked match">
              Replace {included > 0 ? included : ""}
            </button>
          </div>
        )}
        <div className="search-row">
          <label className="small muted" htmlFor="search-scope">
            In
          </label>
          <select id="search-scope" className="search-scope" value={s.scope} onChange={(e) => setScope(e.target.value)}>
            <option value="document">This document</option>
            <option value="chapter">This chapter or folder</option>
            <option value="manuscript">Manuscript</option>
            <option value="notes">Notes and research</option>
            <option value="all">Everything</option>
          </select>
          {scopeTitle && <span className="small muted ellipsis">{scopeTitle}</span>}
        </div>
      </div>
      {s.error && <p className="error pad small">{s.error}</p>}
      {!s.error && s.text && (
        <p className="muted pad small">
          {s.busy ? "Searching…" : `${s.hits.length}${s.truncated ? "+" : ""} match${s.hits.length === 1 ? "" : "es"} in ${groups.length} document${groups.length === 1 ? "" : "s"}`}
        </p>
      )}
      <div className="search-results" role="tree" aria-label="Matches">
        {groups.map(([docId, hits]) => (
          <div key={docId} role="group" className="search-group">
            <div className="search-doc">
              {s.showReplace && (
                <input
                  type="checkbox"
                  aria-label={`Include matches in ${hits[0]?.title}`}
                  checked={hits.every((h) => !s.excluded[hitKey(h)])}
                  onChange={(e) => {
                    const next = { ...s.excluded };
                    for (const h of hits) {
                      if (e.target.checked) delete next[hitKey(h)];
                      else next[hitKey(h)] = true;
                    }
                    useSearch.setState({ excluded: next });
                  }}
                />
              )}
              <span className="search-doc-title">{hits[0]?.title}</span>
              <span className="count">{hits.length}</span>
            </div>
            {hits.map((h) => (
              <div key={hitKey(h)} role="treeitem" className="search-hit">
                {s.showReplace && (
                  <input
                    type="checkbox"
                    aria-label="Include this match"
                    checked={!s.excluded[hitKey(h)]}
                    onChange={(e) => {
                      const next = { ...s.excluded };
                      if (e.target.checked) delete next[hitKey(h)];
                      else next[hitKey(h)] = true;
                      useSearch.setState({ excluded: next });
                    }}
                  />
                )}
                <button
                  className="search-hit-text"
                  onClick={() => editorHost.openAt(h.docId, { block: h.block, start: h.start, end: h.end, text: h.matched })}
                >
                  <span className="muted">{h.before}</span>
                  {s.showReplace && !s.excluded[hitKey(h)] ? (
                    <>
                      <del>{h.matched}</del>
                      <ins>{s.regex ? previewRegex(h.matched) : s.replacement}</ins>
                    </>
                  ) : (
                    <mark>{h.matched}</mark>
                  )}
                  <span className="muted">{h.after}</span>
                </button>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

/** Preview of a regex replacement for one match (JS semantics close to Rust's for $1/${name}). */
function previewRegex(matched: string): string {
  const s = useSearch.getState();
  try {
    const re = new RegExp(s.wholeWord ? `\\b(?:${s.text})\\b` : s.text, s.caseSensitive ? "u" : "iu");
    return matched.replace(re, s.replacement.replace(/\$\{(\w+)\}/g, "$<$1>"));
  } catch {
    return s.replacement;
  }
}
