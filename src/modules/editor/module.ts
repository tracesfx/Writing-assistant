import {
  defineModule,
  editorHost,
  lifecycle,
  newId,
  sdkVersion,
  settings,
  story,
  uiState,
  workbench,
  type FocusMode,
  type PmNode,
} from "@margin/sdk";

import { nextBookmark, toggleBookmark } from "../../editor/bookmarks";
import type { DivStyle } from "../../editor/schema";
import { DocumentView } from "./DocumentView";
import { lastEdited, navigate, wireNavigation } from "./positions";
import { editors } from "./state";
import { SaveStateItem, WhereWasIItem, WordCountItem } from "./StatusItems";

function activeHandle() {
  const tab = workbench.activeTab();
  return tab ? editors.get(tab.id) : undefined;
}

const hasEditor = () => activeHandle() !== undefined;
const errorMessage = (e: unknown) => (e instanceof Error ? e.message : String(e));

const styleCommand = (style: DivStyle, title: string) => ({
  id: `editor.style.${style}`,
  title: `Style: ${title}`,
  category: "Format",
  when: hasEditor,
  run: () => activeHandle()?.editor.chain().focus().toggleStyledBlock(style).run(),
});

/** Split the active document at the cursor (SPEC §11.1). */
async function splitAtCursor(): Promise<void> {
  const h = activeHandle();
  if (!h) return;
  const { state } = h.editor;
  const pos = state.selection.from;
  if (pos <= 1 || pos >= state.doc.content.size - 1) {
    workbench.notify({ message: "Put the cursor inside the text to split there." });
    return;
  }
  const before = state.doc.cut(0, pos).toJSON() as PmNode;
  const after = state.doc.cut(pos).toJSON() as PmNode;
  try {
    await editorHost.flushAll();
    const meta = await story.documents.split(h.docId, before, after);
    const original = h.docId;
    workbench.openDocument(meta.id);
    workbench.notify({
      message: `Split into “${meta.title}”`,
      action: {
        label: "Undo",
        run: () => {
          void (async () => {
            await editorHost.flushAll();
            await story.documents.merge([original, meta.id]);
            workbench.openDocument(original);
          })().catch((e: unknown) => workbench.notify({ message: `Could not undo the split: ${errorMessage(e)}` }));
        },
      },
      timeoutMs: 10_000,
    });
  } catch (e) {
    workbench.notify({ message: `Could not split: ${errorMessage(e)}` });
  }
}

function insertFootnote(): void {
  const h = activeHandle();
  if (!h) return;
  const ok = h.editor.chain().focus().insertContent({ type: "footnote", attrs: { text: "" } }).run();
  if (!ok) return;
  const pos = h.editor.state.selection.from - 1;
  window.dispatchEvent(new CustomEvent("margin:footnote-new", { detail: { tabId: workbench.activeTab()?.id, pos } }));
}

async function addComment(): Promise<void> {
  const h = activeHandle();
  if (!h) return;
  if (h.editor.state.selection.empty) {
    workbench.notify({ message: "Select the text to comment on first." });
    return;
  }
  const id = newId();
  h.editor.chain().focus().setMark("comment", { id }).run();
  try {
    await story.comments.put(h.docId, { id, body: "", resolved: false, createdAt: "", updatedAt: "" });
    uiState.set("comments.focus", { docId: h.docId, id, at: Date.now() });
    workbench.showPanel("comments.panel");
  } catch (e) {
    workbench.notify({ message: `Could not add the comment: ${errorMessage(e)}` });
  }
}

const FOCUS_ORDER: FocusMode[] = ["off", "sentence", "paragraph"];

function jumpTo(docId: string, pos: number): void {
  editorHost.openAt(docId, { pos });
}

