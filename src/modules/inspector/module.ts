import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { PropertiesPanel } from "./PropertiesPanel";

export default defineModule({
  id: "inspector",
  name: "Inspector",
  description: "Document properties: status, labels, POV, target, synopsis, export and AI switches.",
  sdkVersion,
  contributes: {
    sidebarPanels: [{ id: "inspector.properties", title: "Properties", side: "right", order: 0, component: PropertiesPanel }],
    commands: [
      {
        id: "inspector.show",
        title: "Show document properties",
        category: "View",
        keybinding: "Ctrl+Alt+I",
        run: () => workbench.togglePanel("inspector.properties"),
      },
    ],
  },
});
