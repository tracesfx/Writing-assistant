import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { HelloView } from "./HelloView";

export const HELLO_VIEW = "insights.hello";

/**
 * A complete Insights view. To use it, copy this folder to
 * `src/modules/hello-view/`. Nothing else changes: the module list picks the
 * folder up by itself, and the Insights workspace lists the view.
 */
export default defineModule({
  id: "hello-view",
  name: "Insights: Hello view",
  description: "A sample view that shows the manuscript's word count.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: HELLO_VIEW, title: "Hello view", description: "A sample", order: 100, component: HelloView }],
    commands: [{ id: "insights.hello.open", title: "Insights: Hello view", category: "View", run: () => openInsight(HELLO_VIEW) }],
  },
});
