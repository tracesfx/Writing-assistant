/**
 * One model per open document (SPEC §11.2 split view). Every editor of a
 * document joins its hub: edits in one editor are replayed as steps in the
 * others (not added to their undo history, exactly like collaborative
 * editing), and there is a single autosave controller and recovery journal
 * per document.
 */
import type { Editor } from "@tiptap/core";
import type { Node as PmDoc } from "@tiptap/pm/model";
import type { Transaction } from "@tiptap/pm/state";

import { StoryError, story, type PmNode } from "@margin/sdk";

import { AutosaveController, type SaveState } from "../../editor/autosave";

export const REMOTE = "marginRemote";

export interface DocHub {
  docId: string;
  /** Latest document (updated per transaction, O(1)). */
  doc: PmDoc | null;
  autosave: AutosaveController;
  editors: Set<Editor>;
  save: SaveState;
  /** Changed on disk while there were unsaved edits. */
  externalChange: boolean;
  listeners: Set<() => void>;
}

const hubs = new Map<string, DocHub>();

export function getHub(docId: string): DocHub | undefined {
  return hubs.get(docId);
}

function notify(h: DocHub): void {
  for (const l of h.listeners) l();
}

export function joinHub(docId: string, editor: Editor, fallback: PmNode): DocHub {
  let h = hubs.get(docId);
  if (!h) {
    const hub: DocHub = {
      docId,
      doc: editor.state.doc,
      editors: new Set(),
      save: { status: "saved" },
      externalChange: false,
      listeners: new Set(),
      autosave: undefined as unknown as AutosaveController,
    };
    hub.autosave = new AutosaveController({
      getContent: () => (hub.doc ? (hub.doc.toJSON() as PmNode) : fallback),
      save: (content, force) => story.documents.save(docId, content, force),
      journal: (content) => story.recovery.write(docId, content),
      onState: (s) => {
        hub.save = s;
        if (s.status === "saved") hub.externalChange = false;
        notify(hub);
      },
      isConflict: (err) => err instanceof StoryError && err.kind === "conflict",
    });
    hubs.set(docId, hub);
    h = hub;
  }
  h.editors.add(editor);
  return h;
}

/** Leave the hub; the last editor out saves and disposes it. */
export function leaveHub(h: DocHub, editor: Editor): void {
  h.editors.delete(editor);
  if (h.editors.size > 0) return;
  hubs.delete(h.docId);
  void h.autosave.flush().finally(() => h.autosave.dispose());
}

/** Call from an editor's onUpdate: record the doc and mirror local edits. */
export function onLocalTransaction(h: DocHub, from: Editor, tr: Transaction): void {
  h.doc = from.state.doc;
  if (!tr.docChanged || tr.getMeta(REMOTE)) return;
  h.autosave.markDirty();
  for (const other of h.editors) {
    if (other === from || other.isDestroyed) continue;
    const mirror = other.state.tr;
    let ok = true;
    for (const step of tr.steps) {
      if (mirror.maybeStep(step).failed) {
        ok = false;
        break;
      }
    }
    if (!ok) {
      // Out of sync (should not happen): take the full document.
      other.commands.setContent(from.getJSON(), { emitUpdate: false });
      continue;
    }
    mirror.setMeta(REMOTE, true).setMeta("addToHistory", false);
    other.view.dispatch(mirror);
  }
}

/** Replace the content of every editor of the document (reload from disk). */
export function replaceContent(h: DocHub, content: PmNode): void {
  for (const ed of h.editors) {
    if (ed.isDestroyed) continue;
    const tr = ed.state.tr;
    const node = ed.schema.nodeFromJSON(content);
    tr.replaceWith(0, ed.state.doc.content.size, node.content);
    tr.setMeta(REMOTE, true).setMeta("addToHistory", false);
    ed.view.dispatch(tr);
    h.doc = ed.state.doc;
  }
  h.externalChange = false;
  h.autosave.markClean();
  notify(h);
}

export function subscribeHub(h: DocHub, l: () => void): () => void {
  h.listeners.add(l);
  return () => h.listeners.delete(l);
}
