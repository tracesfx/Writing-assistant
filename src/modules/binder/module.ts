import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { OUTLINE_VIEW, createNode, duplicateNodes, mergeNodes, openOutline, trashNodes } from "./actions";
import { BinderPanel } from "./BinderPanel";
import { OutlineView } from "./OutlineView";
import { useBinderUi } from "./state";

const selection = () => useBinderUi.getState().selection;
/** Selection commands apply only while the binder is the active region. */
const binderSelection = (min: number) => () => useBinderUi.getState().active && selection().length >= min;

export default defineModule({
  id: "binder",
  name: "Binder",
  description: "The project tree: parts, chapters, scenes, notes, research and trash.",
  sdkVersion,
  required: true,
  contributes: {
    sidebarPanels: [{ id: "binder", title: "Binder", side: "left", order: 0, component: BinderPanel }],
    workspaceViews: [{ id: OUTLINE_VIEW, title: "Corkboard", component: OutlineView }],
    commands: [
      { id: "binder.newScene", title: "New scene", category: "Binder", keybinding: "Ctrl+N", run: () => createNode("scene") },
      {
        id: "binder.newChapter",
        title: "New chapter",
        category: "Binder",
        keybinding: "Ctrl+Alt+N",
        run: () => createNode("chapter"),
      },
      { id: "binder.newPart", title: "New part", category: "Binder", run: () => createNode("part") },
      { id: "binder.newNote", title: "New note", category: "Binder", run: () => createNode("note") },
      { id: "binder.newFolder", title: "New folder", category: "Binder", run: () => createNode("folder") },
      {
        id: "binder.show",
        title: "Show binder",
        category: "View",
        keybinding: "Ctrl+Alt+B",
        run: () => {
          workbench.showPanel("binder");
          document.getElementById("binder-tree")?.focus();
        },
      },
      {
        id: "binder.rename",
        title: "Rename selected item",
        category: "Binder",
        when: () => useBinderUi.getState().active && selection().length === 1,
        run: () => {
          const id = selection()[0];
          if (id) useBinderUi.getState().startRename(id);
        },
      },
      {
        id: "binder.duplicate",
        title: "Duplicate selected items",
        category: "Binder",
        keybinding: "Ctrl+D",
        when: binderSelection(1),
        run: () => duplicateNodes(selection()),
      },
      {
        id: "binder.merge",
        title: "Merge selected documents",
        category: "Binder",
        keybinding: "Ctrl+Shift+M",
        when: binderSelection(2),
        run: () => mergeNodes(selection()),
      },
      {
        id: "binder.trash",
        title: "Move selected items to Trash",
        category: "Binder",
        when: binderSelection(1),
        run: () => trashNodes(selection()),
      },
      {
        id: "binder.corkboard",
        title: "Open corkboard / outline",
        category: "View",
        keybinding: "Ctrl+Alt+O",
        run: () => openOutline(),
      },
    ],
  },
});
