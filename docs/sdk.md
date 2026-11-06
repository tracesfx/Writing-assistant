# `@margin/sdk`

The only API that modules use (SPEC §5). `sdkVersion` is `0.4.0`. It stays pre-1.0 while milestones M1–M5 shape it; breaking changes are listed at the end of this file.

## Modules

A module is a folder `src/modules/<name>/` whose `module.ts` default-exports a manifest:

```ts
import { defineModule, sdkVersion } from "@margin/sdk";

export default defineModule({
  id: "binder",
  name: "Binder",
  sdkVersion,
  required: true,           // cannot be disabled
  activate() {              // optional: background wiring when the module is enabled
    const off = events.on("doc.saved", …);
    return off;             // cleanup, called when the module is disabled
  },
  contributes: {
    sidebarPanels: [{ id: "binder", title: "Binder", side: "left", component: BinderPanel }],
    commands: [{ id: "binder.newScene", title: "New scene", keybinding: "Ctrl+N", run: () => … }],
  },
});
```

Register it in `src/modules/index.ts`. Modules import only from `@margin/sdk` (plus shared libraries such as `src/editor/`), never from another module or from `src/app/`.

### Extension points

| Point | Status | Shape |
|---|---|---|
| `workspaceViews` | live | `{ id, title, opens?: "document", component(props: { tabId, resourceId?, active, focused }) }` |
| `sidebarPanels` | live | `{ id, title, side: "left" \| "right", order?, component(props: { visible }) }` |
| `commands` | live | `{ id, title, category?, keybinding?, when?(), allowDefault?, run(ctx) }` |
| `statusBarItems` | live | `{ id, align: "left" \| "right", order?, component }` |
| `settingsPages` | live | `{ id, title, component }`; a page in Settings after the built-in ones (e.g. Backups, Goals) |
| `marginTabs`, `inspectorSections`, `aiTasks`, `memoryExtractors`, `exporters`, `bibleTypes` | placeholder | Shapes are finalised by the milestone that adds their first consumer |

Every contribution is rendered inside an error boundary, so a failing module never takes down the editor. A module turned off in Settings → Modules loses its panels, views, commands and status items, and its `activate()` cleanup runs.

Keybindings: the editor's own keymap runs first. A key the editor handled is never also dispatched as a command. Chords are written like `Ctrl+Shift+K`; letters and digits are matched by physical key. The author can rebind or remove any command's chord (Settings → Shortcuts); overrides live in `settings.keymap`.

## Story Data Service — `story`

All calls are async and run off the UI thread in the backend. Failures throw `StoryError` with `kind`: `io | parse | notFound | invalid | noProject | conflict | internal`.

```ts
story.project.get() / create(req) / open(root) / close() / update(patch)
story.binder.tree() / move(ids, target) / trash(ids) / restore(ids) / duplicate(ids)
story.documents.get(id) / meta(id) / save(id, content, force?) / create(kind, title, target) / updateMeta(id, patch)
story.documents.peek(id) / replace(id, content) / split(id, before, after, title?) / merge(ids)
story.recovery.write(id, content) / list() / get(id) / discard(id)
story.notes.capture(text) / scratchpad() / saveScratchpad(text)
story.comments.list(docId) / put(docId, comment) / delete(docId, commentId)
story.search.text(query) / replace(query, replacement, hits)
story.history.snapshot(name) / list(limit?) / versions(docId) / at(docId, snapshot) / diff(docId, from, to) / restore(snapshot, docIds, label)
story.backups.status() / setConfig(config) / now() / restore(path)
story.stats.summary() / activity(seconds) / sessions()
story.spell.check(texts) / suggest(word) / add(word)
story.app.session() / saveUi(projectId, json) / saveSettings(json) / forgetRecent(root) / beforeClose()
```

- `documents.save` fails with `conflict` if the file changed on disk since Margin last read or wrote it. Pass `force` to keep the editor's version.
- `documents.updateMeta(id, patch)`: fields left out are unchanged. An empty string clears an optional text field, and `target: 0` clears the target.
- Document content is ProseMirror JSON (`PmNode`) in the editor schema (`src/editor/schema.ts`). The backend converts it to and from Markdown.
- `documents.split` / `merge` and `binder.duplicate` carry comment records along with their anchors. Merge moves the other documents to the Trash.
- `search.replace` takes a safety snapshot first and returns it, so the whole replace is one undo step. `history.restore` does the same.
- `documents.peek` reads the file on disk without adopting it (for "compare with disk"); `replace` overwrites a document's body (e.g. restoring a selection).

Reactive hooks: `useProject()`, `useBinderTree()`, plus `findNode(tree, id)` and `locateNode(tree, id)`.

## Events — `events`

```ts
const off = events.on("doc.saved", (e) => e.docId);
```

