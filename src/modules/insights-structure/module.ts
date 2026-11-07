import { defineModule, openInsight, sdkVersion } from "@margin/sdk";

import { StructureView } from "./StructureView";

export const STRUCTURE_VIEW = "insights.structure";

export default defineModule({
  id: "insights-structure",
  name: "Insights: Structure overview",
  description: "Words per chapter by point of view, with an optional line for tension or dialogue.",
  sdkVersion,
  contributes: {
    insightsViews: [{ id: STRUCTURE_VIEW, title: "Structure overview", description: "Chapters, point of view, tension", order: 4, component: StructureView }],
    // The model rates each scene's tension (0-10) while it reads it (Rust: `memory::extract::TENSION`).
    memoryExtractors: [{ id: "tension", metricKey: "tension", title: "Tension (0–10)", max: 10, settingKey: "extractTension" }],
    commands: [{ id: "insights.structure.open", title: "Insights: Structure overview", category: "View", run: () => openInsight(STRUCTURE_VIEW) }],
  },
});
