import { INSIGHTS_VIEW, defineModule, sdkVersion, workbench } from "@margin/sdk";

import { InsightsHome } from "./InsightsHome";

export default defineModule({
  id: "insights",
  name: "Insights",
  description: "A workspace of views of your book: progress, who appears where, relationships, time, structure and plot threads. Each view is its own module.",
  sdkVersion,
  contributes: {
    workspaceViews: [{ id: INSIGHTS_VIEW, title: "Insights", component: InsightsHome }],
    commands: [
      {
        id: "insights.open",
        title: "Open Insights",
        category: "View",
        keybinding: "Ctrl+Alt+V",
        run: () => workbench.openView(INSIGHTS_VIEW),
      },
    ],
  },
});
