/** Shell-only UI state: overlays and distraction-free mode. */
import { create } from "zustand";

import { workbench } from "@margin/sdk";

import type { Registry } from "./registry";

export type Overlay = "palette" | "quickOpen" | "cheatSheet" | null;

interface ShellUi {
  overlay: Overlay;
  distractionFree: boolean;
  registry: Registry | null;
  /** Settings page to show when the Settings view opens. */
  settingsPage: string;
}

export const useShellUi = create<ShellUi>(() => ({
  overlay: null,
  distractionFree: false,
  registry: null,
  settingsPage: "editor",
}));

export const shellUi = {
  open: (overlay: Overlay) => useShellUi.setState({ overlay }),
  close: () => useShellUi.setState({ overlay: null }),
};

export const SETTINGS_VIEW = "shell.settings";

export function openSettings(page?: string): void {
  if (page) useShellUi.setState({ settingsPage: page });
  workbench.openView(SETTINGS_VIEW);
}
