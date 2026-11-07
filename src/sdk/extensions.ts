/**
 * Module manifest and extension points (SPEC §5.3).
 *
 * Every first-party feature is a module in `src/modules/<name>/module.ts`
 * that default-exports a manifest built with `defineModule`. Modules only
 * use `@margin/sdk`; they never import another module or the shell.
 */
import type { ComponentType } from "react";

/** Context passed to command handlers. */
export interface CommandContext {
  /** The element that had focus when the command ran, if any. */
  readonly target: EventTarget | null;
}

export interface CommandContribution {
  /** Globally unique, `module.action` style, e.g. `binder.newScene`. */
  id: string;
  title: string;
  category?: string;
  /** Default keybinding, e.g. `Ctrl+Shift+K`. Rebindable by the author. */
  keybinding?: string;
  /** Only runnable (and shown in the palette) when this returns true. */
  when?: () => boolean;
  /** Let the key event through to the focused element as well. */
  allowDefault?: boolean;
  /** The return value is ignored; a returned promise is awaited for error reporting. */
  run: (ctx: CommandContext) => unknown;
}

export interface SidebarPanelProps {
  /** True while the panel is visible. */
  visible: boolean;
}

export interface SidebarPanelContribution {
  id: string;
  title: string;
  side: "left" | "right";
  order?: number;
  component: ComponentType<SidebarPanelProps>;
}

export interface WorkspaceViewProps {
  /** Unique id of the tab hosting this view. */
  tabId: string;
  /** The resource the view shows, e.g. a document id. */
  resourceId?: string;
  /** True when this tab is the visible tab of its pane. */
  active: boolean;
  /** True when this tab's pane has focus. */
  focused: boolean;
}

export interface WorkspaceViewContribution {
  id: string;
  title: string;
  /** Resource kind this view opens (e.g. `document`); omit for standalone views. */
  opens?: "document";
  component: ComponentType<WorkspaceViewProps>;
}

export interface StatusBarItemContribution {
  id: string;
  align: "left" | "right";
  order?: number;
  component: ComponentType;
}

export interface SettingsPageContribution {
  id: string;
  title: string;
  component: ComponentType;
}

/* Extension points whose shapes become concrete in the milestone that adds
 * their first consumer. `marginTabs` has been concrete since M4. */
export interface MarginTabProps {
  /** True while this tab is the one showing. */
  active: boolean;
}

/** A tab in the Margin panel (SPEC §8.1): Ideas, Ask my book, Phrasing. */
export interface MarginTabContribution {
  id: string;
  title: string;
  order?: number;
  component: ComponentType<MarginTabProps>;
}
export interface InspectorSectionContribution {
  id: string;
  title: string;
  component: ComponentType<{ resourceId: string }>;
}
export interface AiTaskContribution {
  id: string;
}
export interface MemoryExtractorContribution {
  id: string;
  metricKey: string;
}
export interface ExporterContribution {
  id: string;
  title: string;
}
export interface BibleTypeContribution {
  id: string;
  title: string;
}

export interface Contributions {
  workspaceViews?: WorkspaceViewContribution[];
  sidebarPanels?: SidebarPanelContribution[];
  marginTabs?: MarginTabContribution[];
  inspectorSections?: InspectorSectionContribution[];
  commands?: CommandContribution[];
  statusBarItems?: StatusBarItemContribution[];
  settingsPages?: SettingsPageContribution[];
  aiTasks?: AiTaskContribution[];
  memoryExtractors?: MemoryExtractorContribution[];
  exporters?: ExporterContribution[];
  bibleTypes?: BibleTypeContribution[];
}

export interface ModuleManifest {
  id: string;
  name: string;
  description?: string;
  /** SDK version the module was written against. */
  sdkVersion: string;
  /** First-party modules default to enabled. */
  defaultEnabled?: boolean;
  /** Modules that cannot be disabled (the editor, the binder). */
  required?: boolean;
  /**
   * Called when the module is enabled (app start or Settings). Returns a
   * cleanup function, called when it is disabled. For background wiring
   * such as event subscriptions; never block here.
   */
  activate?: () => (() => void) | void;
  contributes: Contributions;
}

export function defineModule(manifest: ModuleManifest): ModuleManifest {
  return manifest;
}
