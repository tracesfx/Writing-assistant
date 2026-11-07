import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { TimelineView } from "./TimelineView";

export const TIMELINE_VIEW = "insights.timeline";

export default defineModule({
  id: "insights-timeline",
  name: "Insights: Timeline",
  description: "Events in the order the book tells them and the order they happen, with point-of-view bands.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: TIMELINE_VIEW, title: "Timeline", description: "Told order and story order", order: 3, component: TimelineView }],
    commands: [{ id: "insights.timeline.open", title: "Insights: Timeline", category: "View", run: () => openInsight(TIMELINE_VIEW) }],
  },
});
