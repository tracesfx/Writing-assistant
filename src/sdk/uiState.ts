/**
 * Per-project UI state that survives restarts (cursor positions, bookmarks,
 * the last edited place, panel choices). Modules store small JSON values by
 * key; the shell persists them with the session (SPEC §11.4). Keys should be
 * namespaced by module, e.g. `editor.cursor`.
 */
import { create } from "zustand";

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

interface UiStateStore {
  values: Record<string, Json>;
}

export const useUiStateStore = create<UiStateStore>(() => ({ values: {} }));

export const uiState = {
  get<T>(key: string): T | undefined {
    return useUiStateStore.getState().values[key] as T | undefined;
  },
  set(key: string, value: unknown): void {
    useUiStateStore.setState((s) => ({ values: { ...s.values, [key]: value as Json } }));
  },
  /** Replace everything (session restore). */
  replaceAll(values: Record<string, unknown>): void {
    useUiStateStore.setState({ values: values as Record<string, Json> });
  },
  all(): Record<string, unknown> {
    return useUiStateStore.getState().values;
  },
};

export function useUiState<T>(key: string): T | undefined {
  return useUiStateStore((s) => s.values[key] as T | undefined);
}
