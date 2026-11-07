# CLAUDE.md — Margin (local AI writing studio)

Margin is a Windows desktop app for writing books, with a quiet local AI assistant. **`SPEC.md` is the source of truth.** Read the sections relevant to your task before writing code. If the implementation must deviate from the spec, update `SPEC.md` (decisions log §1.2, or the relevant section) in the same change, and tell the user.

## Status
- Phase: **M5 built** (2026-11-06): the Insights workspace (six views, one module each) and the `tension` extractor, on top of M4's Story Memory and Margin panel. M6 (formatting and export) next. See `docs/progress.md`.
- Track milestone progress in `docs/progress.md`, a checklist per milestone. Create it on first run.
- After scaffolding, replace the "Commands" section below with the real, verified commands.

## Non-negotiables (from SPEC §2) — never violate
1. The AI **never** modifies manuscript text except through an explicit user accept action. Ghost text is a ProseMirror **decoration only**. It is never a document node, never in undo history, never saved.
2. **Never block typing.** No AI, IO, or indexing work on the UI thread. Every keystroke cancels in-flight ghost text.
3. **No network** except configured `127.0.0.1` model endpoints. No telemetry, no analytics, no CDN-loaded assets. Bundle fonts and libraries.
4. **Canon is sacred.** AI writes may only create or update `inferred` memory records. Canon changes only via author actions. Enforce this in the Rust Story Data Service, not just the UI.
5. **No modal dialogs or confirmation pop-ups** for routine actions. Make actions undoable or recoverable instead (Trash, history snapshots).
6. **Atomic writes** for every project file (temp → fsync → rename). Never leave a half-written file.
7. **Platform first.** Features are modules using the Story Data Service, event bus, and Module SDK (SPEC §5). Don't reach into another module's internals or bypass the service to touch files.
8. Everything the model tracks must be **inspectable and traceable** to a source passage (SPEC §9.6). New memory fields must appear in the Memory Inspector and the exports.

## Tech stack (SPEC §4.1)
- **Tauri 2** (Rust backend) + **React + TypeScript (strict)** frontend, Vite
- Editor: **TipTap / ProseMirror**
- IPC: Tauri commands/events with **`tauri-specta`**-generated TS bindings. Never hand-write IPC types.
- Storage: Markdown + YAML frontmatter (manuscript, bible), JSON (`memory/`), SQLite + `sqlite-vec` (rebuildable caches in `.margin/`)
- History: `git2`. Spellcheck: Hunspell-compatible (`spellbook`). Export: `docx-rs`, Typst crate.
- Charts: Apache ECharts. Graphs: Cytoscape.js.
- Inference: `llama-server` over HTTP (native `/completion` preferred, OpenAI-compatible fallback). Embeddings: a second `llama-server --embedding` on CPU.

## Planned repository layout
```
src-tauri/                 Rust backend
  src/
    project/               Project Store: file format, atomic IO, watcher
    history/               git2 snapshots/diff/restore
    story/                 Story Data Service (queries + validated commands), schema/migrations
    events/                event bus + typed events
    ai/                    scheduler, llama client, presets, context builder, context log
    memory/                indexer, extraction schemas, retrieval, embeddings
    stats/                 sessions, provenance, time series
    spell/                 spellcheck
    export/                exporter trait + markdown/docx/typst impls
src/                       Frontend
  app/                     shell: layout, palette, keymap, status bar, settings
  editor/                  TipTap extensions (semantic styles, ghost text, para IDs, provenance)
  sdk/                     @margin/sdk: story client, events, extension registry types
  modules/<name>/          first-party modules, each with module.ts manifest
docs/                      data-format.md, sdk.md, progress.md, hello-view example
fixtures/                  sample projects (public-domain text) for tests and bake-off
```

