import {
  BIBLE_ENTRY_VIEW,
  BIBLE_PANEL,
  MEMORY_INSPECTOR_VIEW,
  defineModule,
  editorHost,
  memory,
  sdkVersion,
  workbench,
  useWorkbench,
} from "@margin/sdk";

import { fail } from "./common";
import { BiblePanel } from "./BiblePanel";
import { EntryView } from "./EntryView";
import { InspectorView } from "./InspectorView";
import { MemoryPage } from "./MemoryPage";
import { MemoryStatusItem } from "./MemoryStatusItem";

/** The scene the author is in: memory reads it first. */
function currentDoc(): string | undefined {
  return editorHost.active()?.docId ?? workbench.activeDocumentId();
}

export default defineModule({
  id: "bible",
  name: "Story Bible and memory",
  description: "The Story Bible, the Memory Inspector, and the background reading of your scenes (canon versus inferred).",
  sdkVersion,
  activate: () => {
    // Tell memory which scene is in front, so it reads the nearest ones first.
    let last: string | undefined;
    return useWorkbench.subscribe(() => {
      const id = workbench.activeDocumentId();
      if (id && id !== last && workbench.activeTab()?.viewId !== BIBLE_ENTRY_VIEW) {
        last = id;
        void memory.focus(id).catch(() => undefined);
      }
    });
  },
  contributes: {
    sidebarPanels: [{ id: BIBLE_PANEL, title: "Bible", side: "left", order: 10, component: BiblePanel }],
    workspaceViews: [
      { id: BIBLE_ENTRY_VIEW, title: "Bible entry", component: EntryView },
      { id: MEMORY_INSPECTOR_VIEW, title: "Memory Inspector", component: InspectorView },
    ],
    statusBarItems: [{ id: "memory.status", align: "right", order: 1, component: MemoryStatusItem }],
    settingsPages: [{ id: "memory", title: "Memory", component: MemoryPage }],
    commands: [
      { id: "bible.show", title: "Show the Story Bible", category: "View", keybinding: "Ctrl+Alt+B", run: () => workbench.togglePanel(BIBLE_PANEL) },
      { id: "memory.inspector", title: "Memory Inspector", category: "Memory", run: () => workbench.openView(MEMORY_INSPECTOR_VIEW) },
      {
        id: "bible.new",
        title: "New bible entry",
        category: "Memory",
        run: async () => {
          const name = await workbench.ask({ title: "Name of the new character (use the Bible panel for other types)", placeholder: "Name" });
          if (!name?.trim()) return;
          try {
            const e = await memory.entities.create("character", name.trim());
            workbench.showPanel(BIBLE_PANEL);
            workbench.openView(BIBLE_ENTRY_VIEW, e.id);
          } catch (err) {
            fail("Could not add the entry", err);
          }
        },
      },
      {
        id: "memory.reindexScene",
        title: "Re-read this scene (Re-index scene)",
        category: "Memory",
        run: async () => {
          const doc = currentDoc();
          if (!doc) return;
          workbench.notify({ message: "Reading this scene…", key: "memory.reindex" });
          try {
            await memory.reindexScene(doc);
            workbench.notify({ message: "Scene read. The bible is up to date.", key: "memory.reindex" });
          } catch (err) {
            fail("Could not read the scene", err);
          }
        },
      },
      {
        id: "memory.rebuildAll",
        title: "Rebuild all memory",
        category: "Memory",
        run: async () => {
          try {
            const n = await memory.rebuildAll();
            workbench.notify({ message: `Memory will be rebuilt: ${n} scene${n === 1 ? "" : "s"} to read again. Canon is untouched.` });
          } catch (err) {
            fail("Could not rebuild", err);
          }
        },
      },
      {
        id: "memory.forgetDocument",
        title: "Forget this document (keep it out of memory)",
        category: "Memory",
        run: async () => {
          const doc = currentDoc();
          if (!doc) return;
          try {
            await memory.forgetDocument(doc, true);
            workbench.notify({
              message: "This document is now out of memory: nothing read from it is kept.",
              action: { label: "Undo", run: () => void memory.forgetDocument(doc, false) },
              timeoutMs: 10000,
            });
          } catch (err) {
            fail("Could not forget the document", err);
          }
        },
      },
      {
        id: "memory.includeDocument",
        title: "Include this document in memory again",
        category: "Memory",
        run: async () => {
          const doc = currentDoc();
          if (!doc) return;
          await memory.forgetDocument(doc, false).catch((err: unknown) => fail("Could not include the document", err));
        },
      },
    ],
  },
});

