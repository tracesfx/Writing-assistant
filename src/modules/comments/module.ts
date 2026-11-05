import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { CommentsPanel } from "./CommentsPanel";

export default defineModule({
  id: "comments",
  name: "Comments",
  description: "Inline comments anchored to text. Kept in the project, never exported.",
  sdkVersion,
  contributes: {
    sidebarPanels: [{ id: "comments.panel", title: "Comments", side: "right", order: 10, component: CommentsPanel }],
    commands: [
      {
        id: "comments.show",
        title: "Show comments",
        category: "View",
        keybinding: "Ctrl+Alt+C",
        run: () => workbench.togglePanel("comments.panel"),
      },
    ],
  },
});
