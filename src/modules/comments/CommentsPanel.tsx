/**
 * Inline comments (SPEC §11.2) for the active document. The anchor is a
 * mark in the text; the body is stored in `comments/<docId>.json`. Deleting
 * is undoable from the notice.
 */
import { useEffect, useRef, useState } from "react";

import {
  editorHost,
  events,
  story,
  useActiveDocumentId,
  useUiState,
  workbench,
  type Comment,
  type SidebarPanelProps,
} from "@margin/sdk";

interface FocusReq {
  docId: string;
  id: string;
  at: number;
}

export function CommentsPanel({ visible }: SidebarPanelProps) {
  const docId = useActiveDocumentId();
  const [list, setList] = useState<Comment[]>([]);
  const [showResolved, setShowResolved] = useState(false);
  const focus = useUiState<FocusReq>("comments.focus");
  const refs = useRef(new Map<string, HTMLTextAreaElement>());

  useEffect(() => {
    if (!docId || !visible) return;
    let live = true;
    const load = () =>
      story.comments
        .list(docId)
        .then((l) => live && setList(l))
        .catch(() => live && setList([]));
    void load();
    // Split, merge, restore and replace-all can move comments between documents.
    const offs = [
      events.on("doc.contentReplaced", (e) => e.docIds.includes(docId) && void load()),
      events.on("binder.changed", (e) => e.docIds.includes(docId) && void load()),
    ];
    return () => {
      live = false;
      for (const off of offs) off();
    };
  }, [docId, visible, focus?.at]);

  // A new comment (or a click on an anchor) focuses its body.
  useEffect(() => {
    if (!focus || focus.docId !== docId) return;
    workbench.showPanel("comments.panel");
    const t = setTimeout(() => refs.current.get(focus.id)?.focus(), 50);
    return () => clearTimeout(t);
  }, [focus?.at, list.length]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!docId) return <div className="panel"><p className="muted pad">Open a document to see its comments.</p></div>;

  const put = async (c: Comment) => {
    try {
      setList(await story.comments.put(docId, c));
    } catch (e) {
      workbench.notify({ message: `Could not save the comment: ${e instanceof Error ? e.message : String(e)}` });
    }
  };

  const remove = async (c: Comment) => {
    const editor = editorHost.forDoc(docId)[0];
    editor?.removeComment(c.id);
    setList(await story.comments.delete(docId, c.id));
    workbench.notify({
      message: "Comment deleted",
      action: {
        label: "Undo",
        run: () => {
          void story.comments.put(docId, c).then(setList);
          workbench.notify({ message: "Comment restored. Press Ctrl+Z in the text to restore its highlight.", timeoutMs: 5000 });
        },
      },
      timeoutMs: 10_000,
    });
  };

  const shown = list.filter((c) => showResolved || !c.resolved);
  return (
    <div className="panel comments">
      <div className="panel-toolbar">
        <span className="panel-title">Comments</span>
        <label className="small">
          <input type="checkbox" checked={showResolved} onChange={(e) => setShowResolved(e.target.checked)} /> Resolved
        </label>
      </div>
      {shown.length === 0 && <p className="muted pad">Select text and press Ctrl+Alt+M to comment on it.</p>}
      {shown.map((c) => (
        <CommentCard
          key={c.id}
          comment={c}
          focused={focus?.id === c.id}
          inputRef={(el) => {
            if (el) refs.current.set(c.id, el);
            else refs.current.delete(c.id);
          }}
          onJump={() => {
            const h = editorHost.forDoc(docId)[0];
            if (!h?.revealComment(c.id)) workbench.notify({ message: "The text this comment was attached to is gone." });
          }}
          onSave={(body) => void put({ ...c, body })}
          onResolve={() => void put({ ...c, resolved: !c.resolved })}
          onDelete={() => void remove(c)}
        />
      ))}
    </div>
  );
}

function CommentCard({
  comment,
  focused,
  inputRef,
  onJump,
  onSave,
  onResolve,
  onDelete,
}: {
  comment: Comment;
  focused: boolean;
  inputRef: (el: HTMLTextAreaElement | null) => void;
  onJump: () => void;
  onSave: (body: string) => void;
  onResolve: () => void;
  onDelete: () => void;
}) {
  const [body, setBody] = useState(comment.body);
  const [seenBody, setSeenBody] = useState(comment.body);
  if (seenBody !== comment.body) {
    setSeenBody(comment.body);
    setBody(comment.body);
  }
  return (
    <div className={`comment-card${focused ? " focused" : ""}${comment.resolved ? " resolved" : ""}`}>
      <textarea
        ref={inputRef}
        rows={2}
        value={body}
        placeholder="Write a comment"
        aria-label="Comment"
        onChange={(e) => setBody(e.target.value)}
        onBlur={() => body !== comment.body && onSave(body)}
      />
      <div className="comment-actions">
        <button className="link" onClick={onJump}>
          Show text
        </button>
        <button className="link" onClick={onResolve}>
          {comment.resolved ? "Reopen" : "Resolve"}
        </button>
        <button className="link danger" onClick={onDelete}>
          Delete
        </button>
        <span className="muted small">{comment.createdAt ? new Date(comment.createdAt).toLocaleDateString("en-US") : ""}</span>
      </div>
    </div>
  );
}