| Type | Payload |
|---|---|
| `project.opened` | `projectId, title, root` |
| `project.closed` | `projectId` |
| `binder.changed` | `reason, docIds` |
| `doc.saved` | `docId, revision, wordCount` |
| `doc.changedOnDisk` | `docId` |
| `project.settingsChanged` | `projectId` |
| `doc.contentReplaced` | `docIds, reason` — the backend rewrote these bodies (split, merge, replace-all, restore); open editors reload |
| `stats.updated` | `todayWords` |
| `history.snapshot` | `snapshotId, name` |
| `backup.finished` | `path, error` |
| `ai.status` | `status` (`off` | `starting` | `ready` | `thinking` | `pausedTyping` | `notRunning`), `detail` |
| `ai.bakeoffProgress` | `presetId, stage, finished` |
| `ai.ghost` | `requestId, state` (`streaming` | `done` | `empty` | `cancelled` | `failed`), `text` (the whole suggestion so far, already post-processed), `error` |
| `ai.provenanceChanged` | `docId, aiWords` — the AI-accepted ranges of a document were saved |

Events travel inside a versioned envelope `{ v: 1, seq, event }`.

## AI — `ai`

Connection, status, the context log and the bake-off. Wraps the generated commands like `story` does. None of these calls touch manuscript text.

```ts
await ai.connect();                  // start (managed) or connect (external)
const panel = await ai.panel();      // status, VRAM, slots, capabilities, log tail
await ai.keystroke();                // the author typed: cancels ghost text, pauses idle work
const entries = await ai.contextLog.list(50);
const entry = await ai.contextLog.get(entries[0].id); // exact prompt, sections, output, timings
```

Ghost text (SPEC §7), used by the editor module:

```ts
ai.isReady();                          // the model can take a request now (follows ai.status events)
ai.status();                           // "ready" | "thinking" | "pausedTyping" | "starting" | "notRunning" | "off"
await ai.ghost.start({ requestId, docId, textBefore, length, variant, typography });
await ai.ghost.cancel(requestId);      // a cancel that arrives before its request still wins
const off = ai.ghost.subscribe((e) => e.state);   // `ai.ghost` events: streaming text, then done / empty / cancelled / failed
```

`story.provenance.get(docId)`, `.put(record)` and `.totals()` read and write the accepted-AI-text records (`docs/data-format.md`). `aiSession.add(words)` / `useAiSession` hold the words accepted this session, for the status bar.

Requests themselves are made by the backend scheduler (SPEC §6.4): P0 ghost text, P1 on request, P2 idle. The Margin panel of M4 will use the rest. A provider interface (`InferenceClient`) keeps the backend open to other local or, later, cloud providers (SPEC §1.2).

## Workbench — `workbench`, `useWorkbench`

Tabs, panes (`left`, plus `right` when split), and quiet notices:

```ts
workbench.openDocument(id, { pane?: "left" | "right" })
workbench.openView(viewId, resourceId?)
workbench.activeDocumentId() / activeTab()
workbench.notify({ message, action?: { label, run }, timeoutMs?, key? })   // non-modal; never a dialog
workbench.ask({ title, placeholder?, initial? })   // one-line prompt bar; resolves null on Esc
workbench.showPanel(panelId) / togglePanel(panelId)   // sidebar panels, left or right
useActiveDocumentId()
```

A notice with a `key` replaces any earlier notice with the same key (use it for toggles such as "Focus mode: sentence").

## Settings — `settings`, `useSettings`

App-wide preferences (`AppSettings`: theme, editor font/size/line height/width, UI scale, smart typography rules, focus mode, typewriter, spellcheck, keymap overrides, disabled modules), stored in the app state, not the project. `settings.get()`, `settings.update(patch)`, and the `useSettings(selector)` hook.

## UI state — `uiState`, `useUiState`

Small JSON values a module wants restored with the session (cursor positions, bookmarks, a view's layout). Keys are namespaced by module id (`binder.outlineMode`). `uiState.set(key, value)`, `useUiState(key)`. Saved per project with the open tabs.

## Editors — `editorHost`

Open editors register a handle so other modules can reach the text without importing the editor module:

```ts
editorHost.active() / forDoc(docId) / all()
editorHost.openAt(docId, target)       // open and reveal a text range or position
editorHost.flushAll()                  // save every dirty editor (before history, search, merge…)
handle.selectedText() / insertText(text) / reveal(target) / cursor() / setCursor(pos) / focus()
handle.revealComment(id) / removeComment(id) / isDirty() / flush()
```

## Lifecycle — `lifecycle`

`lifecycle.onBeforeClose(async () => …)` registers work that must finish before the window closes or the project switches, such as flushing unsaved text.

## Breaking changes

None yet. 0.4.0 only added the ghost-text and provenance APIs above (`ai.ghost`, `ai.status`, `ai.isReady`, `story.provenance`, `aiSession`, the `ai.ghost` and `ai.provenanceChanged` events, and the ghost-text and Flow-mode settings). 0.3.0 only added the `ai` client and the `ai.*` events. 0.2.0 only added APIs (settings, uiState, editorHost, prompt, right sidebar, `activate()`, notice keys, and the Story calls above).
