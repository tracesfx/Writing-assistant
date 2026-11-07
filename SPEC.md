# Margin — Specification

*A local, author-first writing studio with a quiet AI assistant.*
Working name: **Margin** (the AI lives in the margin, not in your text).
Spec version **1.0 (final for v1)** · 2026-11-04

---

## 1. Summary

Margin is a Windows desktop app for writing books (fiction and non-fiction). It has four layers:

- **Writing environment.** A polished, distraction-free editor whose first priority is quality of life.
- **Quiet local AI.** Ghost-text autocomplete and a side panel of ideas.
- **Story Memory.** A transparent, fully inspectable model of the book (characters, places, relationships, timeline, threads). The AI builds it and the author owns it.
- **Extension framework.** Visualizations, charts, and new tools are built on the same data and APIs the core uses.

Everything runs on the author's machine. Nothing leaves `localhost`.

### 1.1 Target environment

| | |
|---|---|
| OS | Windows 11 |
| GPU | RTX 3060 12 GB |
| CPU / RAM | Ryzen 5 3600X (6C/12T) · 32 GB |
| Default model | **Ternary Bonsai 9B** (`PQ2_0`) via the PrismML `llama-server` build, chosen by the author after the M2 bake-off |
| Also supported | Ternary Bonsai 27B (`PTQ1_0`, `PQ2_0`), any other GGUF through a custom preset, any OpenAI-compatible endpoint (incl. the author's custom harness) |

### 1.2 Decisions log

| Topic | Decision |
|---|---|
| Platform | Tauri 2 desktop app (Windows first) |
| AI features (v1) | Ghost-text autocomplete · Margin side panel (ideas) · Story Memory |
| Editorial critique | **Not in v1.** Must be addable later as an AI-task module (§5) |
| Model strategy | **Single model, low latency.** Default Ternary Bonsai 9B (the author's choice from the M2 bake-off). The built-in bake-off stays for comparing future models |
| Genre | Fiction and non-fiction (project type: Fiction / Non-fiction / Both) |
| Language | **English only** for v1 |
| Spelling | **US/Canadian.** Default dictionary `en-US`, `en-CA` selectable per project. Double curly quotes “ ” and US punctuation conventions |
| Import | None (author is starting fresh) |
| Publishing | **Not a priority yet.** Book formatting and preview stay in scope. Print-production extras (platform presets, preflight) are deferred |
| Export | Markdown/TXT and DOCX first. Formatted PDF later in v1 |
| Provenance stats | **"Your words vs AI words" visible by default** |
| Priorities | **1. Quality of life · 2. Extensibility · 3. Transparent, accessible memory** |
| Binder storage | Every binder node (part, chapter, scene, note, folder) is a `<slug>--<id>.md` file with `kind:` in its frontmatter. `project.yaml` holds only the ordered ID tree per section (§13) |
| Author data location | Comments (`comments/<docId>.json`), writing stats (`stats/sessions.json`), the scratchpad (`scratchpad.md`) and the personal dictionary (`dictionary.txt`) are plain project files, not `.margin/`: they are the author's data, travel with the folder, and are backed up and versioned. `.margin/` keeps only rebuildable caches and app state (§13) |
| Local server ownership | Margin runs its own managed `llama-server` (default port 8080) and never shares it with other apps. The author does not run their game harness (which owns a server on 8081) at the same time. External mode remains for any compatible endpoint; there is no integration with the harness API |
| Default model (M2 result, decided 2026-11-05) | The bake-off (GPU free) found Gemma 4 26B-A4B **missed the §6.6 speed limits** on the target machine (warm first token p50 381–413 ms against a limit of 350, p95 521–849 ms against 700, decode 16–23 tok/s against 20). Nemo 12B, Gemma 4 E4B, Bonsai 9B and both Bonsai 27B variants passed. The author then judged quality blind and chose **Ternary Bonsai 9B (`PQ2_0`)** as the default. The Gemma 4 and Mistral Nemo presets were removed from the built-ins (saved copies are dropped on upgrade, preset revision 3); their model files are left on disk, and the Gemma and Mistral prompt templates stay in the code for custom presets. 9B runs with `-c 32768 --parallel 2` (two slots of 16K), because ghost text may send ~8K tokens. Numbers are in `docs/progress.md` |
| Bake-off candidates | The M2 run compared Gemma 4 26B-A4B, Mistral Nemo 12B and Gemma 4 E4B with Ternary Bonsai 2 (27B `PTQ1_0`, 27B `PQ2_0`, 9B `PQ2_0`). Only the three Bonsai presets are built in now. Models live under `D:/MODELS` |
| Future cloud APIs | Optional OpenAI and Anthropic API providers are wanted **after v1; not in M2** (nor in any current milestone). Planned as extra `InferenceClient` implementations behind the same scheduler and context log, opt-in per role, with keys in the OS credential store and a visible "leaves this machine" indicator. Doing so means amending Principle 8 and the network rule (§14) in the same change. M2 keeps the client behind a trait so this needs no rewrite |
| Writing stats | "Words written" is the net change from the author's edits to manuscript documents. Moving, duplicating, splitting and merging documents is not writing |
| Markdown dialect | Implemented once, in Rust. The editor exchanges ProseMirror-shaped JSON with the backend. Losslessness is defined on the canonical form: `serialize(parse(md)) == md` for canonical files, and `parse(serialize(doc)) == doc` |
| Ghost text keys and focus (M3) | While a suggestion shows, **Esc is handled inside the editor** (ProseMirror consumes Esc before the app keymap sees it), so it cannot be rebound. Tab, Ctrl+→ and Alt+] / Alt+[ are ordinary rebindable commands (`editor.ghost.*`). A suggestion is dropped when the editor loses focus. A request made right after a key press waits for that key's notification to reach the backend: IPC calls are separate requests and can overtake each other, which cancelled Alt+] alternatives |
| Provenance storage (M3) | Accepted AI text is tracked as editor plugin ranges (never a mark or node), mapped through edits, and saved to `.margin/provenance/<docId>.json` as UTF-16 offsets into the document's plain text (non-empty text blocks joined by a blank line, so empty paragraphs do not matter) plus the SHA-256 of that text, once the document itself is saved. A hash mismatch drops the record with one notice. It is not rebuildable, yet `.margin/` is left out of backups and history: moving it to the project's own files is an open question for the author |
| Paragraph ids (M4) | Ids live in a sidecar map per document, `memory/paragraphs/<docId>.json`, reconciled after every save (identical text keeps its id, edited text keeps it by word overlap ≥ 0.6, new text gets a new ULID). They are **not** editor node attributes: the editor, the Markdown and the undo history are untouched, and a lost sidecar degrades a citation to its quote and block hint. Sidecars are plain project files, so they are backed up and versioned |
| Memory index store (M4) | No `index.db`. Entries, facts, relations, scene cards, the timeline and summaries are plain files (§9.3). Mentions are derived from the text on demand. Paragraph vectors are a flat cache, `.margin/embeddings.json`, searched by brute-force cosine: at book scale that is as fast as `sqlite-vec` and needs no native extension. Keyword search (BM25) always works; embeddings are optional and need a GGUF embedding model the author downloads (Margin never downloads models), run as a second CPU `llama-server --embedding` (§9.5) |
| Reading a scene (M4) | One grammar-constrained call per scene, or per window of whole paragraphs (≈4,500 tokens) for a scene too long for the 16K slot. llama.cpp writes object keys **alphabetically**, so the schema's keys carry letter prefixes (`a_scene`, `b_entities`, …) and the summary and cast come out first; a truncated answer is repaired. Relation kinds are a closed list. Before anything is kept: an entry must be named in the scene text, a fact must be a sentence that names its entry, a relation's paragraph must name both entries, and every source points at a real paragraph (a quote that is not in the paragraph becomes the paragraph's opening). Scenes under 12 words and AI-off or forgotten documents are not read. A reading never changes canon (§9.1, §17 check 9) |
| Margin panel shortcuts (M4) | `Ctrl+Shift+M` toggles the Margin panel, as §8 says. The Binder's "merge selected documents" also had `Ctrl+Shift+M` (M1); it applies only with two or more binder items selected and wins then, so the key is context-sensitive and both stay rebindable |
| "To note" (M4) | A scene has no notes field (the file format is unchanged), so "To note" appends to a note document named "<scene> — notes" in the Notes section, created on first use. Pins live in `memory/ideas.json` |
| Context log ids (M4) | A context-log entry is named by its request's job id (a ULID), so a card's "Why?" and `Ctrl+Shift+L` open exactly that request |
| Memory exclusions (M4) | "Forget this document" is a list in `memory/settings.json`. Documents with AI off are excluded too |
| Insights views (M5) | Insights is a workspace view whose own extension point, `insightsViews`, lists the views other modules contribute (§5.3). Each of the six reference views is its own module folder (`src/modules/insights-*`); the module list is found with `import.meta.glob`, so a new view is one new folder (proven by `docs/hello-view/`). Charts use Apache ECharts and the relationship graph Cytoscape.js, both bundled and loaded only when a view first draws. Shared plumbing (the themed chart, export bar, data hook) lives in the SDK, because modules may not import each other |
| Insights data (M5) | Views read only through the SDK. They re-read on `memory.updated`, `binder.changed` and `stats.updated`, batched over 250 ms, and not while hidden. Every view exports its data as CSV, and its chart as PNG and SVG, into a folder the author picks (the native folder picker, no confirmation); an existing file is never overwritten. Every item opens its passage or entry in at most two clicks: a heatmap cell, a graph relation, a timeline event, a thread dot, a chapter bar |
| `tension` extractor (M5) | The first `memoryExtractors` entry. The reading of a scene also reports `g_metrics.a_tension`, an integer 0-10, in the same grammar-constrained call (no second call), stored as the scene card's `metrics.tension`. Scenes are averaged per chapter weighted by words. It is on by default (Settings → Memory); a card made before it was on, or while it was off, is read again once it is switched on (the card records the extractors it asked for in `extractors`, so a scene the model could not rate is not re-read for ever). An extractor is Rust (`memory::extract::EXTRACTORS`); the module declares the metric so the views offer it |
| Story order of events (M5) | The model reports an event's story time only as a label ("that night"), so a Timeline with real story order needs the author: the Timeline view moves an event earlier or later in story time. That sets `storyTime.order` (a number on the scale of narrative position, 1 = the first event told) and, as an author action, makes the event canon, so a later reading of its scene leaves it alone. Events without a number sit where the book tells them (hollow dots) |
| Relationship graph "as of chapter N" (M5) | A relation is shown when the passage it was read from is in a chapter at or before N; a relation with no recorded passage is always shown |

---

## 2. Principles (these override everything else)

1. **The author's words are the source of truth.** The AI never inserts, deletes, or rewrites manuscript text on its own. The only way AI text enters the manuscript is an explicit accept (Tab, word-accept key, or an "Insert"/"Replace" button).
2. **Quiet by default.** No pop-ups, modals, sounds, or unprompted panels. AI activity shows as one small status-bar indicator.
3. **Ideas over prose.** The side panel suggests *directions*, not paragraphs to paste. Draft snippets are opt-in.
4. **Never in the way.** Typing is never delayed by the AI or by IO. Any keystroke cancels in-flight ghost text. Background AI runs only while the author is idle.
5. **Transparent memory.** Everything the model tracks about the book can be seen, searched, edited, exported, and traced back to the passage it came from. There is no hidden state. The author can always see *exactly* what context was sent with any AI request.
6. **Quality of life first.** Small frictions are bugs. Every feature must be keyboard-reachable, undoable, non-blocking, and safe against data loss.
7. **Build on the platform.** Core features use the same public internal APIs (data queries, events, extension points) that future modules will use. No feature reaches around them.
8. **Local and private.** No telemetry, no accounts, no cloud. Network access is restricted to configured local endpoints.
9. **Own your files.** Manuscript and memory are plain Markdown/YAML/JSON in a normal folder, readable without the app.
10. **Easy to turn off.** There is a global AI kill switch (`Ctrl+Shift+Space`), a per-document "AI off" toggle, and per-feature toggles.

---

## 3. Scope

### 3.1 In scope (v1)
- Project and manuscript management: binder, scenes and chapters, notes, research
- Semantic-style editor: focus, typewriter, and page views; split view
- **Quality-of-life suite (§11):**
  - autosave and crash recovery, version history, backups
  - session restore, quick capture, sprints
  - goals and stats
  - project-wide find/replace and entity rename
  - story-aware spellcheck
  - command palette, rebindable keys, read-aloud
- Local model integration with managed or external server, health monitoring, and the **model bake-off**
- Ghost-text autocomplete
- Margin panel: Ideas, Ask my book, Phrasing alternatives
- Story Memory with **Memory Inspector**, **Context Inspector**, and **memory export**
- **Extension framework (§5)**, plus an **Insights** workspace with reference visualizations (§10)
- Book formatting (semantic styles, templates, live paged preview)
- Export: Markdown/TXT, DOCX (Standard Manuscript Format and styled), formatted PDF

### 3.2 Out of scope (v1)
- Editorial critique: pacing, continuity reports, line edits. Designed to slot in as a module later.
- Print-production extras: KDP/IngramSpark presets, gutter-by-page-count tables, bleed, preflight
- EPUB export
- Import from DOCX, Scrivener, or Google Docs
- Third-party plugin *loading*. The framework exists, but v1 modules are first-party and compiled in.
- Cloud model APIs (OpenAI, Anthropic): wanted later, not in v1 as scoped (see decisions log, "Future cloud APIs")
- Cloud sync, collaboration, mobile
- Grammar checking beyond spelling
- Any AI feature that rewrites the author's text in place
- Non-English languages

---

## 4. Architecture

```
┌──────────────────────────────── Margin (Tauri 2) ────────────────────────────────┐
│ Frontend (WebView2) — TypeScript + React                                         │
│  ├─ Shell: workspace layout, panels, command palette, status bar, keymap         │
│  ├─ Editor: TipTap/ProseMirror (ghost text = decorations, never doc nodes)        │
│  ├─ Modules (first-party): Binder · Corkboard · Bible · Margin panel ·           │
│  │   Memory Inspector · Insights views · Export UI · Stats …                     │
│  └─ Module SDK (TS): story query client · event bus · extension registry          │
│                         ▲  Tauri IPC: typed commands + streamed events            │
│ Backend (Rust)          ▼  (TS bindings generated via tauri-specta)               │
│  ├─ Project Store      Markdown/YAML/JSON files, atomic writes, file watcher      │
│  ├─ History            embedded git (git2): snapshots, diffs, restore             │
│  ├─ Story Data Service canonical query/command API over manuscript + memory       │
│  ├─ Event Bus          domain events (doc.saved, memory.updated, …)               │
│  ├─ AI Scheduler       priority queue, cancellation, slot routing, AI-task registry│
│  ├─ Context Builder    budgeted prompt assembly + context logging                 │
│  ├─ Memory Indexer     summaries, entities, facts, relations, mentions, metrics   │
│  ├─ Index DB           SQLite + sqlite-vec (embeddings, caches) — rebuildable     │
│  ├─ Stats Service      sessions, word counts, provenance, time series             │
│  ├─ Spellcheck         Hunspell-compatible (e.g. `spellbook` crate)               │
│  └─ Exporters          Markdown/TXT · docx-rs · Typst (PDF + preview pages)       │
└───────────────┬──────────────────────────────────────────┬──────────────────────┘
                │ HTTP 127.0.0.1                           │ HTTP 127.0.0.1
     ┌──────────▼──────────┐                    ┌──────────▼──────────┐
     │ llama-server (GPU)  │                    │ Embedding server    │
     │ Gemma 4 26B-A4B     │                    │ (CPU, ~300M model,  │
     │ slot 0 ghost text   │                    │  e.g. EmbeddingGemma)│
     │ slot 1 background   │                    └─────────────────────┘
     └─────────────────────┘
```

### 4.1 Technology choices

| Area | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** | Native, small install, low RAM, Rust backend |
| UI | React + TypeScript (strict) | Ecosystem for ProseMirror and charts |
| Editor | **TipTap (ProseMirror)** | Decorations for ghost text. Custom nodes and marks for book semantics |
| IPC types | `tauri-specta` | One source of truth for command and event types (Rust → TS) |
| Storage | Markdown + YAML frontmatter, JSON for memory | Human-readable, diffable |
| Cache / index | SQLite + `sqlite-vec` | Embeddings and derived caches. Safe to delete and rebuild |
| History | `git2` (hidden repo) | Snapshots, diffs, dedupe |
| Charts | **Apache ECharts** | Time series, heatmaps, timelines, Sankey. Themeable, fast |
| Graphs | **Cytoscape.js** | Relationship and entity networks |
| Typesetting | **Typst** (embedded crate) | Real typesetting. The same engine produces the PDF and the preview |
| DOCX | `docx-rs` | Style-mapped output |
| Inference | `llama-server` (llama.cpp) | Prefix caching, slots, JSON-schema output, streaming, cancellation |

---

## 5. Extensibility framework

Goal: visualizations, charts, new AI tasks (e.g. future editorial feedback), new exporters, and new panels can be added **without touching core code**. They are built only on the APIs below. All v1 features are themselves built as modules on this framework ("dogfooding").

### 5.1 Story Data Service (single source of truth)
- A Rust service exposing **typed read queries** and **validated write commands** over:
  - manuscript structure
  - documents
  - Story Memory (§9.3 data model)
  - stats
  - settings
- **Reads** are open to every module.
- **Writes** go through commands that enforce the rules:
  - canon is never overwritten by AI
  - manuscript text changes only via the editor, or an explicit author action like Rename entity
  - every write emits events
- Frontend access is through the **Module SDK** (`@margin/sdk`, in-repo package). Indicative shape:

```ts
story.project.get()
story.binder.tree()
story.scenes.list({ chapterId? }) / story.scenes.card(sceneId)            // AI scene cards
story.entities.list({ type?, query? }) / story.entities.get(id)
story.facts.forEntity(id, { status? })                                  // canon | inferred
story.relations.list({ entityId?, type? }) / story.relations.graph()     // nodes + edges
story.mentions.byEntity(id) / story.mentions.matrix({ types, by: 'scene'|'chapter' })
story.timeline.events({ order: 'story'|'narrative' })
story.threads.list({ status? })
story.metrics.series(metricKey, { by: 'scene'|'chapter'|'day' })        // e.g. words, tension
story.stats.sessions({ from, to }) / story.stats.provenance()
story.search.semantic(text, { k }) / story.search.text(query, opts)
story.ai.contextLog({ limit })                                           // what was sent to the model
events.on('memory.updated', ({ entityIds, sceneIds }) => …)
```

### 5.2 Event bus
Domain events (Rust → frontend and between backend services). They are typed and versioned.

- `project.opened` · `binder.changed`
- `doc.changed` (debounced) · `doc.saved`
- `memory.updated {entityIds, sceneIds, kinds}` · `index.progress`
- `stats.updated` · `session.started/ended`
- `ai.request.started/finished {role, contextId}` · `model.status`
- `export.finished`

### 5.3 Extension points (module manifest)
Each module lives in `src/modules/<name>/` with a `module.ts` manifest declaring what it contributes:

| Extension point | Example |
|---|---|
| `workspaceViews` | full-size tab: Insights → Relationship Graph |
| `sidebarPanels` | docked side panel: Binder (v1 consumer); later Outline, Bible browser, Search results |
| `marginTabs` | a tab in the Margin panel |
| `insightsViews` | a view in the Insights workspace (M5): Writing progress, Character presence, … |
| `inspectorSections` | extra section in the document/entity inspector |
| `commands` | command-palette entries + default keybindings |
| `statusBarItems` | e.g. sprint timer |
| `settingsPages` | module settings |
| `aiTasks` | Registered AI job: prompt template, JSON schema, priority, trigger (manual / idle / on-save), and output handler. Future editorial feedback plugs in here |
| `memoryExtractors` | Extra fields extracted per scene (e.g. a `tension` metric, 0–10). Produces new `metrics` keys automatically available to charts (live in M5: `tension`) |
| `exporters` | an output format (Rust trait on the backend + UI options) |
| `bibleTypes` | new entry types with field schemas (e.g. "Magic system", "Spaceship") |

Rules:
- Modules can be enabled or disabled in Settings.
- A module that throws is isolated by an error boundary and never takes down the editor.
- Backend extensions (exporters, extractors, AI tasks with Rust handlers) implement documented Rust traits and are registered at startup.

### 5.4 Schema and versioning
- Every data file and table carries `schemaVersion`. Forward migrations run at project open, and a backup is taken first.
- The SDK API is versioned (`sdkVersion`). Breaking changes require a migration note in `docs/`.

---

## 6. Local model integration

### 6.1 Connection modes
- **Managed (default).** Margin launches and supervises `llama-server` from a user-chosen binary and model file with configurable arguments. It shows startup progress and restarts on crash (up to 3 times, then notifies once).
- **External.** Connects to an existing endpoint, e.g. the author's custom harness.
  - `llama-server` native (`/completion`, `/tokenize`, `/health`, `/slots`) is preferred.
  - OpenAI-compatible (`/v1/completions`, `/v1/chat/completions`) is the fallback.
- **Capability probe** on connect enables or disables features. Without raw completion, ghost text uses chat completion with assistant prefill where supported. The probe also asks the server to render a sample chat with its own template and warns in the Model panel if that differs from the template Margin sends.
- **Adopting a leftover server.** If a server is already answering on Margin's managed port (for example from a session that ended abnormally), Margin attaches to it instead of starting a second one.
- **Endpoints are loopback only** (127.0.0.1, `localhost`, `::1`). The HTTP client is built without TLS, with redirects and proxies off.

### 6.2 Default model profile: Ternary Bonsai 9B (chosen after the M2 bake-off)
```
llama-server -m Ternary-Bonsai-2-9B-PQ2_0.gguf       # PrismML build b12128
  --jinja -ngl 99 -fa on --no-webui --reasoning off
  -c 32768 --parallel 2               # 2 slots of 16K: ghost text may send ~8K tokens
  --host 127.0.0.1 --port 8080
```
- **Thinking is off** for all roles. The template is ChatML (Qwen3 style). Raw prompts prefill an empty think block (`<think>\n\n</think>\n\n`); the server also runs with `--reasoning off`.
- **Measured on the target machine** (bake-off, GPU free): 4.4 GB VRAM and 7.9 GB headroom, warm first token p50 121 ms and p95 140 ms, 86 tok/s, a cold 6K-token prompt in 4.3 s. In the app (M3, real keystrokes): a phrase suggestion is complete about 250 ms after the first token request, and an alternative about 100 ms after the key.
- Flags are starting points and must be verified against the llama.cpp build in use.
- **Other templates, kept for custom presets.** Gemma 4's turn format (verified against `llama-server` `/apply-template`, 2026-11-05): `<|turn>system\n…<turn|>\n<|turn>user\n…<turn|>\n<|turn>model\n`, stop token `<turn|>`; its template adds a `<|think|>` line only when `enable_thinking` is true, so raw prompts leave it out. Mistral: `[INST] system\n\nuser[/INST]`, stop `</s>`. The PrismML build loads Gemma 4.

### 6.3 Presets

| Preset | Notes |
|---|---|
| Ternary Bonsai 9B *(default)* | PrismML fork. 2.4 GB file. Two slots of 16K. Thinking off by prefill |
| Ternary Bonsai 27B (`PTQ1_0`) | 5.9 GB file. Slower, 16K context in total |
| Ternary Bonsai 27B (`PQ2_0`) | 7.2 GB file |
| Custom | Any GGUF (with its template: ChatML, Gemma or Mistral) or compatible endpoint |

Each preset stores: template handling, stop sequences, thinking switch, sampling per role, and context size. Saved presets carry a revision; when the app improves a built-in's template, thinking switch or stop tokens, existing configs pick that up and keep the author's paths and arguments (`docs/data-format.md`). Revision 3 also retired the Gemma and Nemo presets.

### 6.4 AI Scheduler

| Priority | Role | Slot | Runs when | Pre-emption |
|---|---|---|---|---|
| P0 | Ghost text | 0 | typing pause | cancelled by any keystroke |
| P1 | On-request (Margin panel, Ask, Phrasing) | 1 | author asks | pre-empts P2 |
| P2 | Memory indexing, idle ideas, registered idle `aiTasks` | 1 | author idle ≥ 20 s | paused on keystroke, resumed when idle |

- Closing the HTTP stream cancels generation. The scheduler cancels within one frame of the keystroke reaching the backend.
- Ghost text is pinned to slot 0 so its prefix cache is never evicted.
- P2 is paused while P0 is in flight.

### 6.5 Sampling defaults (editable per role)

| Role | temp | top_p | min_p | max tokens | Other |
|---|---|---|---|---|---|
| Ghost text | 0.7 | 0.9 | 0.05 | word/phrase/sentence = 6/16/40 | stop at `\n`, and at sentence end in sentence mode |
| Ideas | 0.9 | 0.95 | 0.05 | 400 | JSON schema |
| Ask my book | 0.3 | 0.9 | 0.05 | 400 | must cite sources |
| Memory extraction | 0.1 | 1.0 | 0.0 | 1200 | strict JSON schema (grammar-constrained) |

### 6.6 Model bake-off (Settings → Models → Bake-off)
1. Choose models (presets or custom). Text comes from the author's manuscript, or a bundled public-domain sample while the project is new.
2. **Speed test:**
   - VRAM (via `nvidia-smi`)
   - cold TTFT on a 6K-token prompt
   - **warm TTFT** with cached prefix
   - decode tok/s
   - p50/p95 ghost latency over 30 simulated pauses
3. **Blind quality test:**
   - 10 cut points, with continuations shown anonymised as A/B/C/D (A–F when more models are compared); the author picks a winner or "none", and model names appear only after the last answer
   - 3 "Ask my book" questions with known answers
4. A results table with **Use this model** writes the preset plus the tuned `--n-cpu-moe`. Results are saved to `.margin/bakeoff/` and viewable in Insights.

**Go/no-go for the default model** (target machine):
- warm TTFT p50 ≤ 350 ms, p95 ≤ 700 ms
- decode ≥ 20 tok/s
- a sentence ghost completes in ≤ 2 s
- VRAM headroom ≥ 0.6 GB

### 6.7 Health and status
The status bar shows **AI: Off · Starting · Ready · Thinking · Paused (typing) · Model not running**. Clicking it opens the Model panel:
- server log tail
- VRAM
- slot states
- last request latency
- restart button

---

## 7. Ghost-text autocomplete

### 7.1 Behaviour
- **Trigger.** Typing pause ≥ debounce (default **600 ms**, range 250–2000 ms).
- **Fires only when:**
  - the cursor is at the end of the paragraph, or only whitespace follows it
  - there is no selection or IME composition
  - the block is a body paragraph
  - AI is on for this document
  - the current or previous paragraph has ≥ 3 words
- **Suppressed:**
  - for 1.5 s after a dismiss at the same position
  - during a Backspace streak
  - in **Flow mode** (ghost off, everything else on; `Ctrl+Alt+F`)
- **Streaming.** Tokens fade in (≤ 120 ms opacity, no motion).
- **Length setting:** word / **phrase** (default) / sentence. Output is trimmed to a clean boundary.

### 7.2 Controls (rebindable)

| Key | Action |
|---|---|
| `Tab` | accept all (only while a ghost is visible; otherwise normal Tab) |
| `Ctrl+→` | accept next word |
| `Esc` | dismiss |
| `Alt+]` / `Alt+[` | next / previous alternative (lazy, up to 3) |
| keep typing | if the typed characters match the ghost's prefix, the ghost shrinks; otherwise it is discarded |

Esc is handled by the editor itself while a suggestion shows, so it is fixed; the other keys are rebindable commands. A suggestion is also dropped when the cursor moves, the selection changes, or the editor loses focus. An alternative that repeats an earlier one is retried once. `Alt+]` with no suggestion showing asks for one.

### 7.3 Rendering
- ProseMirror widget decoration. It is never in the document, undo stack, word count, or saved files.
- Author's text colour at configurable opacity (default 40%), optionally italic.
- **Never push text** (default on). The ghost is clipped to the rest of the current visual line, with a faint "…", so the lines below never reflow. Accept inserts the full suggestion.
- An accepted suggestion is a single undo step.

### 7.4 Prompt construction (ordered for prefix-cache reuse)
```
[1] Role instructions                                   stable per session
[2] Project primer: genre, POV, tense, style sheet,     changes when bible/style changes
    spelling convention (e.g. "Canadian English: colour, centre, -ize")
[3] Book synopsis + current chapter summary             changes at scene boundaries
[4] Bible cards for entities in the current scene       refreshed at paragraph boundaries only
[5] Current scene text up to the last ~2 paragraphs
[6] Assistant turn prefilled with last ~2 paragraphs up to the cursor  ← model continues
```
- **Budget** ~8K tokens: [1–2] ≤ 800, [3] ≤ 600, [4] ≤ 1,200, [5–6] the remainder.
- **Role instruction (draft):** *"You are a silent co-writer. Continue the author's text exactly where it stops, matching their voice, tense, point of view and spelling. Output only the continuation. Never summarise, explain, or add commentary."*
- Every assembled prompt is recorded in the **context log** (§9.6).

### 7.5 Post-processing
- Fix leading whitespace (after a space, mid-word, or after an opening quote)
- Remove repeats of the author's last words
- Reject meta-text ("Sure,", "Here is…") and chat-template tokens
- Apply smart-typography rules
- Normalise spelling variants to the project dictionary where unambiguous (e.g. color↔colour)
- Discard if empty

### 7.6 Provenance ("your words first")
- Accepted AI text carries an invisible `ai` mark. Characters the author retypes or edits lose the mark.
- **Stats visible by default:**
  - the status bar shows session words *yours / AI-accepted*
  - Stats and Insights show per-document and project totals
- Optional **Highlight AI-assisted text** view (off by default).
- Saved to `.margin/provenance/<doc-id>.json` as ranges plus a content hash. They are not saved in the Markdown. If the file is edited outside Margin and the hash no longer matches, provenance for that document is dropped, with one notice.

---

## 8. Margin panel

The panel is collapsed by default and toggled with `Ctrl+Shift+M`. It never opens itself. An optional dot on the toggle signals fresh ideas (off by default).

### 8.1 Tabs
1. **Ideas.** One- or two-sentence cards, generated:
   - on request (`Ctrl+Shift+I`), or
   - automatically at paragraph or scene end after ≥ 20 s idle, **only while the panel is open** (default on).

   Card types:
   - *Fiction:* what could happen next · raise the stakes · hidden reaction · sensory detail · unresolved-thread reminder · question to ask yourself
   - *Non-fiction:* next point · counter-argument · missing evidence · clarify a term · transition
2. **Ask my book.** Questions are answered **only from Story Memory and the manuscript**, with clickable citations that jump to the passage. If there is no evidence, the answer says so and does not guess.
3. **Phrasing.** Select text, then press `Ctrl+Shift+P`. Shows 3–5 alternatives. The selection is untouched until the author clicks *Replace selection* (one undo step) or *Copy*.

### 8.2 Card actions
- **Pin** (persists in project notes)
- **Dismiss** (feeds a session "don't suggest" list)
- **Expand** (more detail, still ideas)
- **To note** (appends to the scene's notes)
- **Why?** (opens the Context Inspector for that request)

### 8.3 Ideas schema
```json
{ "ideas": [ { "type": "next|stakes|reaction|sensory|thread|question|point|counter|evidence|term|transition",
               "text": "≤ 240 chars", "refs": ["<sceneId>#<paraId>"] } ] }
```

---

## 9. Story Memory

### 9.1 Authority rules
- Author-written or author-confirmed facts are **canon**. **The AI never changes or deletes canon.**
- AI-extracted facts are **inferred**. They show a dotted underline and a "✓ make canon" control. There is no review queue and no pop-ups.
- When text contradicts an inferred fact, the indexer updates the fact and keeps its history.
- **Conflict flagging** against canon is opt-in (default off). When on, it shows only inside the bible entry and in the Memory Inspector, never in the editor.
- Aliases are proposed by the AI and editable by the author.
- **Forget this document** excludes a document from memory entirely.

### 9.2 Bible entry types
- **Fiction:** Character · Location · Item · Faction/Group · Event · Plot thread · Glossary term
- **Non-fiction:** Concept/Term · Claim/Argument · Source · Person/Organisation · Outline node
- **Both:** Style sheet (spellings, capitalisation, number style, dialogue conventions, POV/tense rules)
- Modules can add types via `bibleTypes` (§5.3).

### 9.3 Data model (all of it is reachable through the Story Data Service and exportable)

| Record | Key fields | Stored in |
|---|---|---|
| **Entity** | `id, type, name, aliases[], fields{}, facts[], relations[], tags[], status, createdBy` | `bible/<type>/<slug>--<id>.md` (YAML frontmatter + author notes body) |
| **Fact** | `id, text, field?, status(canon\|inferred), source{doc, para, quote}, history[]` | inside its entity |
| **Relation** | `id, from, to, type (sibling, parent, ally, rival, romantic, member-of, located-in, owns, mentor, custom…), directed, status, source, since?(story time)` | inside the *from* entity |
| **Scene card** | `sceneId, summary, pov, tense, location, storyTime{label, order?}, entitiesPresent[], threadsTouched[], metrics{}` | `memory/scenes/<sceneId>.json` |
| **Chapter / book summary** | `id, summary, sourceHashes[]` | `memory/chapters/<id>.json`, `memory/synopsis.json` |
| **Event** (timeline) | `id, title, storyTime{label, order}, narrativePos{sceneId}, entities[], source` | `memory/timeline.json` |
| **Thread** | `id, title, status(open\|resolved\|dropped), introducedIn, touches[], resolvedIn?` | `bible/threads/…md` |
| **Mention** | `entityId, sceneId, paraId, count, kind(name\|alias\|pronoun-inferred?)` | derived. `index.db` (rebuildable), exportable |
| **Metric** | `key, scope(scene\|chapter), value, source(extractor id)` | in scene cards. Built-in keys: `words`, `dialogueRatio`, `entityCount`. Optional extractor: `tension`, `sentiment` |
| **Embedding** | `paraId/summaryId, vector, model` | `index.db` only (cache) |

- Paragraph IDs (`paraId`) are stable node attributes in the editor, persisted as hidden IDs in a sidecar map, so citations survive edits.
- **Everything except embeddings, mentions, and caches lives in plain files** inside the project folder and is versioned by the history system.

### 9.4 Indexing pipeline (P2, idle-only)
1. A scene becomes **dirty** when its content hash changes.
2. It is indexed after ≥ 60 s unchanged **and** the author is idle. Scenes nearest the cursor go first.
3. Per scene:
   1. Re-embed changed paragraphs.
   2. Run one grammar-constrained extraction call. It returns the summary, POV/tense, location, story time, entities and new aliases, facts with paragraph anchors, relations, events, thread updates, and the metrics from registered `memoryExtractors`.
   3. Merge the results, respecting canon.
   4. Recompute mentions.
   5. Mark the parent summaries stale.
4. Emit `memory.updated`.
5. **Status indicator:** "Memory: up to date / 3 scenes pending / paused (typing)".
6. Commands: Re-index scene · Rebuild all memory · Forget this document.

### 9.5 Retrieval (Context Builder)
- **Entity match** (names and aliases in the paragraph and scene), then the relevant facts and relations.
- **Semantic recall** over paragraphs and summaries, reranked by scene distance and deduplicated.
- **Hierarchy:** book synopsis → chapter summary → previous 2 scene cards → current scene text.
- **Budgets:** ghost text ~8K. Ideas and Ask ~14K (inside the 16K slot).
- **Embeddings** run on CPU in a second `llama-server --embedding` instance (e.g. EmbeddingGemma ~300M), so they don't compete for VRAM.

### 9.6 Making memory accessible
- **Story Bible view.** Browse, edit, and create entries. Each fact shows its status and source link. A history tab per entry.
- **Memory Inspector** (workspace view). One searchable, filterable table of *everything* the model tracks: entities, facts, relations, events, threads, scene cards, mentions, and metrics. Columns: status, source, last updated, created by (AI/author). Bulk actions: make canon, delete inferred, re-extract.
- **Context Inspector ("Why this suggestion?").** For every AI request, the context log stores:
  - role, timestamp, and model
  - the exact assembled prompt, split into labelled sections [1]–[6]
  - token counts
  - the raw output
  - latency

  It is reachable from a card's *Why?*, from the status bar, and via `Ctrl+Shift+L` for the last ghost suggestion. The log is a ring buffer of the last 500 requests, in `.margin/context-log/` (clearable).
- **Export memory:**
  - JSON (full data model)
  - CSV (one file per record type)
  - Markdown (a readable "Story Bible" document)
  - GraphML / JSON graph (relations, for external tools such as Gephi)
- **Plain-file access.** The `bible/` and `memory/` folders are documented in `docs/data-format.md`, so the author or other tools can read them directly.

---

## 10. Insights workspace (reference visualizations)

Insights is a workspace tab of views built **only** on the Module SDK. It proves the framework, and every view is a separate module. Each view must:
- update live on `memory.updated` / `stats.updated`
- click through to source passages or bible entries
- export its data as CSV and the chart as PNG/SVG

| View (v1) | Data | Visual |
|---|---|---|
| **Writing progress** | `stats.sessions`, provenance | Words/day bar chart, streak heatmap, cumulative line vs goal, your-words vs AI-words split |
| **Character presence** | `mentions.matrix` | Heatmap: entities × chapters/scenes. Sortable, filterable by type |
| **Relationship graph** | `relations.graph` | Cytoscape network. Edge colour by type, node size by mention count. Filters by type and chapter range ("as of chapter N") |
| **Timeline** | `timeline.events`, scene cards | Dual lanes: story order vs narrative order. POV colour bands |
| **Structure overview** | binder + scene cards + metrics | Stacked bars of words per chapter by POV. Optional metric line (e.g. tension) |
| **Plot threads** | `threads.list` | Swimlanes from introduced → touched → resolved. Open threads highlighted |

**Future modules** (designed for, not built in v1):
- location map board
- emotional arc chart
- Sankey of character co-occurrence
- non-fiction argument map
- editorial-feedback AI task
- manuscript "health" dashboard

---

## 11. Editor, project management and quality of life

QoL is the top priority. Everything in this section is in v1 and starts in M1.

### 11.1 Project structure
- **Binder tree:**
  - Book → Parts → Chapters → Scenes
  - Front matter, Back matter
  - Notes, Research, Trash (never exported)
- Drag-and-drop with multi-select. Split scene at the cursor (`Ctrl+Shift+K`). Merge scenes. Duplicate. Move to trash with **undo** (no confirmation dialogs; Trash is recoverable).
- **Per-document properties:**
  - status (Idea / Draft / Revised / Final, customisable)
  - labels/colours
  - POV
  - word target
  - author synopsis
  - include-in-export
  - AI off
- **Corkboard / Outline view:** author synopsis, with the AI scene summary beneath in muted style, plus status and word counts.

### 11.2 Writing surface
- **Views:** Draft (continuous) · Page (paginated by the active template) · **Split** (two documents side by side, e.g. a bible entry beside a scene) · **Tabs** for open documents
- **Focus mode** (dim all but the current sentence or paragraph) · **Typewriter scrolling** · **Distraction-free full screen** (UI hides until the mouse reaches an edge)
- **Themes:** Light, Sepia, Dark, High-contrast. Editor font, size, line height, and width are independent of the export template.
- **Smart typography:** “ ” ‘ ’ curly quotes, `--` → —, `...` → …, each toggleable. Undoing right after an auto-replacement restores the literal characters.
- **Inline comments** anchored to ranges. Not exported, except optionally as DOCX comments.
- **Selection info** in the status bar: word count of the selection.

### 11.3 Never lose work
- **Autosave** 2 s after typing stops and on blur. Atomic writes (temp → fsync → rename). A **recovery journal** restores unsaved edits after a crash, with a non-modal banner.
- **Version history** (hidden git):
  - auto snapshot at session start and every 15 min of changes
  - **named snapshots**
  - per-document timeline with visual diff and restore (whole document or selection)
  - **"Compare with…"** any snapshot
- **Backups.** Scheduled zips to a chosen folder (e.g. OneDrive). Retention: 10 recent, plus daily for 14 days, plus weekly for 8 weeks. "Back up now" command. Restore wizard.
- **External edits.** A file watcher reloads silently if the document is clean, and shows a non-modal keep/load/diff bar if it is dirty.

### 11.4 Flow and continuity of work
- **Session restore.** Reopens the last project, open tabs, cursor and scroll positions, panel layout, and split state.
- **Where was I?** On open, a subtle line in the status bar shows the last edited scene and "Jump back" (`Ctrl+Alt+←`). Navigation history goes back/forward across documents (`Alt+←/→`).
- **Bookmarks** inside documents (`Ctrl+F2`, next with `F2`).
- **Quick capture** (`Ctrl+Shift+N`). A small inline box sends a thought to the Inbox note without leaving the scene.
- **Scratchpad.** A persistent per-project notes pane, never exported.
- **Writing sprints.** Timer (e.g. 25 min) in the status bar, with a words-written-in-sprint counter. It ends with a quiet summary line, not a dialog.
- **Quick open** (`Ctrl+P`) for documents and bible entries. **Command palette** (`Ctrl+K`). **Fully rebindable keymap**, with a cheat sheet (`Ctrl+/`).

### 11.5 Goals and stats
- Session, daily, and project word targets. Deadline calculator (words/day needed).
- Streak calendar. Words per hour. Time writing (active, not idle).
- **Your words vs AI-accepted words**: visible by default (§7.6).
- All stats are queryable via the SDK and shown in Insights → Writing progress.

### 11.6 Find, replace, rename
- Project-wide find/replace with regex, match case, and whole word. Scopes: document, chapter, manuscript, notes, bible. Preview list with per-hit include/exclude. **One undo step per replace-all**, also recorded as a history snapshot.
- **Rename entity.** Renames a name and its chosen aliases across the manuscript and bible, with preview.

### 11.7 Spelling
- Hunspell-compatible dictionaries: **en-US** (default) and **en-CA**. A per-project personal dictionary.
- Bible names, aliases, and glossary terms are added to the project dictionary automatically.
- Style-sheet spelling rules (e.g. "grey" not "gray") are enforced as gentle underlines.

### 11.8 Other
- **Read aloud** of the selection or scene via Windows voices (for proofreading).
- **Accessibility:** fully keyboard-operable, screen-reader labels, UI scaling, reduced motion respected.
- **Onboarding:** a first-run guide covering creating a project, connecting a model, and the 5 key shortcuts. Dismissible and never shown again.

---

## 12. Book formatting and export

### 12.1 Semantic styles
- **Blocks:**
  - Body
  - First paragraph (automatic, no indent)
  - Chapter title/number, Part title
  - Scene break
  - Block quote, Epigraph (+ attribution)
  - Letter/Document, Text message
  - Verse, Centered
  - H2–H4, lists, figure + caption, simple table
- **Inline:** italic, bold, small caps, footnote, link, no-break.
- Templates decide appearance. The manuscript stays clean.

### 12.2 Templates and preview
- Built-in templates: **Classic**, **Modern**, **Literary**, **Non-fiction**.
- Editable settings:
  - fonts, size, leading, indent
  - justification and hyphenation (en-US)
  - chapter opening style, drop cap or small-caps lead-in
  - scene-break ornament
  - running heads, page numbers
  - recto starts, widow/orphan control
- Trim: a few common sizes (5×8, 5.5×8.5, 6×9 in, A5) plus custom, with sensible mirrored margins. Platform-specific gutter tables and preflight are **deferred**.
- Bundled OFL fonts: EB Garamond, Crimson Pro, Libre Baskerville, Source Serif 4, Literata. System fonts are usable, with an embedding-licence warning.
- **Page view and export preview** render the same Typst output (SVG pages, spreads, click-to-source).

### 12.3 Front and back matter
Built-in layouts for:
- Title page, Copyright, Dedication, Epigraph
- TOC (auto)
- Foreword/Preface, Acknowledgements
- About the Author, Also By

### 12.4 Formats (in build order)
1. **Markdown / plain text.** One file or one per chapter. For TXT, choose how italics appear and the scene-break marker.
2. **DOCX:**
   - **Standard Manuscript Format:**
     - 12 pt Times New Roman or Courier, double-spaced, 1 in margins, 0.5 in indent
     - header "Surname / TITLE / page"
     - first-page contact block and rounded word count
     - chapters start on a new page, one-third down
     - centred `#` scene breaks
   - **Styled DOCX:** semantic styles map to named Word styles. Footnotes become real footnotes.
3. **PDF.** Typst, fully embedded fonts, PDF bookmarks.

### 12.5 Export profiles
Saved profiles set:
- the documents included
- chapter numbering style
- scene separator
- front/back matter
- template
- output path

Exports run in the background. `exporters` extension point (§5.3).

---

## 13. Data and file layout

```
My Novel/                         ← the project (a plain folder)
  project.yaml                    ← title, author, type, locale (en-US|en-CA), binder order, schemaVersion
  manuscript/  <slug>--<id>.md    ← one file per scene/document
  front-matter/  back-matter/  notes/  research/  trash/
  bible/
    characters/ locations/ items/ factions/ events/ threads/ glossary/
    concepts/ claims/ sources/ people/ outline/        (non-fiction)
    style-sheet.md
  memory/                         ← AI-derived but human-readable, versioned
    scenes/<sceneId>.json   chapters/<id>.json   synopsis.json   timeline.json
  comments/<docId>.json           ← comment bodies (anchors are in the text)
  stats/sessions.json             ← words per day, writing sessions
  scratchpad.md   dictionary.txt  ← project scratchpad, personal spelling dictionary
  templates/   exports/profiles.yaml
  .margin/                        ← app-managed caches; safe to delete (rebuilds)
    index.db        (embeddings, mentions, search & stats caches)
    paraids/        (stable paragraph-ID maps)
    provenance/  context-log/  bakeoff/  history/ (git)  recovery/
```

### 13.1 Scene file
```markdown
---
id: 01J8Z3K6Q…
title: The Arrival
status: draft
pov: Mara
labels: [harbor, act-1]
target: 2500
synopsis: Mara reaches Kessel and finds the harbor closed.
include: true
ai: on
---
The harbor bells had stopped ringing by the time *Gull's Wake* came round the point.
```

### 13.2 Markdown dialect (pandoc-compatible where possible)

| Element | Syntax |
|---|---|
| Italic / bold | `*x*` / `**x**` |
| Small caps | `[x]{.sc}` |
| Scene break | line containing only `***` |
| Footnote | `[^1]` + definition |
| Block styles | fenced divs `::: epigraph` … `:::` (`letter`, `verse`, `center`, `textmsg`) |
| Comment anchor | `<!-- c:<id> -->` … `<!-- /c -->`, body stored in `comments/<docId>.json` |

Binder order lives in `project.yaml` as an ordered tree of IDs. Renaming a title updates the frontmatter and slug, never the ID.

---

## 14. Non-functional requirements

| Requirement | Target |
|---|---|
| Keystroke-to-render | < 16 ms always. AI and IO off the UI thread |
| Ghost cancel on keystroke | stream stopped < 30 ms |
| Ghost warm TTFT (default model, target machine) | p50 ≤ 350 ms, p95 ≤ 700 ms |
| Project open (150k words) | < 2 s, without the model |
| App RAM (excl. model servers) | < 350 MB at 150k words |
| Insights view render (150k words) | < 500 ms |
| Data loss on crash | ≤ last 2 s of typing |
| Network | configured local endpoints only. CSP `connect-src` restricted |
| Without the model | every non-AI feature works. One status-bar notice, no dialogs |

---

## 15. Settings overview
- **AI:**
  - master switch
  - ghost text: on/off, debounce, length, opacity, never-push-text
  - idle threshold
  - auto-ideas while panel open
  - draft snippets
  - conflict flagging (off)
  - context-log retention
- **Models:** mode, paths, args, presets, sampling per role, bake-off
- **Editor:** fonts, theme, focus and typewriter modes, smart typography, spelling locale (en-US / en-CA)
- **Project:** type, author info, dictionaries, backups, goals
- **Modules:** enable/disable, per-module settings
- **Shortcuts:** keymap editor

---

## 16. Milestones

Build order puts quality of life and the framework first. Each milestone ends with its acceptance checks passing.

**M1 — Foundation and writing core, including the QoL essentials**
- Tauri + React scaffold, `tauri-specta` bindings
- Project Store (file format §13), binder, editor with semantic styles
- Autosave, recovery journal, git history with diff and restore, backups, session restore
- Themes, focus, typewriter, and distraction-free modes; tabs and split view
- Word counts and goals, command palette, quick open, keymap
- Find/replace, spellcheck (en-US/en-CA), smart typography
- **Story Data Service, event bus, Module SDK, and module registry. Binder and editor panels are built as modules.**
- ✅ A full novel can be written, reorganised, searched, versioned, and safely saved with no AI. Acceptance checks 1, 6, 7.

**M2 — Model integration and bake-off**
- Managed and external connection, capability probe, health and status panel
- AI Scheduler with slots and pre-emption, context log
- Bake-off: tune Gemma 4 26B-A4B (`--n-cpu-moe`, KV quantisation, thinking off). Compare with Bonsai 27B, Nemo 12B, and E4B.
- Speed test done on all six candidates (2026-11-05). ✅ The author chose Ternary Bonsai 9B as the default.

**M3 — Ghost text** ✅ (2026-11-05)
- Triggers, streaming decoration, accept/dismiss/word-accept/cycle, post-processing
- Provenance marks and visible stats
- ✅ Acceptance checks 2–5 and 8.

**M4 — Story Memory and Margin panel** ✅ (2026-11-06)
- Indexer and extraction schema, CPU embeddings, paragraph IDs
- Bible UI with canon/inferred, Memory Inspector, Context Inspector, memory export
- Ideas / Ask / Phrasing tabs
- ✅ "Ask my book" answers with correct citations on a 50k-word test manuscript. Acceptance checks 9, 10 and 11 (see `docs/progress.md`).

**M5 — Insights workspace** ✅ (2026-11-06)
- The six reference views as independent modules
- `memoryExtractors` example (`tension` metric) feeding the Structure view
- ✅ A new view can be added by creating one module folder only. Proven by a sample "Hello view" module in `docs/`. Acceptance results in `docs/progress.md`.

**M6 — Formatting and export**
- Templates, front/back matter, Typst page view and preview
- Markdown/TXT, SMF and styled DOCX, PDF, export profiles

**M7 — Polish**
- Writing sprints, read-aloud, onboarding, accessibility pass
- Performance pass against §14
- Installer (MSI/NSIS), optional update check (no data sent)

---

## 17. Acceptance checks

1. Typing at 120 wpm for 60 s with all features on produces zero dropped or delayed keystrokes. Verified with AI both on and off.
2. A ghost never appears with a selection, in a heading, or within 1.5 s of a dismiss at the same spot.
3. Any character key during ghost streaming stops generation (confirmed in `llama-server` logs).
4. Undo right after `Tab` removes exactly the accepted text.
5. Saved `.md` files contain no ghost text, provenance markup, or AI artefacts.
6. Killing the app mid-sentence loses ≤ 2 s of typing. The recovery banner restores the rest.
7. Every destructive action (trash, replace-all, merge, restore snapshot) is undoable or recoverable without a confirmation dialog.
8. With the model server stopped, all non-AI features work, with one status-bar notice and no dialogs.
9. Editing a canon fact, then writing a contradicting sentence and re-indexing, leaves canon unchanged.
10. Every item shown in any AI output or Insights view can be traced to its source passage in ≤ 2 clicks.
11. Memory export (JSON) round-trips. Rebuilding `.margin/` from scratch reproduces the same entities and relations, apart from the inferred facts the model regenerates.
12. A firewall shows no outbound connections other than 127.0.0.1.

---

## 18. Risks and verification items

| Risk / item | Mitigation |
|---|---|
| Gemma 4 26B-A4B with expert offload may have slow **cold** prompt processing (DDR4, possibly PCIe 3.0) | **Resolved in M2:** it missed the speed limits and was not chosen. The default is Ternary Bonsai 9B, fully on the GPU |
| 16K context per slot | Hierarchical summaries and retrieval. The model never needs the whole book |
| Gemma 4 chat template and thinking switch | **Resolved in M2** (§6.2). Per-preset template config stays, and the probe warns on a mismatch |
| Bonsai needs the PrismML fork | Managed mode with a custom binary path |
| Stable paragraph IDs across external edits | Sidecar maps plus fuzzy re-anchoring by text similarity. Citations degrade to the scene level rather than breaking |
| Extraction quality from a ~4B-active model | Grammar-constrained JSON, small focused prompts per scene, canon protection, re-extract command |
| Typst drop caps and ornaments | Typst packages. Fallback to small-caps lead-in |
| Framework over-engineering | Only the extension points in §5.3. Each must have at least one v1 consumer |