## Commands
Verified 2026-11-04. Rust is at `%USERPROFILE%\.cargo\bin`; Git Bash may need `export PATH="/c/Users/Joe/.cargo/bin:$PATH"`.
- Dev: `npm run tauri dev` (also regenerates `src/sdk/bindings.ts`)
- Frontend tests: `npm test` (Vitest)
- Type-check / lint: `npm run typecheck`, `npm run lint`
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Rust lint / format: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `cargo fmt --manifest-path src-tauri/Cargo.toml`
- Regenerate IPC bindings without launching: `cargo test --manifest-path src-tauri/Cargo.toml --lib export_bindings`
- Drive the running app (screenshots, clicks): launch with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, then `playwright-core`'s `chromium.connectOverCDP("http://127.0.0.1:9222")`
- Real-model tests are opt-in: `MARGIN_LIVE_MODEL=1` (optional `MARGIN_LIVE_URL`, default `http://127.0.0.1:8080`) with a server already running, e.g. `MARGIN_LIVE_MODEL=1 cargo test --manifest-path src-tauri/Cargo.toml --lib live_ -- --nocapture`. The M4 acceptance run (`live_ask_my_book_on_a_50k_word_manuscript`) reads `fixtures/manuscript50k/` and takes about 20 minutes
- Editing Rust while `npm run tauri dev` runs restarts the app and stops its managed model server. Closing the window stops it too (`RunEvent::Exit`).
- Windows note: `src-tauri/build.rs` embeds `windows-app-manifest.xml` into every linked target, because test binaries otherwise fail with `STATUS_ENTRYPOINT_NOT_FOUND`

## Conventions
- **Rust:**
  - `thiserror` for error types, `anyhow` only at the binary edges
  - no `unwrap`/`expect` outside tests
  - async via `tokio`
  - long work on background tasks, never on the Tauri main thread
- **TypeScript:** `strict`, no `any` without a comment explaining why. React function components. State in small stores (Zustand) per module.
- **IDs:** ULIDs for documents, entities, and records. Never derive identity from titles or filenames.
- **Schemas:** every persisted file/table has `schemaVersion`. Add a migration plus a test when changing one.
- **Data formats:** documented in `docs/data-format.md`. Update it whenever a file format or record type changes.
- **Locale:** English only. Spelling `en-US` default, `en-CA` selectable. UI copy is US English.
- Keep dependencies lean. Justify any new crate or npm package in the PR/commit message.

## Testing expectations
- **Unit tests** for:
  - the Story Data Service canon rules
  - the Markdown dialect round-trip (parse → serialize must be lossless)
  - atomic writes and recovery
  - migrations
  - the context builder's token budgets
  - ghost-text post-processing
- **Editor tests** (Vitest + ProseMirror test utils) for ghost-text decoration invariants: not in the doc, not in undo, never saved (SPEC §17 checks 2–5).
- **AI tests must not require a GPU.** Use a mock llama-server (recorded/streamed fixtures, with artificial latency and cancellation). Real-model tests are opt-in (`MARGIN_LIVE_MODEL=1`).
- When a milestone completes, run its SPEC §17 acceptance checks and record the results in `docs/progress.md`.

## Environment notes
- Windows 11, PowerShell is the primary shell. The project path contains spaces (`D:\Claude code\writing assistant`), so always quote paths.
- Target hardware: RTX 3060 12 GB, Ryzen 5 3600X, 32 GB RAM.
- Default model per SPEC §6.2: **Ternary Bonsai 9B (`PQ2_0`)**, chosen by the author after the M2 bake-off; the built-in presets are the Bonsai family only. Binary (PrismML build b12128, also loads Gemma 4) and models are under `D:/MODELS` (`bonsai/`; `gemma4/` and `nemo/` are no longer used and were left on disk); paths are in the built-in presets (`src-tauri/src/ai/presets.rs`) and editable in Settings → Models. Margin runs its own managed server on port 8080. The user's game harness owns :8081 and other apps may run their own llama-server: never kill llama-server by name, and don't download multi-GB models without asking.
- The git repo is initialised (remote: `origin` on GitHub). Commit and push only when the user asks.

## Workflow
1. Before each task, re-read the relevant SPEC sections and `docs/progress.md`.
2. Work milestone by milestone, in SPEC §16 order. Don't start AI features before the M1 foundation (Story Data Service, event bus, Module SDK) exists.
3. Prefer small vertical slices that run end to end in the app. Verify UI changes by actually running the app, not just the tests.
4. Ask the user before decisions the spec leaves open that are hard to reverse: file formats, public SDK shape, new extension points.
