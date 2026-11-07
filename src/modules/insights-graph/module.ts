import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { GraphView } from "./GraphView";

export const GRAPH_VIEW = "insights.graph";

export default defineModule({
  id: "insights-graph",
  name: "Insights: Relationship graph",
  description: "A network of how your bible's people, places and groups relate, as of any chapter.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: GRAPH_VIEW, title: "Relationship graph", description: "Who relates to whom", order: 2, component: GraphView }],
    commands: [{ id: "insights.graph.open", title: "Insights: Relationship graph", category: "View", run: () => openInsight(GRAPH_VIEW) }],
  },
});
