/**
 * First-party modules, compiled in (SPEC §3.2: no third-party loading in v1).
 *
 * Every `src/modules/<name>/module.ts` is picked up automatically, so a new
 * module (an Insights view, for one) is a new folder and nothing else
 * (SPEC §16 M5). The modules listed first keep their registration order,
 * which decides tie-breaks among panels, status items and commands.
 */
import type { ModuleManifest } from "@margin/sdk";

const FIRST = ["editor", "binder", "search", "inspector", "comments", "notes", "history", "stats", "ai", "bible", "margin", "insights"];

const found = import.meta.glob<{ default: ModuleManifest }>("./*/module.ts", { eager: true });

/** Order the discovered manifests: known ones first, then the rest by folder name. */
export function orderModules(entries: [path: string, manifest: ModuleManifest][]): ModuleManifest[] {
  const folder = (p: string) => p.split("/").at(-2) ?? p;
  const rank = (p: string) => {
    const i = FIRST.indexOf(folder(p));
    return i < 0 ? FIRST.length : i;
  };
  return [...entries].sort((a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0])).map(([, m]) => m);
}

export const firstPartyModules: ModuleManifest[] = orderModules(Object.entries(found).map(([p, m]) => [p, m.default]));
