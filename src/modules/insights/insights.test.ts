import { describe, expect, it } from "vitest";

import { setInsights, useInsightsStore, type ModuleManifest } from "@margin/sdk";

import hello from "../../../docs/hello-view/module";
import { buildRegistry } from "../../app/registry";
import { firstPartyModules, orderModules } from "../index";

const ids = (m: ModuleManifest[]) => m.map((x) => x.id);

describe("module discovery", () => {
  it("finds every module folder by itself, in a stable order", () => {
    const found = ids(firstPartyModules);
    for (const id of [
      "editor",
      "binder",
      "margin",
      "insights",
      "insights-progress",
      "insights-presence",
      "insights-graph",
      "insights-timeline",
      "insights-structure",
      "insights-threads",
    ]) {
      expect(found).toContain(id);
    }
    // Known modules keep their order; the rest follow by folder name.
    expect(found.indexOf("editor")).toBeLessThan(found.indexOf("binder"));
    expect(found.indexOf("margin")).toBeLessThan(found.indexOf("insights"));
    expect(found.indexOf("insights")).toBeLessThan(found.indexOf("insights-graph"));
  });

  it("orders unknown folders after known ones, alphabetically", () => {
    const m = (id: string) => ({ id }) as ModuleManifest;
    const out = orderModules([
      ["./zeta/module.ts", m("zeta")],
      ["./binder/module.ts", m("binder")],
      ["./alpha/module.ts", m("alpha")],
      ["./editor/module.ts", m("editor")],
    ]);
    expect(ids(out)).toEqual(["editor", "binder", "alpha", "zeta"]);
  });
});

describe("Insights", () => {
  it("lists the six reference views, each from its own module", () => {
    const reg = buildRegistry(firstPartyModules, () => true);
    const views = reg.insightsViews;
    expect(views.map((v) => v.id)).toEqual(
      expect.arrayContaining(["insights.progress", "insights.presence", "insights.graph", "insights.timeline", "insights.structure", "insights.threads"]),
    );
    const owners = new Set(views.map((v) => v.moduleId));
    expect(owners.size).toBe(views.length); // one module per view
    expect(reg.memoryExtractors.map((x) => x.metricKey)).toContain("tension");
  });

  it("a view is switched off with its module", () => {
    const reg = buildRegistry(firstPartyModules, (m) => m.id !== "insights-graph");
    expect(reg.insightsViews.map((v) => v.id)).not.toContain("insights.graph");
  });

  it("the Hello view sample adds a view with one folder and nothing else", () => {
    const before = buildRegistry(firstPartyModules, () => true).insightsViews.length;
    const reg = buildRegistry([...firstPartyModules, hello], () => true);
    expect(reg.insightsViews.length).toBe(before + 1);
    expect(reg.insightsViews.find((v) => v.id === "insights.hello")?.moduleId).toBe("hello-view");
    expect(reg.commands.has("insights.hello.open")).toBe(true);
    setInsights(reg.insightsViews, reg.memoryExtractors);
    expect(useInsightsStore.getState().views.at(-1)?.id).toBe("insights.hello"); // order 100: last
  });

  it("a duplicate view id is refused rather than shown twice", () => {
    const dup = { ...hello, id: "hello-again" };
    const reg = buildRegistry([hello, dup], () => true);
    expect(reg.insightsViews).toHaveLength(1);
  });
});
