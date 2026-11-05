import { afterEach, describe, expect, it, vi } from "vitest";

import { defineModule, useWorkbench } from "@margin/sdk";

import { buildKeymap, chordOf, normalizeChord, resolve } from "./keymap";
import { buildRegistry } from "./registry";

const View = () => null;

describe("keymap", () => {
  it("normalizes chords", () => {
    expect(normalizeChord("Shift+Ctrl+K")).toBe("ctrl+shift+k");
    expect(normalizeChord("Ctrl+Alt+\\")).toBe("ctrl+alt+\\");
  });

  it("reads key events layout-independently", () => {
    const e = { key: "K", code: "KeyK", ctrlKey: true, altKey: false, shiftKey: true, metaKey: false };
    expect(chordOf(e)).toBe("ctrl+shift+k");
    expect(chordOf({ ...e, key: "@", code: "Digit2", shiftKey: false, altKey: true })).toBe("ctrl+alt+2");
  });

  it("skips keys the editor already handled and respects `when`", () => {
    const run = vi.fn();
    const map = buildKeymap([
      { id: "a", title: "A", keybinding: "Ctrl+N", when: () => false, run },
      { id: "b", title: "B", keybinding: "Ctrl+N", run },
    ]);
    const ev = new KeyboardEvent("keydown", { key: "n", code: "KeyN", ctrlKey: true, cancelable: true });
    expect(resolve(map, ev)?.id).toBe("b");
    ev.preventDefault();
    expect(resolve(map, ev)).toBeUndefined();
  });

  it("applies overrides, including unbinding", () => {
    const run = vi.fn();
    const map = buildKeymap([{ id: "a", title: "A", keybinding: "Ctrl+N", run }], { a: null });
    expect(map.size).toBe(0);
  });
});

describe("registry", () => {
  it("collects contributions and resolves the document view", () => {
    const m1 = defineModule({
      id: "m1",
      name: "M1",
      sdkVersion: "0.1.0",
      contributes: { workspaceViews: [{ id: "doc", title: "Doc", opens: "document", component: View }] },
    });
    const m2 = defineModule({
      id: "m2",
      name: "M2",
      sdkVersion: "0.1.0",
      contributes: { sidebarPanels: [{ id: "p", title: "P", side: "left", component: View }] },
    });
    const reg = buildRegistry([m1, m2], () => true);
    expect(reg.viewFor("document")).toBe("doc");
    expect(reg.panels.map((p) => p.moduleId)).toEqual(["m2"]);
  });

  it("drops disabled modules unless required", () => {
    const opt = defineModule({ id: "opt", name: "Opt", sdkVersion: "0.1.0", contributes: { commands: [{ id: "x", title: "X", run: () => {} }] } });
    const req = defineModule({ id: "req", name: "Req", sdkVersion: "0.1.0", required: true, contributes: {} });
    const reg = buildRegistry([opt, req], () => false);
    expect(reg.modules.map((m) => m.id)).toEqual(["req"]);
    expect(reg.commands.size).toBe(0);
  });
});

describe("workbench", () => {
  afterEach(() => useWorkbench.getState().restore([{ id: "left", tabs: [] }], "left"));

  it("opens each document once per pane and activates it", () => {
    useWorkbench.setState({ viewFor: () => "doc" });
    const wb = useWorkbench.getState();
    wb.openDocument("a");
    wb.openDocument("b");
    wb.openDocument("a");
    const left = useWorkbench.getState().panes[0]!;
    expect(left.tabs.map((t) => t.resourceId)).toEqual(["a", "b"]);
    expect(left.tabs.find((t) => t.id === left.activeTabId)?.resourceId).toBe("a");
  });

  it("splits and unsplits", () => {
    useWorkbench.setState({ viewFor: () => "doc" });
    useWorkbench.getState().openDocument("a");
    useWorkbench.getState().toggleSplit();
    expect(useWorkbench.getState().panes.map((p) => p.id)).toEqual(["left", "right"]);
    expect(useWorkbench.getState().focusedPane).toBe("right");
    useWorkbench.getState().toggleSplit();
    expect(useWorkbench.getState().panes.map((p) => p.id)).toEqual(["left"]);
  });

  it("closing the active tab activates a neighbour", () => {
    useWorkbench.setState({ viewFor: () => "doc" });
    const wb = useWorkbench.getState();
    wb.openDocument("a");
    wb.openDocument("b");
    const left = useWorkbench.getState().panes[0]!;
    wb.closeTab(left.activeTabId!);
    const after = useWorkbench.getState().panes[0]!;
    expect(after.tabs.find((t) => t.id === after.activeTabId)?.resourceId).toBe("a");
  });
});