export default defineModule({
  id: "editor",
  name: "Editor",
  description: "The manuscript editor with semantic styles.",
  sdkVersion,
  required: true,
  activate() {
    const unwire = wireNavigation((docId) => editorHost.forDoc(docId)[0]?.cursor());
    const off = lifecycle.onBeforeClose(() => editorHost.flushAll());
    return () => {
      unwire();
      off();
    };
  },
  contributes: {
    workspaceViews: [{ id: "editor.document", title: "Document", opens: "document", component: DocumentView }],
    statusBarItems: [
      { id: "editor.save", align: "left", order: 0, component: SaveStateItem },
      { id: "editor.words", align: "left", order: 1, component: WordCountItem },
      { id: "editor.whereWasI", align: "left", order: 5, component: WhereWasIItem },
    ],
    commands: [
      {
        id: "editor.save",
        title: "Save now",
        category: "File",
        keybinding: "Ctrl+S",
        when: hasEditor,
        run: () => activeHandle()?.hub.autosave.flush(),
      },
      {
        id: "editor.split",
        title: "Split document at cursor",
        category: "Binder",
        keybinding: "Ctrl+Shift+K",
        when: hasEditor,
        run: splitAtCursor,
      },
      {
        id: "editor.sceneBreak",
        title: "Insert scene break",
        category: "Insert",
        keybinding: "Ctrl+Shift+Enter",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().insertSceneBreak().run(),
      },
      {
        id: "editor.footnote",
        title: "Insert footnote",
        category: "Insert",
        keybinding: "Ctrl+Alt+D",
        when: hasEditor,
        run: insertFootnote,
      },
      {
        id: "editor.comment",
        title: "Comment on selection",
        category: "Insert",
        keybinding: "Ctrl+Alt+M",
        when: hasEditor,
        run: addComment,
      },
      {
        id: "editor.bookmark",
        title: "Toggle bookmark",
        category: "Navigate",
        keybinding: "Ctrl+F2",
        when: hasEditor,
        run: () => {
          const h = activeHandle();
          if (h) toggleBookmark(h.editor.view);
        },
      },
      {
        id: "editor.nextBookmark",
        title: "Next bookmark",
        category: "Navigate",
        keybinding: "F2",
        when: hasEditor,
        run: () => {
          const h = activeHandle();
          if (h && !nextBookmark(h.editor.view, 1)) workbench.notify({ message: "No bookmarks here. Ctrl+F2 adds one." });
        },
      },
      {
        id: "editor.prevBookmark",
        title: "Previous bookmark",
        category: "Navigate",
        keybinding: "Shift+F2",
        when: hasEditor,
        run: () => {
          const h = activeHandle();
          if (h) nextBookmark(h.editor.view, -1);
        },
      },
      {
        id: "editor.back",
        title: "Go back",
        category: "Navigate",
        keybinding: "Alt+Left",
        run: () => {
          const h = activeHandle();
          navigate("back", h ? { docId: h.docId, pos: h.editor.state.selection.head } : undefined, (e) => jumpTo(e.docId, e.pos));
        },
      },
      {
        id: "editor.forward",
        title: "Go forward",
        category: "Navigate",
        keybinding: "Alt+Right",
        run: () => {
          const h = activeHandle();
          navigate("forward", h ? { docId: h.docId, pos: h.editor.state.selection.head } : undefined, (e) =>
            jumpTo(e.docId, e.pos),
          );
        },
      },
      {
        id: "editor.jumpBack",
        title: "Jump back to where I was",
        category: "Navigate",
        keybinding: "Ctrl+Alt+Left",
        run: () => {
          const last = lastEdited();
          if (last) jumpTo(last.docId, last.pos);
        },
      },
      {
        id: "editor.focusMode",
        title: "Focus mode: cycle off / sentence / paragraph",
        category: "View",
        keybinding: "F9",
        run: () => {
          const cur = settings.get().focusMode;
          const next = FOCUS_ORDER[(FOCUS_ORDER.indexOf(cur) + 1) % FOCUS_ORDER.length]!;
          settings.update({ focusMode: next });
          workbench.notify({ message: next === "off" ? "Focus mode off" : `Focus mode: ${next}`, timeoutMs: 2000, key: "editor.focusMode" });
        },
      },
      {
        id: "editor.typewriter",
        title: "Toggle typewriter scrolling",
        category: "View",
        keybinding: "Shift+F9",
        run: () => {
          const on = !settings.get().typewriter;
          settings.update({ typewriter: on });
          workbench.notify({ message: on ? "Typewriter scrolling on" : "Typewriter scrolling off", timeoutMs: 2000, key: "editor.typewriter" });
        },
      },
      {
        id: "editor.spellcheck",
        title: "Toggle spellcheck",
        category: "View",
        run: () => settings.update({ spellcheck: !settings.get().spellcheck }),
      },
      {
        id: "editor.body",
        title: "Style: Body text",
        category: "Format",
        keybinding: "Ctrl+Alt+0",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().setParagraph().run(),
      },
      ...([2, 3, 4] as const).map((level) => ({
        id: `editor.heading${level}`,
        title: `Style: Heading ${level}`,
        category: "Format",
        keybinding: `Ctrl+Alt+${level}`,
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().toggleHeading({ level }).run(),
      })),
      {
        id: "editor.blockquote",
        title: "Style: Block quote",
        category: "Format",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().toggleBlockquote().run(),
      },
      styleCommand("epigraph", "Epigraph"),
      styleCommand("attribution", "Epigraph attribution"),
      styleCommand("letter", "Letter / document"),
      styleCommand("textmsg", "Text message"),
      styleCommand("verse", "Verse"),
      styleCommand("center", "Centered"),
      {
        id: "editor.smallCaps",
        title: "Small caps",
        category: "Format",
        keybinding: "Ctrl+Shift+C",
        when: hasEditor,
        allowDefault: true,
        run: () => undefined, // handled by the editor keymap; listed for the cheat sheet
      },
      {
        id: "editor.noBreak",
        title: "No break",
        category: "Format",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().toggleNoBreak().run(),
      },
      {
        id: "editor.bulletList",
        title: "Bulleted list",
        category: "Format",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().toggleBulletList().run(),
      },
      {
        id: "editor.orderedList",
        title: "Numbered list",
        category: "Format",
        when: hasEditor,
        run: () => activeHandle()?.editor.chain().focus().toggleOrderedList().run(),
      },
    ],
  },
});
