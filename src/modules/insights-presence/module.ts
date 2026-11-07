import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { PresenceView } from "./PresenceView";

export const PRESENCE_VIEW = "insights.presence";

export default defineModule({
  id: "insights-presence",
  name: "Insights: Character presence",
  description: "A heatmap of who and what appears in each scene or chapter.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: PRESENCE_VIEW, title: "Character presence", description: "Entries by scene or chapter", order: 1, component: PresenceView }],
    commands: [{ id: "insights.presence.open", title: "Insights: Character presence", category: "View", run: () => openInsight(PRESENCE_VIEW) }],
  },
});
