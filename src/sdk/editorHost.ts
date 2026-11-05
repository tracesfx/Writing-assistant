/**
 * The editor as seen by other modules. The editor module registers one
 * handle per open editor; modules (search, history, capture) use these
 * instead of reaching into the editor's internals.
 */
import { useWorkbench, type PaneId } from "./workbench";

/** A position in a document's text blocks, as returned by `story.search`. */
export interface TextTarget {
  /** Index of the text block (paragraph or heading) in document order. */
  block: number;
  /** UTF-16 offsets within the block's text; a footnote counts as one. */
  start: number;
  end: number;
  /** The text expected there; used to re-find it if the blocks shifted. */
  text?: string;
}

export interface EditorHandleApi {
  readonly docId: string;
  readonly tabId: string;
  /** Plain text of the selection (empty if none). */
  selectedText(): string;
  /** Insert text at the cursor (replacing the selection) as one undo step. */
  insertText(text: string): void;
  /** Select a range and scroll it into view. */
  reveal(target: TextTarget): void;
  /** Move the cursor to a ProseMirror position. */
  setCursor(pos: number): void;
  cursor(): number;
  focus(): void;
  /** Select the text anchored to a comment; false if the anchor is gone. */
  revealComment(id: string): boolean;
  /** Remove a comment anchor (one undo step). */
  removeComment(id: string): void;
  /** Unsaved changes? */
  isDirty(): boolean;
  /** Save now. */
  flush(): Promise<void>;
}

const handles = new Map<string, EditorHandleApi>();
const pendingReveal = new Map<string, TextTarget | { pos: number }>();

export const editorHost = {
  register(h: EditorHandleApi): () => void {
    handles.set(h.tabId, h);
    return () => {
      if (handles.get(h.tabId) === h) handles.delete(h.tabId);
    };
  },
  /** The editor in the focused pane's active tab, if any. */
  active(): EditorHandleApi | undefined {
    const s = useWorkbench.getState();
    const pane = s.panes.find((p) => p.id === s.focusedPane);
    return pane?.activeTabId ? handles.get(pane.activeTabId) : undefined;
  },
  /** Every open editor of a document. */
  forDoc(docId: string): EditorHandleApi[] {
    return [...handles.values()].filter((h) => h.docId === docId);
  },
  all(): EditorHandleApi[] {
    return [...handles.values()];
  },
  /**
   * Open a document and reveal a range (or position) once its editor is
   * ready.
   */
  openAt(docId: string, target: TextTarget | { pos: number }, opts?: { pane?: PaneId }): void {
    const ws = useWorkbench.getState();
    const open = editorHost.forDoc(docId);
    ws.openDocument(docId, opts);
    // If an editor already exists, reveal after the tab switch renders.
    if (open.length > 0) {
      setTimeout(() => {
        const h = editorHost.active() ?? open[0];
        if (!h) return;
        if ("pos" in target) h.setCursor(target.pos);
        else h.reveal(target);
        h.focus();
      }, 0);
    } else {
      pendingReveal.set(docId, target);
    }
  },
  /** Called by a newly mounted editor. */
  takePendingReveal(docId: string): TextTarget | { pos: number } | undefined {
    const t = pendingReveal.get(docId);
    pendingReveal.delete(docId);
    return t;
  },
  /** Save every open editor (before an operation rewrites files). */
  async flushAll(): Promise<void> {
    await Promise.all([...handles.values()].map((h) => h.flush().catch(() => undefined)));
  },
};
