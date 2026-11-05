import { useState } from "react";

import { editorHost, findNode, useBinderTree, useWorkbench } from "@margin/sdk";

import { lastEdited, type LastEdited } from "./positions";
import { useEditorStore, type EditorStatus } from "./state";

function useFocusedStatus(): EditorStatus | undefined {
  const tabId = useWorkbench((s) => s.panes.find((p) => p.id === s.focusedPane)?.activeTabId);
  return useEditorStore((s) => (tabId ? s.byTab[tabId] : undefined));
}

const LABEL: Record<string, string> = {
  saved: "Saved",
  dirty: "Edited",
  saving: "Saving…",
  conflict: "Changed on disk",
  error: "Not saved — retrying",
};

export function SaveStateItem() {
  const st = useFocusedStatus();
  if (!st) return null;
  const s = st.save.status;
  return (
    <span className={`status-save status-save--${s}`} title={st.save.error ?? undefined} aria-live="polite">
      {LABEL[s]}
    </span>
  );
}

export function WordCountItem() {
  const st = useFocusedStatus();
  if (!st) return null;
  const fmt = (n: number) => n.toLocaleString("en-US");
  return (
    <span className="status-words">
      {st.selectionWords > 0 ? `${fmt(st.selectionWords)} of ${fmt(st.words)} words` : `${fmt(st.words)} words`}
    </span>
  );
}

/**
 * "Where was I?" (SPEC §11.4): after opening a project, a quiet line shows
 * the last edited document with a jump-back link, until the author goes
 * there or starts editing elsewhere.
 */
export function WhereWasIItem() {
  const tree = useBinderTree();
  const [last, setLast] = useState<LastEdited | undefined>(() => lastEdited());
  const activeDoc = useWorkbench((s) => s.panes.find((p) => p.id === s.focusedPane)?.tabs.find((t) => t.id === s.panes.find((p) => p.id === s.focusedPane)?.activeTabId)?.resourceId);
  const anyDirty = useEditorStore((s) => Object.values(s.byTab).some((t) => t.save.status === "dirty"));

  if (last && (activeDoc === last.docId || anyDirty)) setLast(undefined);

  if (!last) return null;
  const node = findNode(tree, last.docId);
  if (!node) return null;
  return (
    <span className="status-where">
      <span className="muted">Last edited: {node.title}</span>{" "}
      <button className="link" title="Jump back (Ctrl+Alt+←)" onClick={() => editorHost.openAt(last.docId, { pos: last.pos })}>
        Jump back
      </button>
    </span>
  );
}
