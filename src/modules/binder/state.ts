import { create } from "zustand";

interface BinderUi {
  selection: string[];
  /** Ids of collapsed nodes and sections (everything else is expanded). */
  collapsed: Record<string, true>;
  renaming: string | null;
  /** The binder was the last focused region (overlays do not count). */
  active: boolean;
  select(ids: string[]): void;
  toggleSelect(id: string): void;
  toggleCollapsed(id: string): void;
  expand(id: string | null | undefined): void;
  collapse(id: string): void;
  startRename(id: string): void;
  stopRename(): void;
}

export const useBinderUi = create<BinderUi>((set) => ({
  selection: [],
  collapsed: { "section:trash": true, "section:frontMatter": true, "section:backMatter": true, "section:research": true },
  renaming: null,
  active: false,
  select: (ids) => set({ selection: ids }),
  toggleSelect: (id) =>
    set((s) => ({
      selection: s.selection.includes(id) ? s.selection.filter((x) => x !== id) : [...s.selection, id],
    })),
  toggleCollapsed: (id) =>
    set((s) => {
      const next = { ...s.collapsed };
      if (next[id]) delete next[id];
      else next[id] = true;
      return { collapsed: next };
    }),
  expand: (id) =>
    set((s) => {
      if (!id || !s.collapsed[id]) return s;
      const next = { ...s.collapsed };
      delete next[id];
      return { collapsed: next };
    }),
  collapse: (id) => set((s) => ({ collapsed: { ...s.collapsed, [id]: true } })),
  startRename: (id) => set({ renaming: id }),
  stopRename: () => set({ renaming: null }),
}));

// Track whether the binder is the region the author is working in, so
// selection commands (duplicate, merge, trash) never act while typing.
if (typeof document !== "undefined") {
  document.addEventListener("focusin", (e) => {
    const t = e.target instanceof Element ? e.target : null;
    if (!t || t.closest(".overlay-backdrop, .popover, .menu")) return;
    const inBinder = t.closest(".binder") !== null;
    if (useBinderUi.getState().active !== inBinder) useBinderUi.setState({ active: inBinder });
  });
}
