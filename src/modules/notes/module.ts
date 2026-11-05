import { defineModule, sdkVersion, workbench } from "@margin/sdk";

import { CaptureBox, openCapture } from "./Capture";
import { ScratchpadPanel } from "./ScratchpadPanel";

export default defineModule({
  id: "notes",
  name: "Quick capture and scratchpad",
  description: "Send a thought to the Inbox without leaving the scene; a per-project scratchpad.",
  sdkVersion,
  contributes: {
    sidebarPanels: [{ id: "notes.scratchpad", title: "Scratchpad", side: "right", order: 20, component: ScratchpadPanel }],
    statusBarItems: [{ id: "notes.capture", align: "left", order: 100, component: CaptureBox }],
    commands: [
      { id: "notes.capture", title: "Quick capture to Inbox", category: "Notes", keybinding: "Ctrl+Shift+N", run: openCapture },
      {
        id: "notes.scratchpad",
        title: "Show scratchpad",
        category: "View",
        keybinding: "Ctrl+Alt+P",
        run: () => workbench.togglePanel("notes.scratchpad"),
      },
    ],
  },
});
