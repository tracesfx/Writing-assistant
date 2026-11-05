/**
 * App settings (SPEC §15): appearance, editor behaviour, keymap overrides
 * and disabled modules. Stored per machine through the backend app state;
 * modules read them with `useSettings` and change them with `settings.update`.
 */
import { create } from "zustand";

import { story } from "./story";

export type Theme = "system" | "light" | "sepia" | "dark" | "contrast";
export type EditorFont = "literata" | "georgia" | "palatino" | "system-serif" | "sans";
export type FocusMode = "off" | "sentence" | "paragraph";

export interface AppSettings {
  v: 1;
  theme: Theme;
  editorFont: EditorFont;
  /** Pixels. */
  editorSize: number;
  lineHeight: number;
  /** Text column width in rem. */
  editorWidth: number;
  /** UI scale factor (accessibility). */
  uiScale: number;
  smartQuotes: boolean;
  smartDashes: boolean;
  smartEllipsis: boolean;
  focusMode: FocusMode;
  typewriter: boolean;
  spellcheck: boolean;
  /** Command id → chord, or `null` to unbind. Absent = default binding. */
  keymap: Record<string, string | null>;
  disabledModules: string[];
}

export const DEFAULT_SETTINGS: AppSettings = {
  v: 1,
  theme: "system",
  editorFont: "literata",
  editorSize: 19,
  lineHeight: 1.65,
  editorWidth: 40,
  uiScale: 1,
  smartQuotes: true,
  smartDashes: true,
  smartEllipsis: true,
  focusMode: "off",
  typewriter: false,
  spellcheck: true,
  keymap: {},
  disabledModules: [],
};

interface SettingsState {
  settings: AppSettings;
  loaded: boolean;
}

export const useSettingsStore = create<SettingsState>(() => ({ settings: DEFAULT_SETTINGS, loaded: false }));

let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** Parse a stored settings blob, filling gaps with defaults. */
export function parseSettings(raw: string | null | undefined): AppSettings {
  if (!raw) return DEFAULT_SETTINGS;
  try {
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
    return { ...DEFAULT_SETTINGS, ...parsed, v: 1, keymap: { ...(parsed.keymap ?? {}) } };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export const settings = {
  get: (): AppSettings => useSettingsStore.getState().settings,
  /** Called once by the shell with the stored blob. */
  load(raw: string | null | undefined): void {
    useSettingsStore.setState({ settings: parseSettings(raw), loaded: true });
  },
  update(patch: Partial<AppSettings>): void {
    const next = { ...useSettingsStore.getState().settings, ...patch };
    useSettingsStore.setState({ settings: next });
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      story.app.saveSettings(JSON.stringify(next)).catch((e: unknown) => console.warn("[margin] could not save settings", e));
    }, 400);
  },
};

export function useSettings<T>(select: (s: AppSettings) => T): T {
  return useSettingsStore((st) => select(st.settings));
}
