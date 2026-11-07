/**
 * The `marginTabs` extension point (SPEC §5.3): modules contribute tabs to
 * the Margin panel. The shell fills this list from the module registry;
 * the Margin panel reads it. A module never imports another module.
 */
import { create } from "zustand";

import type { MarginTabContribution } from "./extensions";

export type RegisteredMarginTab = MarginTabContribution & { moduleId: string };

export const useMarginTabsStore = create<{ tabs: RegisteredMarginTab[] }>(() => ({ tabs: [] }));
const useTabs = useMarginTabsStore;

export function setMarginTabs(tabs: RegisteredMarginTab[]): void {
  useTabs.setState({ tabs: [...tabs].sort((a, b) => (a.order ?? 0) - (b.order ?? 0)) });
}

export function useMarginTabs(): RegisteredMarginTab[] {
  return useTabs((s) => s.tabs);
}
