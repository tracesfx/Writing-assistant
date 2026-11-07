import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { ThreadsView } from "./ThreadsView";

export const THREADS_VIEW = "insights.threads";

export default defineModule({
  id: "insights-threads",
  name: "Insights: Plot threads",
  description: "Swimlanes from where each plot thread is introduced, through the scenes that touch it, to where it is resolved.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: THREADS_VIEW, title: "Plot threads", description: "Introduced, touched, resolved", order: 5, component: ThreadsView }],
    commands: [{ id: "insights.threads.open", title: "Insights: Plot threads", category: "View", run: () => openInsight(THREADS_VIEW) }],
  },
});
