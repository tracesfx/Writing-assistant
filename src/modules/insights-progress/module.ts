import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { ProgressView } from "./ProgressView";

export const PROGRESS_VIEW = "insights.progress";

export default defineModule({
  id: "insights-progress",
  name: "Insights: Writing progress",
  description: "Words per day, streak calendar, growth against your goal, your words and AI words.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: PROGRESS_VIEW, title: "Writing progress", description: "Words per day, streaks, goal", order: 0, component: ProgressView }],
    commands: [{ id: "insights.progress.open", title: "Insights: Writing progress", category: "View", run: () => openInsight(PROGRESS_VIEW) }],
  },
});
