import { ai, defineModule, sdkVersion, workbench } from "@margin/sdk";

import { AiStatusItem } from "./AiStatusItem";
import { BakeoffPage } from "./BakeoffPage";
import { MODEL_VIEW } from "./ids";
import { ModelView } from "./ModelView";
import { ModelsPage } from "./ModelsPage";
import { errMessage, refreshConfig, saveConfig, startAi, useAi } from "./store";

export default defineModule({
  id: "ai",
  name: "Local model",
  description: "Connects to the local model server, shows its health, keeps the context log, and runs the model bake-off.",
  sdkVersion,
  activate: startAi,
  contributes: {
    workspaceViews: [{ id: MODEL_VIEW, title: "Model", component: ModelView }],
    statusBarItems: [{ id: "ai.status", align: "right", order: 0, component: AiStatusItem }],
    settingsPages: [
      { id: "models", title: "Models", component: ModelsPage },
      { id: "bakeoff", title: "Bake-off", component: BakeoffPage },
    ],
    commands: [
      { id: "ai.openModel", title: "Model panel", category: "AI", run: () => workbench.openView(MODEL_VIEW) },
      {
        id: "ai.toggle",
        title: "Turn AI on or off",
        category: "AI",
        keybinding: "Ctrl+Shift+Space",
        run: async () => {
          const cfg = useAi.getState().config ?? (await refreshConfig());
          if (!cfg) return;
          await saveConfig({ enabled: !cfg.enabled });
          workbench.notify({ message: cfg.enabled ? "AI is off." : "AI is on.", key: "ai.toggle" });
        },
      },
      {
        id: "ai.start",
        title: "Start the model server",
        category: "AI",
        run: () => ai.connect().catch((e: unknown) => workbench.notify({ message: errMessage(e) })),
      },
      {
        id: "ai.stop",
        title: "Stop the model server",
        category: "AI",
        run: () => ai.disconnect().catch((e: unknown) => workbench.notify({ message: errMessage(e) })),
      },
    ],
  },
});
