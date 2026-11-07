import { useEffect } from "react";

import { CONTEXT_VIEW, openSource, uiState, useUiState, workbench, type Idea, type MarginTabProps } from "@margin/sdk";

import {
  AUTO_KEY,
  autoIdeas,
  dismissIdea,
  expandIdea,
  ideaToNote,
  loadPinned,
  pinIdea,
  requestIdeas,
  unpinIdea,
  useMargin,
} from "./store";

export const KIND_LABEL: Record<string, string> = {
  next: "What next",
  stakes: "Raise the stakes",
  reaction: "Hidden reaction",
  sensory: "Sensory detail",
  thread: "Loose thread",
  question: "Ask yourself",
  point: "Next point",
  counter: "Counter-argument",
  evidence: "Missing evidence",
  term: "Clarify a term",
  transition: "Transition",
};

export function IdeasTab({ active }: MarginTabProps) {
  const { ideas, ideasBusy, ideasNote, expanded, pinned } = useMargin();
  const auto = useUiState<boolean>(AUTO_KEY) ?? true;

  useEffect(() => {
    if (active) {
      useMargin.setState({ fresh: false });
      void loadPinned();
    }
  }, [active]);

  return (
    <div className="ideas-tab">
      <div className="margin-actions">
        <button className="primary" disabled={ideasBusy} onClick={() => void requestIdeas()} title="Ctrl+Shift+I">
          {ideasBusy ? "Thinking…" : "Get ideas"}
        </button>
        <label className="margin-auto" title="While this panel is open, offer ideas after you pause at the end of a paragraph.">
          <input type="checkbox" checked={auto} onChange={(e) => uiState.set(AUTO_KEY, e.target.checked)} /> At paragraph ends
        </label>
      </div>
      {ideasNote && <p className="muted">{ideasNote}</p>}
      {ideas.length === 0 && !ideasBusy && !ideasNote && (
        <p className="muted">
          Ideas are small nudges, never text to paste. Press <kbd>Ctrl+Shift+I</kbd>
          {autoIdeas() ? ", or pause for 20 seconds at the end of a paragraph," : ""} and they appear here.
        </p>
      )}
      {ideas.map((idea) => (
        <IdeaCard key={idea.id} idea={idea} expanded={expanded[idea.id]?.text} pinned={false} />
      ))}
      {pinned.length > 0 && (
        <section className="margin-pinned">
          <h4>Pinned</h4>
          {pinned.map((p) => (
            <IdeaCard key={p.idea.id} idea={p.idea} pinned expanded={undefined} />
          ))}
        </section>
      )}
    </div>
  );
}

function IdeaCard({ idea, expanded, pinned }: { idea: Idea; expanded: string | undefined; pinned: boolean }) {
  return (
    <article className="idea-card">
      <span className="idea-kind">{KIND_LABEL[idea.type] ?? idea.type}</span>
      <p>{idea.text}</p>
      {expanded && <p className="idea-expanded">{expanded}</p>}
      {idea.refs.length > 0 && (
        <p className="idea-refs">
          {idea.refs.map((r, i) => (
            <button
              key={i}
              className="link"
              title={`In ${r.docTitle}`}
              onClick={() => void openSource({ doc: r.doc, para: r.para, block: r.block, quote: r.quote })}
            >
              “{r.quote.length > 48 ? `${r.quote.slice(0, 48).trimEnd()}…` : r.quote}”
            </button>
          ))}
        </p>
      )}
      <div className="idea-actions">
        {pinned ? (
          <button className="link" onClick={() => void unpinIdea(idea.id)}>
            Unpin
          </button>
        ) : (
          <>
            <button className="link" onClick={() => void pinIdea(idea)}>
              Pin
            </button>
            <button className="link" onClick={() => dismissIdea(idea)}>
              Dismiss
            </button>
          </>
        )}
        {!pinned && !expanded && (
          <button className="link" onClick={() => void expandIdea(idea)}>
            Expand
          </button>
        )}
        <button className="link" onClick={() => void ideaToNote(idea)}>
          To note
        </button>
        <button className="link" title="See exactly what was sent to the model" onClick={() => workbench.openView(CONTEXT_VIEW, idea.contextId)}>
          Why?
        </button>
      </div>
    </article>
  );
}
