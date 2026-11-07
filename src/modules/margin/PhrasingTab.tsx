import { editorHost, workbench, CONTEXT_VIEW, type MarginTabProps } from "@margin/sdk";

import { requestPhrasing, useMargin } from "./store";

/** The selection is untouched until the author clicks Replace selection (one undo step) or Copy. */
export function PhrasingTab(_props: MarginTabProps) {
  const p = useMargin((s) => s.phrasing);

  function replace(text: string) {
    const h = editorHost.active();
    if (!h) return;
    // The selection must still be the one the alternatives were made for.
    if (h.selectionContext().text !== p?.selected) {
      workbench.notify({ message: "The selection changed. Select the text again and press Ctrl+Shift+P.", key: "margin.sel" });
      return;
    }
    h.insertText(text);
    h.focus();
  }

  return (
    <div className="phrasing-tab">
      <div className="margin-actions">
        <button className="primary" disabled={p?.busy} onClick={() => void requestPhrasing()} title="Ctrl+Shift+P">
          {p?.busy ? "Thinking…" : "Rephrase selection"}
        </button>
      </div>
      {!p && <p className="muted">Select some text in a scene, then press <kbd>Ctrl+Shift+P</kbd>. Your text is not touched until you choose Replace selection.</p>}
      {p?.error && <p className="muted">{p.error}</p>}
      {p && p.selected && (
        <blockquote className="phrasing-original" title="Your selection">
          {p.selected.length > 300 ? `${p.selected.slice(0, 300).trimEnd()}…` : p.selected}
        </blockquote>
      )}
      {p?.result?.note && <p className="muted">{p.result.note}</p>}
      {p?.result?.alternatives.map((alt, i) => (
        <article key={i} className="idea-card">
          <p>{alt}</p>
          <div className="idea-actions">
            <button className="link" onClick={() => replace(alt)}>
              Replace selection
            </button>
            <button
              className="link"
              onClick={() => {
                void navigator.clipboard?.writeText(alt).then(() => workbench.notify({ message: "Copied.", key: "margin.copy" }));
              }}
            >
              Copy
            </button>
          </div>
        </article>
      ))}
      {p?.result && p.result.contextId && (
        <button className="link" onClick={() => workbench.openView(CONTEXT_VIEW, p.result?.contextId)}>
          Why?
        </button>
      )}
    </div>
  );
}
