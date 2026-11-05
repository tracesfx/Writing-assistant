/**
 * The shell's own contributions, declared like any module: palette, quick
 * open, cheat sheet, Settings, sidebars, tabs and distraction-free mode.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";

import { defineModule, sdkVersion, useWorkbench } from "@margin/sdk";

import { SettingsView } from "./SettingsView";
import { SETTINGS_VIEW, openSettings, shellUi, useShellUi } from "./shellUi";

function cycleTab(dir: 1 | -1): void {
  const s = useWorkbench.getState();
  const pane = s.panes.find((p) => p.id === s.focusedPane);
  if (!pane || pane.tabs.length < 2) return;
  const i = pane.tabs.findIndex((t) => t.id === pane.activeTabId);
  const next = pane.tabs[(i + dir + pane.tabs.length) % pane.tabs.length];
  if (next) s.activateTab(next.id);
}

async function toggleDistractionFree(): Promise<void> {
  const on = !useShellUi.getState().distractionFree;
  useShellUi.setState({ distractionFree: on });
  try {
    await getCurrentWindow().setFullscreen(on);
  } catch (e) {
    console.warn("[margin] full screen unavailable", e);
  }
}

export default defineModule({
  id: "shell",
  name: "Workspace",
  description: "Command palette, quick open, settings and layout.",
  sdkVersion,
  required: true,
  contributes: {
    workspaceViews: [{ id: SETTINGS_VIEW, title: "Settings", component: SettingsView }],
    commands: [
      { id: "shell.palette", title: "Command palette", category: "View", keybinding: "Ctrl+K", run: () => shellUi.open("palette") },
      { id: "shell.quickOpen", title: "Quick open", category: "Navigate", keybinding: "Ctrl+P", run: () => shellUi.open("quickOpen") },
      {
        id: "shell.cheatSheet",
        title: "Keyboard shortcuts",
        category: "Help",
        keybinding: "Ctrl+/",
        run: () => shellUi.open("cheatSheet"),
      },
      { id: "shell.settings", title: "Settings", category: "File", keybinding: "Ctrl+,", run: () => openSettings() },
      {
        id: "shell.shortcutsEditor",
        title: "Change keyboard shortcuts",
        category: "File",
        run: () => openSettings("shortcuts"),
      },
      { id: "shell.projectSettings", title: "Project settings", category: "File", run: () => openSettings("project") },
      {
        id: "shell.switchProject",
        title: "Open another project…",
        category: "File",
        run: () => window.dispatchEvent(new Event("margin:switch-project")),
      },
      {
        id: "view.toggleSidebar",
        title: "Toggle sidebar",
        category: "View",
        keybinding: "Ctrl+\\",
        run: () => {
          const sb = useWorkbench.getState().sidebars.left;
          useWorkbench.getState().setSidebar("left", { visible: !sb.visible });
        },
      },
      {
        id: "view.toggleRightSidebar",
        title: "Toggle right sidebar",
        category: "View",
        keybinding: "Ctrl+Shift+\\",
        run: () => {
          const sb = useWorkbench.getState().sidebars.right;
          useWorkbench.getState().setSidebar("right", { visible: !sb.visible });
        },
      },
      {
        id: "view.toggleSplit",
        title: "Toggle split view",
        category: "View",
        keybinding: "Ctrl+Alt+\\",
        run: () => useWorkbench.getState().toggleSplit(),
      },
      {
        id: "view.otherPane",
        title: "Focus the other pane",
        category: "View",
        keybinding: "Ctrl+Alt+Right",
        run: () => {
          const s = useWorkbench.getState();
          s.focusPane(s.focusedPane === "left" ? "right" : "left");
        },
      },
      {
        id: "view.closeTab",
        title: "Close tab",
        category: "View",
        keybinding: "Ctrl+W",
        run: () => {
          const s = useWorkbench.getState();
          const pane = s.panes.find((p) => p.id === s.focusedPane);
          if (pane?.activeTabId) s.closeTab(pane.activeTabId);
        },
      },
      { id: "view.nextTab", title: "Next tab", category: "View", keybinding: "Ctrl+Tab", run: () => cycleTab(1) },
      { id: "view.prevTab", title: "Previous tab", category: "View", keybinding: "Ctrl+Shift+Tab", run: () => cycleTab(-1) },
      {
        id: "view.distractionFree",
        title: "Distraction-free full screen",
        category: "View",
        keybinding: "F11",
        run: toggleDistractionFree,
      },
      {
        id: "view.exitDistractionFree",
        title: "Leave distraction-free mode",
        category: "View",
        keybinding: "Escape",
        allowDefault: true,
        when: () => useShellUi.getState().distractionFree && useShellUi.getState().overlay === null,
        run: toggleDistractionFree,
      },
    ],
  },
});
