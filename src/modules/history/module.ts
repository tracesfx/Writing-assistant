import { defineModule, editorHost, events, sdkVersion, story, workbench } from "@margin/sdk";

import { HISTORY_VIEW, errorMessage, takeSnapshot } from "./actions";
import { BackupsPage } from "./BackupsPage";
import { HistoryView } from "./HistoryView";

export default defineModule({
  id: "history",
  name: "History and backups",
  description: "Snapshots of every document, diffs, restore, and scheduled backups.",
  sdkVersion,
  activate() {
    // Backup failures are reported once, quietly.
    let lastError = "";
    return events.on("backup.finished", (e) => {
      if (e.error && e.error !== lastError) {
        lastError = e.error;
        workbench.notify({ message: `Backup failed: ${e.error}`, timeoutMs: 0 });
      } else if (!e.error) lastError = "";
    });
  },
  contributes: {
    workspaceViews: [{ id: HISTORY_VIEW, title: "History", component: HistoryView }],
    settingsPages: [{ id: "backups", title: "Backups", component: BackupsPage }],
    commands: [
      {
        id: "history.document",
        title: "History of this document",
        category: "History",
        keybinding: "Ctrl+Alt+H",
        when: () => workbench.activeDocumentId() !== undefined,
        run: () => {
          const id = workbench.activeDocumentId();
          if (id) workbench.openView(HISTORY_VIEW, id);
        },
      },
      {
        id: "history.snapshot",
        title: "Take a named snapshot…",
        category: "History",
        keybinding: "Ctrl+Alt+S",
        run: takeSnapshot,
      },
      {
        id: "history.backupNow",
        title: "Back up now",
        category: "History",
        run: async () => {
          try {
            await editorHost.flushAll();
            const b = await story.backups.now();
            workbench.notify({ message: `Backed up to ${b.name}` });
          } catch (e) {
            workbench.notify({ message: `Backup failed: ${errorMessage(e)}` });
          }
        },
      },
    ],
  },
});
