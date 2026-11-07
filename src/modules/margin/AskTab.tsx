import { Fragment, useState } from "react";

import { BIBLE_ENTRY_VIEW, CONTEXT_VIEW, openSource, workbench, type AskAnswer, type Citation, type MarginTabProps } from "@margin/sdk";

import { askBook, useMargin } from "./store";

/** Answers come only from Story Memory and the manuscript, with citations that jump to the passage. */
export function AskTab(_props: MarginTabProps) {
  const { asks, askBusy } = useMargin();
  const [q, setQ] = useState("");
  const submit = () => {
    if (!q.trim()) return;
    void askBook(q);
    setQ("");
  };
  return (
    <div className="ask-tab">
      <form
        className="ask-form"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <textarea
          rows={2}
          value={q}
          placeholder="Ask about your book: Who is the harbour pilot? What colour are Mara's eyes?"
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          aria-label="Question"
        />
        <button className="primary" type="submit" disabled={askBusy || !q.trim()}>
          {askBusy ? "Looking…" : "Ask"}
        </button>
      </form>
      {asks.length === 0 && (
        <p className="muted">Answers use only your Story Bible and your manuscript. If your book does not say, the answer says so.</p>
      )}
      {asks.map((a) => (
        <article key={a.id} className="ask-item">
          <p className="ask-q">{a.question}</p>
          {a.error && <p className="ai-warn">{a.error}</p>}
          {!a.answer && !a.error && <p className="muted">Looking through your book…</p>}
          {a.answer && <Answer answer={a.answer} />}
        </article>
      ))}
    </div>
  );
}

function cite(c: Citation) {
  if (c.source) void openSource({ doc: c.source.doc, para: c.source.para, block: c.source.block, quote: c.source.quote });
  else if (c.entityId) workbench.openView(BIBLE_ENTRY_VIEW, c.entityId);
}

function Answer({ answer }: { answer: AskAnswer }) {
  // Turn [1] markers into buttons that open the cited passage.
  const parts = answer.answer.split(/(\[\d+\])/g);
  return (
    <div className={`ask-answer${answer.found ? "" : " ask-none"}`}>
      <p>
        {parts.map((p, i) => {
          const m = /^\[(\d+)\]$/.exec(p);
          const c = m ? answer.citations.find((x) => x.label === Number(m[1])) : undefined;
          return c ? (
            <button key={i} className="cite-n" title={c.title} onClick={() => cite(c)}>
              {c.label}
            </button>
          ) : (
            <Fragment key={i}>{p}</Fragment>
          );
        })}
      </p>
      {answer.note && <p className="muted">{answer.note}</p>}
      {answer.citations.length > 0 && (
        <ol className="ask-cites">
          {answer.citations.map((c) => (
            <li key={c.label}>
              <button className="link" onClick={() => cite(c)} title="Show in the manuscript">
                <span className="cite-kind">{c.kind}</span> {c.title}
              </button>
              <q>{c.text.length > 160 ? `${c.text.slice(0, 160).trimEnd()}…` : c.text}</q>
            </li>
          ))}
        </ol>
      )}
      {answer.contextId && (
        <button className="link" onClick={() => workbench.openView(CONTEXT_VIEW, answer.contextId ?? undefined)}>
          Why?
        </button>
      )}
    </div>
  );
}
