import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { GoalsPage, StatsStatusItem, StatsView } from "./StatsView";
import { startStats } from "./store";

export const STATS_VIEW = "stats.overview";

export default defineModule({
  id: "stats",
  name: "Goals and stats",
  description: "Words written per day and session, streaks, writing time, goals and deadlines.",
  sdkVersion,
  activate: startStats,
  contributes: {
    workspaceViews: [{ id: STATS_VIEW, title: "Writing stats", component: StatsView }],
    statusBarItems: [{ id: "stats.today", align: "right", order: -10, component: StatsStatusItem }],
    settingsPages: [{ id: "goals", title: "Goals", component: GoalsPage }],
    commands: [
      { id: "stats.open", title: "Writing stats and goals", category: "View", keybinding: "Ctrl+Alt+G", run: () => workbench.openView(STATS_VIEW) },
    ],
  },
});
