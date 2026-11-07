# Hello view: an Insights view in one folder

This is the smallest useful Insights view, and the proof that a view needs
nothing but a module folder (SPEC §16, M5).

1. Copy this folder to `src/modules/hello-view/`.
2. Run the app. **Insights** now lists "Hello view", and the command palette has
   "Insights: Hello view".

That is all. No list to edit, no registration call: `src/modules/index.ts`
finds every `src/modules/*/module.ts` by itself (`import.meta.glob`).

## What a view is

A module with an `insightsViews` contribution:

```ts
insightsViews: [{ id, title, description?, order?, component }]
```

The component receives `{ active }`, which is true while the view is showing.
It reads data through `@margin/sdk` only (`story`, `memory`), and re-reads it
when the project changes by calling `useInsightsData(load, active)`.

## What the SDK gives a view

| Need | SDK |
|---|---|
| Data | `story.*`, `memory.*` (`mentions.matrix`, `relations.graph`, `timeline.events`, `threads.list`, `scenes.cards`, `metrics.series`) |
| Live updates | `useInsightsData(load, active, requestKey?)` (re-reads on `memory.updated`, `binder.changed`, `stats.updated`) |
| Charts | `<Chart option={…} exportRef={…} onClick={…}/>` (Apache ECharts, themed, lazy-loaded) |
| Export | `<ExportBar name csv chart/>` (CSV, PNG, SVG), `toCsv` |
| Layout | `InsightHeader`, `InsightPanel`, `InsightEmpty`, `InsightError` |
| Click through | `openSource(source)`, `workbench.openDocument(id)`, `workbench.openView(BIBLE_ENTRY_VIEW, id)` |
| Chapters | `chapterGroups(tree)`, `useBinderTree()` |
| Extra metrics | declare `memoryExtractors` (see `docs/sdk.md`) |

Every first-party view (`src/modules/insights-*`) is built the same way.
