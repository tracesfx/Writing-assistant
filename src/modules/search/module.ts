import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { SearchPanel } from "./SearchPanel";
import { useSearch } from "./state";

function openSearch(scope: "document" | "manuscript", replace: boolean): void {
  const doc = workbench.activeDocumentId();
  useSearch.setState({
    scope: scope === "document" && doc ? "document" : "manuscript",
    scopeId: doc ?? null,
    showReplace: replace || useSearch.getState().showReplace,
    focusAt: Date.now(),
  });
  workbench.showPanel("search.panel");
}

export default defineModule({
  id: "search",
  name: "Find and replace",
  description: "Project-wide find and replace with regex, case and whole-word options.",
  sdkVersion,
  contributes: {
    sidebarPanels: [{ id: "search.panel", title: "Search", side: "left", order: 10, component: SearchPanel }],
    commands: [
      { id: "search.inDocument", title: "Find in document", category: "Find", keybinding: "Ctrl+F", run: () => openSearch("document", false) },
      { id: "search.replaceInDocument", title: "Replace in document", category: "Find", keybinding: "Ctrl+H", run: () => openSearch("document", true) },
      { id: "search.inProject", title: "Find in project", category: "Find", keybinding: "Ctrl+Shift+F", run: () => openSearch("manuscript", false) },
      {
        id: "search.replaceInProject",
        title: "Replace in project",
        category: "Find",
        keybinding: "Ctrl+Shift+H",
        run: () => openSearch("manuscript", true),
      },
    ],
  },
});
