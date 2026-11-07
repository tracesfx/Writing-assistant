# Progress

Checklist per milestone (SPEC §16). Acceptance check results are recorded at the end of each milestone.

## M1 — Foundation and writing core, including the QoL essentials ✅ complete (2026-11-04)

### Slice 1 — Scaffold and platform foundation ✅ (2026-11-04)
- [x] Toolchain: Rust 1.99 stable-msvc, VS 2022 Build Tools (MSVC 14.44), Windows SDK 10.0.26100
- [x] git repo initialised, `.gitignore`
- [x] Tauri 2 + React 19 + TS (strict) + Vite scaffold
- [x] `tauri-specta` bindings generated automatically on dev builds → `src/sdk/bindings.ts`
- [x] Project Store: `project.yaml` + per-node `.md` files with YAML frontmatter; reconciliation with files added, removed, copied, or unreadable outside Margin
- [x] Atomic writes (temp → fsync → rename, retry on sharing violations) + crash-leftover cleanup + tests
- [x] Markdown dialect (§13.2) parse/serialize in Rust; canonical round-trip tests, plus a randomized test (3,000 docs in CI; 200,000 across 8 seeds checked once)
- [x] Story Data Service: typed queries + validated commands (project, binder, documents, recovery)
- [x] Event bus: typed, versioned domain events (Rust → frontend, single `marginEvent` channel)
- [x] Module SDK (`@margin/sdk`): story client, events, workbench, lifecycle, extension registry types (`docs/sdk.md`)
- [x] Module registry + error boundaries; Binder (`sidebarPanels`) and Editor (`workspaceViews`) are modules
- [x] Editor: TipTap with semantic styles (body, headings, scene break, block quote, fenced-div styles, lists, italic/bold/small caps/link/no-break)
- [x] Autosave (2 s idle, blur, every 30 s while typing), recovery journal (≤ 750 ms) + non-modal restore banner (undoable discard)
- [x] Session restore: last project, open tabs, split, sidebar width

### Slice 2 — Binder and project management ✅
- [x] Binder tree: create (Ctrl+N scene, Ctrl+Alt+N chapter), inline rename (F2 / double-click), keyboard navigation, multi-select
- [x] Drag-and-drop move (before/after/inside, multi-select), undoable. Verified in the app (2026-11-04)
- [x] Trash with undo notice (no confirmation), Restore from Trash
- [x] Duplicate (Ctrl+D), with undo; comments are copied
- [x] Split scene at cursor (`Ctrl+Shift+K`), merge scenes (`Ctrl+Shift+M`), both undoable; comments follow their text
- [x] Per-document properties panel (status, labels, POV, target, synopsis, include, AI off)
- [x] Undo for binder moves
- [x] Corkboard / Outline view (editable synopses)
- [x] Tabs of trashed documents: decided — they close (the Trash is for recovery, not editing)

### Slice 3 — History and safety ✅
- [x] git2 hidden repo in `.margin/history/`: auto snapshot at session start + every 15 min of changes
- [x] Named snapshots (Ctrl+Alt+S), per-document timeline (Ctrl+Alt+H), word diff, restore (undoable via safety snapshot), insert old text at the cursor, compare with any other version
- [x] Backups: scheduled zips, retention policy, "Back up now", restore as a copy
- [x] File watcher: silent reload when clean; keep / load / compare bar when dirty
- [x] "Where was I?" + back/forward navigation, bookmarks (Ctrl+F2, F2); cursor/scroll restore

### Slice 4 — Writing surface ✅
- [x] Themes: Light, Sepia, Dark, High contrast, Match Windows; editor font/size/line-height/width; interface scale
- [x] Focus mode (F9: sentence / paragraph), typewriter scrolling (Shift+F9), distraction-free full screen (F11)
- [x] Tabs; split view (Ctrl+Alt+\)
- [x] Split view: same document in both panes shares one model
- [x] Smart typography (quotes, em dash, ellipsis; undo restores literal)
- [x] Selection word count in status bar
- [x] Footnotes (Ctrl+Alt+D), inline comments (Ctrl+Alt+M)
- [x] Quick capture (`Ctrl+Shift+N`), scratchpad

### Slice 5 — Navigation, stats, search, spelling ✅
- [x] Command palette (`Ctrl+K`), quick open (`Ctrl+P`), rebindable keymap + cheat sheet (`Ctrl+/`)
- [x] Session/daily/project goals, deadline calculator, stats service and view (Ctrl+Alt+G)
- [x] Project-wide find/replace (regex, case, whole word, scopes, preview, per-match include, one undo step + snapshot)
- [x] Spellcheck en-US / en-CA (bundled SCOWL dictionaries), personal dictionary, suggestions menu
- [x] Settings UI incl. module enable/disable

### Verification pass (2026-11-04)
Every item above was exercised in the running app over CDP (Playwright driving WebView2), with screenshots checked in all five themes. Bugs found and fixed in this pass:
- **Writing stats counted structural edits.** Splitting a scene counted the moved words as deleted (Today fell from 64 to 25; the session showed −34), and merging counted them as written. Binder changes now re-baseline the counts, and split/merge publish their binder change first. Regression test: `stats::split_duplicate_and_merge_are_not_writing`.
- **Comments were orphaned by split/merge.** The anchor moved with the text, but the comment body stayed in the old document's `comments/` file. Split, merge and duplicate now move (or copy) comment records with their anchors, and the Comments panel reloads when a document's content is replaced. Test: `comments_follow_their_text_through_split_duplicate_and_merge`.
- **Undoing a binder move could land one slot off.** The backend reads `index` as a drop position before removal; undo now aims accordingly. Verified in the app.
- **Goals form lost typed values.** A save from one field re-synced the whole form and wiped the field being typed. The form now keeps drafts per field and merges each save into the latest stored goals.
- **Bookmark positions went stale after edits.** Positions moved by typing were never persisted, so after a restart a bookmark could point at the wrong paragraph. Shifts are now saved after a 1.5 s pause. Editor test added.
- Search panel overflowed the sidebar horizontally; toolbar buttons wrapped badly in narrow panes; toggle notices (F9) stacked up instead of replacing each other (new optional `key` on notices); native scrollbars were heavy (now thin and theme-coloured).
- 10 lint errors (React hooks: state synced in effects, `Date.now()` in render) fixed.

New tests: `src/editor/plugins.test.ts` (typography, focus mode, bookmarks, spellcheck: decorations stay out of the document and the undo history). Totals: 62 Rust tests, 29 Vitest tests; clippy and ESLint clean.

Known dev-only quirk: a Vite HMR update to SDK or module files re-creates their stores in place, and the running app can look empty until it is reloaded. A full reload or restart restores the session correctly. No effect on builds.

### Acceptance (M1): checks 1, 6, 7 — ✅ passed 2026-11-04
- [x] **1 — 120 wpm for 60 s, zero dropped/delayed keystrokes** (AI off; AI-on re-run in M3). 640 characters typed over CDP in 59.8 s (128 wpm) into a new scene, with spellcheck, autosave, recovery journal and stats running. All 640 characters landed. Keydown → DOM update → next frame: p50 1.6 ms, p99 4.8 ms, max 6.5 ms (budget 16 ms); none over 50 ms.
- [x] **6 — kill mid-sentence loses ≤ 2 s; recovery banner restores the rest.** Typed a sentence, then force-killed `margin.exe` 0.9 s later, before autosave. The sentence was in no file on disk. On relaunch the banner appeared ("Margin closed before saving your latest edits…"), and Restore wrote the full sentence to disk. Nothing lost.
- [x] **7 — every destructive action undoable/recoverable without a confirmation dialog.** Verified in the app: trash (Undo + Restore from Trash), recovery discard (undoable), replace-all (Undo = safety snapshot), merge (Undo), split (Undo), duplicate (Undo), binder move (Undo), restore snapshot (Undo = safety snapshot), comment delete (Undo). The code has no `confirm`/`alert`/dialog-plugin `ask`/`message` calls; the only native dialog is the folder picker.

## Notes and decisions
- 2026-11-04 — User decisions:
  - stats in `stats/sessions.json` (project);
  - comment bodies in `comments/<docId>.json`, `scratchpad.md` and `dictionary.txt` as plain project files, not `.margin/` (SPEC §13.2 to be updated);
  - dictionaries are wooorm `dictionary-en`/`dictionary-en-ca` (SCOWL, licence in `src-tauri/dictionaries/`);
  - backups on by default to `Documents\Margin Backups`, hourly when changed, plus on close.
- New crates: `git2` (history, no default features), `zip` (backups, deflate only), `notify` (watcher), `spellbook` (Hunspell spellcheck), `regex` (find/replace), `similar` (word diff).
- 2026-11-04 — Binder containers (parts, chapters) are `.md` files like scenes, with `kind:` in the frontmatter. `project.yaml` holds only the ID tree (user decision). Recorded in SPEC §1.2.
- 2026-11-04 — The Markdown dialect is implemented once, in Rust (`src-tauri/src/dialect/`). The editor exchanges ProseMirror-shaped JSON with the backend. Find/replace, word counts and exporters reuse the same parser. Recorded in SPEC §1.2.
- 2026-11-04 — New extension point `sidebarPanels` (user decision), with the Binder as its consumer. Recorded in SPEC §5.3.
- 2026-11-04 — Each paragraph is serialized with `*`/`**` and verified by parsing it back. Where CommonMark can't express an overlap, that paragraph's emphasis falls back to pandoc spans `[x]{.em}` / `[x]{.strong}`. Documented in `docs/data-format.md`.
- Dependencies added (justification): `serde_norway` (maintained serde_yaml fork, YAML frontmatter), `sha2` (stable content revisions), `ulid`, `chrono` (timestamps), `tauri-plugin-dialog` (native folder picker), `zustand` (small stores, per CLAUDE.md), `@fontsource-variable/literata` (bundled OFL editor font, no CDN), `playwright-core` (dev only: drives the WebView2 app over CDP; downloads no browser).
- `npm audit` reports 10 moderate advisories, all in dev-only lint tooling (`typescript-eslint` deps); nothing ships.
- 2026-11-04 — Writing stats count only the author's edits: moving, duplicating, splitting and merging documents is not writing (SPEC §1.2).
- 2026-11-04 — SDK 0.2.0 (additive only): settings, uiState, editorHost, `workbench.ask`, right sidebar panels, module `activate()`, notice `key`. Documented in `docs/sdk.md`.

## M2 — Model integration and bake-off (2026-11-05)

### Preparation notes (2026-11-04)

### Environment (from the user, verified by probing)
- **Bonsai runtime** (PrismML llama.cpp fork, build `b12128-d7b289683`, CUDA 12.4): `D:/MODELS/bonsai/runtime/prism-b12128-d7b2896/llama-server.exe`. Also ships `llama-bench`, `llama-tokenize`, `llama-embedding`.
- **Models** in `D:/MODELS/bonsai`: Ternary-Bonsai-2-27B `PTQ1_0` (5.9 GB, currently loaded) and `PQ2_0` (7.2 GB), 9B `PQ2_0` (2.4 GB), 4B `PQ2_0` (1.1 GB). No other LLM or embedding GGUFs on disk (Gemma 4 26B-A4B, Nemo 12B and Gemma 4 E4B are missing).
- **Running llama-server** `http://127.0.0.1:8081`, launched by the harness: `-m …27B-PTQ1_0.gguf -c 32768 -ngl 99 --host 127.0.0.1 --port 8081 --reasoning auto`.
  - 4 slots, each reporting n_ctx 32768: one unified 32K KV cache shared by all slots (not 32K each). fp16 KV, no explicit flash-attention or KV-quant flags. Host-RAM prompt cache is active (entries of 0.7–1 GB get evicted).
  - VRAM: 10.6 / 12.3 GB used (whole GPU), about 1.6 GB free.
  - Endpoints confirmed: `/health`, `/props`, `/slots`, `/tokenize`, `/completion` (raw prompt, `id_slot`, `cache_prompt`), `/v1/chat/completions`, `/v1/models`.
- **Chat template** is ChatML / Qwen3-style (`<|im_start|>…<|im_end|>`). **Thinking is on by default.** Turn it off with `chat_template_kwargs: {enable_thinking: false}` on chat calls, or on raw `/completion` by prefilling `<|im_start|>assistant
<think>

</think>

`. Verified: the prefill gives clean prose with no reasoning tokens. `reasoning_effort` is xhigh / medium / low.
- **LLM Game harness** `http://127.0.0.1:8000` (FastAPI "LLM Game Harness" 0.0.1). REST under `/api/…`, one WebSocket per session at `/ws`, dev frontend on :5173 proxying both. Routers: system, sessions, tree, campaigns, documents, game, models.
  - Provider `local` is OpenAI-compatible with `llama_server = true`: chat on `/v1/chat/completions`, token budgeting through `/tokenize`, structured calls through `response_schema` → llama-server `json_schema`/GBNF.
  - It **owns the 8081 server**: `GET /api/server/local` (state, model, pid, restarts), `…/logs`, `…/models`, `POST …/start|stop|restart`.
  - Margin needs none of the game routers. The only overlap is the GPU and the server lifecycle.

### First measurements (one-off probes, not the bake-off)
| Probe | Result |
|---|---|
| Cold prompt, 64 tokens | 529 ms (121 tok/s) |
| Warm, 4 new tokens after a cached prefix | 110 ms |
| Warm, 10 new tokens after a cached prefix | 399 ms |
| Decode | 36–41 tok/s |

The SPEC target is warm TTFT p50 ≤ 350 ms, p95 ≤ 700 ms. Bonsai's warm cost grows quickly with the number of new tokens since the cached prefix. Ghost text must keep that number small: keep the cached prefix close to the cursor, and keep the slot pinned so other work cannot evict it.

### Plan (SPEC §16 M2), mapped to this environment
1. **Connection layer (Rust `ai/`).** An `InferenceClient` over llama-server-native `/completion` (preferred) with an OpenAI-compatible fallback. A capability probe reads `/props` (template caps, thinking support, slot count), `/slots` and `/tokenize`. Reqwest restricted to 127.0.0.1; streaming via SSE, and cancellation by dropping the stream.
2. **Managed mode.** A supervisor launches llama-server from a per-preset binary and model path with preset args, shows startup progress, and restarts on crash up to 3 times. Bonsai presets point at the PrismML binary; Gemma, Nemo and E4B would need a mainline build (or a check that this fork loads them).
3. **External mode.** Any endpoint, e.g. the harness's 8081 server.
4. **Presets.** Template handling (ChatML for Bonsai), the thinking switch (prefill / `enable_thinking=false`), stops, sampling per role (§6.5), context size.
5. **AI Scheduler.** P0/P1/P2 queue with slot pinning (`id_slot`), pre-emption, pause on keystroke, and a cancel-latency test against a mock server (no GPU in CI).
6. **Context log** (§9.6) and **health/status** (status-bar states, model panel: log tail, VRAM via `nvidia-smi`, slot states, last latency, restart).
7. **Bake-off** (§6.6): speed test (VRAM, cold/warm TTFT, decode, ghost p50/p95 over 30 pauses), then a blind A/B/C/D quality test. Results go to `.margin/bakeoff/`, and "Use this model" writes the preset.
8. **Mock llama-server** for tests: recorded/streamed fixtures with artificial latency and cancellation.

### Decisions (user, 2026-11-04)
1. **GPU sharing → option (a).** The author will not run the harness and Margin's AI at the same time. Margin runs its **own managed llama-server on port 8080** (preset binary + model + args). External mode stays supported (e.g. pointing at the harness's 8081 server), but Margin does **not** integrate with the harness API (no `/api/server/local` start/stop calls).
2. **Bake-off downloads approved.** Download Gemma 4 26B-A4B (Q4), Mistral Nemo 12B (Q5) and Gemma 4 E4B GGUFs into `D:/MODELS`, one subfolder per family (e.g. `D:/MODELS/gemma4`, `D:/MODELS/nemo`). Check the files and licences and record their sources here. First test whether the PrismML build loads them. Fetch a mainline llama.cpp CUDA build only if it does not (and say so).
3. **Bonsai variants join the bake-off:** 27B `PTQ1_0`, 27B `PQ2_0` and 9B `PQ2_0`.


### Build checklist (2026-11-05)
- [x] `ai/client.rs` — `InferenceClient` trait; llama-server native `/completion` (SSE streaming) with an OpenAI-chat fallback; capability probe (`/health`, `/props`, `/v1/models`, `/tokenize`, `/slots`, template check via `/apply-template`); cancellation by dropping the stream; loopback-only URL validation, no TLS, redirects and proxies off
- [x] `ai/supervisor.rs` — managed mode: launch from preset binary + model + args, log ring (400 lines), restart on crash up to 3 times then give up once, stop kills the process, adopts a server already answering on the port
- [x] `ai/scheduler.rs` — P0/P1/P2, slot pinning (ghost 0, others 1; single-slot servers fall back to 0), pre-emption, idle gating, keystroke cancel, kill switch, requeue of paused idle jobs
- [x] `ai/context_log.rs` — ring buffer (500) in `.margin/context-log/`, sections [1]–[6], exact raw prompt, output, timings; clearable
- [x] `ai/presets.rs`, `ai/template.rs` — six built-in presets; ChatML, Gemma 4 and Mistral raw templates; revisioned upgrade of saved presets
- [x] `ai/bakeoff.rs`, `ai/bakeoff_service.rs` — speed test (VRAM, cold 6K TTFT, warm TTFT over 30 pauses, decode, sentence time), go/no-go, `--n-cpu-moe` tuner (binary search on VRAM), 10 blind continuations (A/B/C… shuffled, key hidden until all answered), scoring, "Use this model", results in `.margin/bakeoff/`
- [x] Mock llama-server (`ai/mock.rs`, tests only) with artificial latency and disconnect detection
- [x] Frontend module `ai`: status-bar item (Off / Starting / Ready / Thinking / Paused (typing) / Model not running), Model panel (server, VRAM, slots, last latency, log tail, Try it), context log viewer, Settings → Models and Bake-off, `Ctrl+Shift+Space` kill switch, palette commands
- [x] Docs: `docs/data-format.md` (ai-config, context log, bake-off), `docs/sdk.md` (`ai` client, events; SDK 0.3.0), SPEC §6 updates
- [x] Bake-off run on all six candidates, one run, GPU free (2026-11-05). Blind quality vote: **pending, the author's call**

### Bake-off results (2026-11-05, run `01M6Q1BZJNP4BY77QT22GBSXD5`)
Sample text (project under 3,000 words): the first ~7,000 words of *Pride and Prejudice*. GPU free (desktop only, ~1.5 GB). Cold = a 6K-token prompt on an empty slot. Warm = first token over 30 simulated pauses, 12 words typed each, cached prefix. Go/no-go limits from SPEC §6.6: p50 ≤ 350 ms, p95 ≤ 700 ms, decode ≥ 20 tok/s, sentence ≤ 2 s, headroom ≥ 0.6 GB.

| Model | Warm p50 / p95 | Cold 6K | Decode | Sentence | VRAM used / headroom | Load | Go? |
|---|---|---|---|---|---|---|---|
| Gemma 4 26B-A4B Q4, `--n-cpu-moe 17` | 381 / 849 ms | 18.6 s | 23 tok/s | 1.0 s | 11.1 / 1.2 GB | 11 s | **No** (p50, p95) |
| Mistral Nemo 12B Q5 | 104 / 166 ms | 6.2 s | 25.7 | 0.77 s | 11.0 / 1.3 GB | 29 s | Go |
| Gemma 4 E4B Q4 | 93 / 145 ms | 3.2 s | 58 | 0.25 s | 5.0 / 7.3 GB | 18 s | Go |
| Bonsai 9B PQ2_0 | 121 / 140 ms | 4.3 s | 85.9 | 0.45 s | 4.4 / 7.9 GB | 11 s | Go |
| Bonsai 27B PTQ1_0 | 270 / 322 ms | 12.1 s | 39.2 | 0.54 s | 8.4 / 3.9 GB | 20 s | Go |
| Bonsai 27B PQ2_0 | 266 / 298 ms | 11.8 s | 33.6 | 0.41 s | 9.7 / 2.6 GB | 23 s | Go |

- **Gemma 4 26B-A4B misses the speed limits, and is noisy.** A separate earlier run of the tuner and test gave p50 413 / p95 521 ms and 16 tok/s; this run gave 381 / 849 ms and 23 tok/s. Both runs miss p50; either run misses one of p95 or decode. Cause is plausibly the experts living in system RAM (DDR4): 17 of the layers' experts are on the CPU. The tuner's binary search worked as designed: 40 fit, 20 fit, 10 no, 15 no, 18 fit, 17 fit, 16 no → 17.
- **Bonsai 27B** passes every limit with 2.6–3.9 GB to spare, but its cold prompt (12 s) is the second slowest. Warm numbers varied between runs (PQ2_0 p50 was 268, 484 and 266 ms across three runs), so the ranking inside "Go" is not reliable on speed alone.
- **Nemo 12B** is fast to the first token and has the slowest decode of the passing models (25.7 tok/s) and the least headroom (1.3 GB).
- Quality is what separates the passing models, and that is the author's judgment: the blind test (10 questions, A–F labels) is in Settings → Bake-off → "Earlier runs". **Decision (2026-11-05): the author chose Ternary Bonsai 9B.**

### Findings
- **Gemma 4 template and thinking switch (SPEC §18 risk, resolved).** Verified against `/apply-template` on the PrismML build: format is `<|turn>system
…<turn|>
<|turn>user
…<turn|>
<|turn>model
`, stop `<turn|>`. The template adds `<|think|>` to the system turn only when `enable_thinking` is true, so raw prompts leave it out; chat calls send `enable_thinking: false`. A raw prompt with an assistant prefill gave a clean continuation. Gemma 3's `<start_of_turn>` markers do not apply.
- **The PrismML build (b12128) loads Gemma 4** (E4B tested, then used in the bake-off). No mainline llama.cpp build was needed or downloaded.
- **Stale saved presets.** The first bake-off ran with a Gemma preset saved before the template fix. The probe's template check caught it (E4B warning). Saved presets now carry a revision and are upgraded on load (test: `upgrade_fixes_behaviour_but_keeps_the_authors_paths`).
- **Sentence-time measurement.** The first version used prefixes that did not continue the cached chain, which unfairly punished slow-prompt models (27B PTQ1_0 read 2.8 s). It now keeps typing from where the 30 pauses ended, as a writer would (same model: 0.5 s).
- **Windows reports a refused local connection after about 2 s.** Health polls use a 2 s timeout, so "Model not running" takes up to ~2 s to appear.
- **Another app's llama-server** (port 18080, ~6.8 GB VRAM) was running at the start and gone later. It was left alone. The bake-off numbers below were taken with the GPU free (baseline ~1.5 GB, the desktop).
- Dev note: editing Rust while `npm run tauri dev` runs restarts the app, which stops its managed server (kill_on_drop). Margin does not leave orphans on a normal exit; after a crash the next start adopts a leftover server.

- **M1 bug found and fixed: the window could not be closed.** Tauri's `onCloseRequested` wrapper calls `window.destroy()` after our handler, and the capability file did not grant `core:window:allow-destroy`, so the close handler ran and then the window stayed open ("window.destroy not allowed"). Added the permission. Verified: closing the window now exits the app. `RunEvent::Exit` also stops Margin's managed model server (verified: no `llama-server` left on port 8080 after closing).

### Acceptance (M2)
- [x] **Default model confirmed or replaced** — the author voted and chose **Ternary Bonsai 9B (`PQ2_0`)** as the default (2026-11-05). The Gemma 4 and Mistral Nemo presets were removed (see M3 below).
- [x] **Without the model server running, everything non-AI works** (SPEC §17 check 8, early): the app runs with no server, the status bar shows "AI: Model not running", no dialog appears.
- [x] **Keystroke stops generation, seen on a real server** (SPEC §17 check 3, at scheduler level; the editor side is M3): 0.3 ms to `Cancelled`, `/slots` idle within 1 s.
- [x] **No network except 127.0.0.1** (SPEC §17 check 12, point in time): no non-loopback sockets from `margin.exe` or its WebView2 processes; the client rejects non-loopback endpoints (unit test).

### Tests
- Rust: 106 pass (44 new in `ai::`), clippy and fmt clean. Vitest 29 pass, typecheck and ESLint clean.
- Live (opt-in `MARGIN_LIVE_MODEL=1`): a keystroke ended a real generation in 0.3 ms (`Finished(Cancelled)`), and `/slots` showed slot 0 idle within 1 s. This is SPEC §17 check 3, confirmed against a real server.

## M3 — Ghost text (2026-11-05)

### Decisions (user, 2026-11-05)
- **Default model: Ternary Bonsai 9B (`PQ2_0`).** Remove the Gemma and Mistral models: the three built-in presets are the Bonsai family (9B default, 27B `PTQ1_0`, 27B `PQ2_0`). Preset revision 3 drops the retired presets from saved configs (the author's own custom presets are kept), moves the selection to 9B, and raises the 9B context from 16384 to 32768 (two slots of 16K, since ghost text sends up to ~8K tokens). The Gemma and Mistral prompt templates stay in the code for custom presets. The model files on disk (`D:/MODELS/gemma4`, `D:/MODELS/nemo`) were **not** deleted.

### Build checklist
- [x] `ai/ghost.rs` — context builder in SPEC §7.4 order (role, primer with spelling convention, scene notes with POV and synopsis, earlier scene text, prefilled assistant turn), token budgets (~8K total, primer ≤ 800, notes ≤ 600), whitespace-trimmed prefill so the prefix cache survives typing; sampling by length (word / phrase / sentence) with hotter, looser alternatives
- [x] Post-processing (SPEC §7.5): leading-space fix, repeats of the author's last words, meta-text and template tokens, one line only, trimming to a clean boundary (abbreviation-aware), cut-off last word, en-CA ↔ en-US spelling swaps that skip mid-sentence capitalised words (names), smart typography that follows the author's three settings
- [x] `AiService::ghost_start` / `ghost_cancel` and the `ai.ghost` event: streaming partial text, then done / empty / cancelled / failed; a cancel that overtakes its request still wins; AI-off documents get no request; every request is in the context log
- [x] Editor extension `src/editor/ghost.ts`: widget decoration only; triggers (pause 250–2000 ms, end of a body paragraph, no selection or composition, ≥ 3 words in this or the previous paragraph, no backspace streak, 1.5 s quiet after a dismiss at the same spot, Flow mode); Tab, Ctrl+→, Esc, Alt+] / Alt+[ (lazy, up to three, one automatic retry on a repeat); shrinks while typing matches; clipped to the cursor's line with a faint "…" (never push text); fade-in without motion
- [x] Provenance (SPEC §7.6): `src/editor/provenance.ts` ranges in plugin state mapped through edits (retyping or editing removes the mark), saved to `.margin/provenance/<docId>.json` as text offsets plus SHA-256 once the document is saved, dropped with one notice on a hash mismatch; optional highlight view; status bar "Session N yours · M AI"; Stats view shows project and per-document AI words
- [x] Settings → Writing → Ghost text; commands `editor.ghost.*`, `editor.flow` (Ctrl+Alt+F), `editor.highlightAi`
- [x] Docs: SPEC §1.2, §6.2, §6.3, §7, §16, §18; `docs/data-format.md` (provenance, ai-config revision 3, settings); `docs/sdk.md` (SDK 0.4.0)

### Acceptance (M3): checks 2, 3, 4, 5, 8 — ✅ passed 2026-11-05; check 1 re-run with AI on — ✅
All run in the real app (Playwright over CDP) against Bonsai 9B on :8080, in a throwaway project, with real key events.
- [x] **2 — never with a selection, in a heading, or within 1.5 s of a dismiss at the same spot.** Selection: no suggestion after Shift+Home and a 1.5 s wait (app). Heading, block quote, list, mid-paragraph cursor and fewer than three words: unit tests on `ghostSite`. The 1.5 s rule: unit test with a controllable clock (1,499 ms blocked, 1,500 ms allowed).
- [x] **3 — any character key during generation stops it (confirmed in the server).** With a request running (`/slots` showed slot 0 busy), one key sent slot 0 idle after 5, 6, 45, 60, 62, 75, 169 ms across runs. The job's outcome is `cancelled` in the context log.
- [x] **4 — undo right after Tab removes exactly the accepted text.** In the app, and in a unit test where the Tab follows typing within 200 ms (the accept closes the history group).
- [x] **5 — saved `.md` files contain no ghost text, provenance markup or AI artefacts.** The bodies of every Markdown file in the test project (28 at the time), after accepting suggestions: nothing. Provenance lives in `.margin/provenance/`. Editor unit tests also check the document JSON and HTML while a suggestion shows and after accepting.
- [x] **8 — with the model server stopped, everything non-AI works.** Typing, autosave to disk, no suggestion, no dialog, one status-bar state "AI: Model not running".
- [x] **1 (AI on) — 120 wpm typing, zero dropped or delayed keystrokes.** 640 characters in four bursts typed at 128 wpm with 1.3 s pauses between them (103 wpm over 75 s), suggestions arriving and being cancelled throughout: all 640 landed. Keydown → next frame: p50 2.9 ms, p95 3.6, p99 4.2, max 6.5 ms; none over 50 ms. (With AI off in M1: p50 1.6, p99 4.8.)

Also measured: time to first token 130–395 ms for a suggestion (the first request after a model start is the slow one), and an alternative is complete about 250 ms after its key press; Bonsai 9B gives eight different continuations out of eight on a natural paragraph at every temperature tried.

### Findings (bugs found by running the real app, fixed, with tests)
- **ProseMirror swallows Esc.** Its keydown handler calls `preventDefault` for Enter and Esc, so Esc never reached the app keymap while typing: dismiss now lives in the ghost plugin (`handleKeyDown`). Unit test sends a real Escape key event.
- **TipTap's `contenteditable="false"` rule beat my CSS.** TipTap injects `.ProseMirror [contenteditable="false"] { white-space: normal }`; the widget wrapped over three lines and pushed text. The ghost CSS now has the higher-specificity selector `.ProseMirror.manuscript .ghost-text`.
- **Alt+] alternatives were cancelled about half the time.** IPC commands are separate requests and can overtake each other: the key's `ai_keystroke` could land after the `ai_ghost_start` that followed it and cancel it. A request made within 150 ms of a key press now waits for that key's notification (tests with a late-resolving client).
- **Cold start.** The SDK started following the model's status on the first `isReady()` call, which answered "not running" for the first pause. The editor module now starts following on activation.
- **A suggestion stayed in a background tab.** It is now dropped, and a request in flight cancelled, when the editor loses focus.
- **Empty paragraphs would have broken provenance after a reload.** `textBetween` adds a blank line for every empty paragraph, and the Markdown file cannot hold them, so the text hash differed after a reload. The provenance text now skips empty blocks (tests for a trailing empty paragraph and for ranges surviving a rebuilt document).
- **Duplicate alternatives did nothing visible.** Alternatives are now hotter (+0.3 per step) with a looser cut-off, and a repeat is retried once.
- **Pre-existing, not changed:** the shell command "Leave distraction-free mode" is bound to `"Escape"`, but the keymap normalises the key to `esc` for events and `escape` for bindings, so the two never match (and ProseMirror consumes Esc in the editor anyway). F11 toggles it.

### Not in M3 (by design)
- Bible cards in the prompt (§7.4 [4]) and the Context Inspector shortcut `Ctrl+Shift+L` (§9.6) come with M4. The context log viewer in the Model panel already shows every ghost request's prompt, output and timings.
- The model does not start by itself: Settings → Models → "Start with Margin" (`autoStart`) is off by default, as in M2: start the server from the Model panel, or turn that on.
- `.margin/provenance/` is not backed up or versioned (SPEC §1.2 open question).

### Tests (M3)
- Rust: 136 pass (30 new: ghost prompt and post-processing, provenance storage, ghost requests end to end against the mock server including cancel and "cancel before request", preset migration and its write-back); clippy and fmt clean.
- Vitest: 99 pass, 66 of them new (`ghost.test.ts` 48, `provenance.test.ts` 16, `sha256.test.ts` 2); typecheck and ESLint clean.

## M4 — Story Memory and the Margin panel (2026-11-06)

### Decisions (recorded in SPEC §1.2)
- **Paragraph ids** are a backend sidecar (`memory/paragraphs/<docId>.json`), not editor node attributes: the editor, the Markdown and undo are untouched; ids survive edits by text similarity.
- **No `index.db`.** Entries, cards, timeline and summaries are plain files; mentions are derived from the text; paragraph vectors are a flat cache (`.margin/embeddings.json`) with a brute-force cosine scan. BM25 keyword search always works; embeddings are optional and need a GGUF the author downloads (nothing was downloaded: **no embedding model is on this machine**, so the embedding path is tested against the mock only).
- **Reading a scene**: grammar-constrained JSON, one call per scene (or per ≈4,500-token window of whole paragraphs for long scenes). Closed list of relation kinds; every claim is checked against the scene before it is kept.
- **`Ctrl+Shift+M`** toggles the Margin panel (SPEC §8). The Binder's merge shortcut had the same key (M1); it applies only with two or more binder items selected and wins then. Both stay rebindable.
- **"To note"** appends to a "<scene> — notes" document in the Notes section (scenes have no notes field; the file format is unchanged).
- A context-log entry is named by its request's job id, so "Why?" and `Ctrl+Shift+L` open exactly that request.

### Build checklist
- [x] `memory/types.rs`, `store.rs` — entries as `bible/<type>/<slug>--<id>.md` (YAML + notes), scene cards, timeline, summaries, exclusions. **Canon rules in the Rust store**: `ai_*` commands only create or update `inferred` records and refuse canon; bulk delete refuses canon; deleting an entry is recoverable (`bible/.trash/`)
- [x] `memory/paragraphs.rs` — paragraph id sidecar, reconciled after every save (exact, then by word overlap)
- [x] `memory/extract.rs`, `merge.rs` — prompt (numbered paragraphs, known entries, worked example), JSON schema, lenient reader (repairs answers cut off at the token limit), merge with canon protection, conflict flagging (opt-in), history on changed readings
- [x] `memory/indexer.rs` — idle-only pipeline: dirty by text hash, settle time, nearest-the-cursor first, one scene at a time as a P2 job, retry backoff, chapter and book summaries, `index.progress` / `memory.updated`; commands Re-index scene, Rebuild all memory, Forget this document
- [x] `memory/search.rs` — BM25 + embeddings (second `llama-server --embedding` on the CPU, managed like the main one) fused by rank; vector cache
- [x] `memory/margin.rs` — Ideas (typed, validated, with refs and a dismissed list), Expand, Pin, To note, Ask my book (evidence from facts, relations, scene cards and passages; citations only to evidence that exists; says so when the book has nothing), Phrasing; bible cards for ghost prompts (`[4]`)
- [x] `memory/export.rs` — JSON (and import), CSV per record type, Markdown Story Bible, GraphML
- [x] Frontend: `@margin/sdk` 0.5.0 (`memory`, `openSource`, hooks, `marginTabs` extension point); modules `bible` (Bible panel, entry view, Memory Inspector, status item, Settings → Memory) and `margin` (Margin panel with Ideas / Ask my book / Phrasing); Context Inspector view and `Ctrl+Shift+L`; dotted underline for inferred
- [x] Docs: `docs/data-format.md` (bible, memory, exports), `docs/sdk.md` (0.5.0), SPEC §1.2 and §16

### Findings (bugs found by running it for real, fixed, with tests)
- **llama.cpp writes JSON object keys alphabetically**, whatever order the schema lists. The scene summary came out near the end and was cut off when the answer hit the token limit (empty summaries). The schema's keys now carry letter prefixes (`a_scene`, `b_entities`, …) so the order they come out in is the order we want; a test checks it. The extraction token limit went from 1,800 to 3,000.
- **Fragmentary facts** ("fatigued") and a **made-up relation** ("employer") from the 9B model. Fixed with a worked example in the prompt, a closed list of relation kinds, and merge rules: a fact must be a sentence that names its entry, a relation's paragraph must name both entries, aliases are at most four words.
- **A thread title became an alias of a person** (fuzzy name matching let "Will Mr. Bingley marry…?" match "Mr. Bingley"). Fuzzy matching is now limited to the same type and short names; threads match only exactly. Test added.
- **Inferred threads were pruned as orphans** (no facts, not named in the text). Threads touched by scenes now count as supported.
- **"Forget this document" left entries that only that document supported** (the mention check used all documents). It now counts only documents memory may use.
- `float_roundtrip` enabled in `serde_json`: without it a scene card's `dialogueRatio` came back one digit different after a save and reload.
- Context-log entries were named by a fresh id, so a card could not point at its request; they are now named by the job id (two old tests used non-ULID job ids).
- Scene summaries in the idle pass wrote `{}` in tests until the mock answered summary prompts (test-only).

### Acceptance (M4): checks 9, 10, 11 and the Ask-my-book run — ✅ 2026-11-06
Real model (Ternary Bonsai 9B on :8080), real app, throwaway projects.
- [x] **Reading a 51,840-word, 27-chapter novel** (first 27 chapters of *Pride and Prejudice*, `fixtures/manuscript50k/`): all 27 scenes read in about 1,060–1,080 s (≈ 39 s per chapter, as idle P2 work). Result: 92–95 entries, 218–219 facts, 52–58 relations, 117–119 events, 62–64 threads (two runs).
- [x] **10 — every item traces to its passage.** 275 of 275 sources (facts and relations) resolve to a real paragraph that contains the quote. In the app: a fact's source, an Ask citation and an idea's reference each open the scene with the words selected in one click.
- [x] **9 — canon survives.** A canon fact ("Mr. Darcy is a village schoolteacher", field occupation) was added before the whole book was read: byte-identical afterwards, the entry still canon, no inferred occupation beside it. Also covered deterministically with the mock (edit a canon fact, write a contradicting sentence, re-index, rebuild all).
- [x] **11 — export round-trips; caches rebuild.** JSON export → wipe `bible/` and `memory/` → import gives identical entries, cards and events (test). Deleting `.margin/` and reloading gives the same entries, relations and cards, with nothing to re-read.
- [x] **Ask my book on the 52k-word manuscript:** 4 of 5 questions answered with a relevant cited passage (Bingley's friend → Darcy; Collins; which sister → Jane; why Mrs. Bennet wants to meet Bingley). **One wrong answer**: "Where do the Lucases live?" got "near Cheapside" (the retrieved passage is about someone else); keyword retrieval missed "Lucas Lodge", and an embedding model would probably help. Of 19 citations shown, 9 contained the expected words; the others were loosely related passages or facts. The question the book cannot answer (a pet dog) was refused with no citations.
- [x] **In the running app (CDP):** status bar goes "Memory: reading" → "up to date"; inferred entries are marked with a dotted underline; "✓ make canon" turns one into canon; Ask answers with a clickable citation that selects the passage, and says so when the book does not know; Ideas cards, **Why?** opens the Context Inspector with sections [1]–[6]; Phrasing leaves the selection untouched until Replace selection, and one Ctrl+Z restores it; the ghost prompt now has `[4] Bible cards` and **Ctrl+Shift+L** opens it.
- [x] **1 (memory and AI on) — typing at 120 wpm.** 640 characters at 102 wpm over 75 s with the model on and a scene waiting to be read: all 640 landed; keydown → next frame p50 2.8 ms, p95 3.6, p99 4.3, max 6.8 ms; none over 50 ms. Reading is held while typing (state "working, pending 1").

### Known limits and notes
- **No embedding model on this machine**, so the embedding path (second `llama-server --embedding`) is tested only against the mock; passage search is keyword-only until the author supplies a GGUF (Settings → Memory).
- **Ctrl+Shift+I opens Edge DevTools in `tauri dev`** and swallows the key before the app sees it. DevTools are off in release builds. The command "Get ideas" works from the palette meanwhile, and the key is rebindable.
- The 9B model's readings are useful, not perfect: some facts are loose paraphrases and some relations are wrong. That is why they are *inferred*, sourced, and one click from canon or deletion.
- Not built: the `bibleTypes`, `aiTasks` and `memoryExtractors` extension points are still placeholders (M5 adds the `tension` extractor); the optional "fresh ideas" dot sits on the Ideas tab, not the toggle (ideas are only made automatically while the panel is open).
- `.margin/embeddings.json` and the context log stay out of backups and history, as before.

### Tests (M4)
- Rust: 193 pass (58 new in `memory::`: store and canon rules, paragraph ids, extraction and merge, search, export and import, and end-to-end indexing, Ask, Ideas, Phrasing and ghost cards against the mock model), plus 3 opt-in live tests; clippy and fmt clean.
- Vitest: 101 pass; typecheck and ESLint clean.

## M5 — Insights workspace (2026-11-06)

### Decisions (recorded in SPEC §1.2)
- Insights is a workspace view with its own extension point, `insightsViews`; each of the six views is a module folder (`src/modules/insights-*`). Module folders are found with `import.meta.glob`, so a new view is one new folder. Proven by `docs/hello-view/` and a test that adds it to the registry.
- ECharts (charts) and Cytoscape (graph) are bundled and load on first draw. Shared plumbing (themed `Chart`, `ExportBar`, `useInsightsData`, `chapterGroups`) lives in the SDK because modules may not import each other. SDK 0.6.0.
- `tension` is the first `memoryExtractors` entry: the same extraction call also reports `g_metrics.a_tension` (0–10), stored in the scene card's `metrics`, with `extractors` on the card recording what was asked. On by default (Settings → Memory); a card made without it is read again once it is on.
- Story order: the model gives events only a label, so the Timeline lets the author move an event earlier/later (`event_set_order`); that sets `storyTime.order` and makes the event canon. Shown in the Memory Inspector, `events.csv` and the JSON export.
- Exports (CSV, PNG, SVG) go to a folder picked in the native picker (`insight_export`, plain file names, never overwrites). CSV cells that start with `= + - @` are defused.

### Build checklist
- [x] Writing progress: words/day, streak calendar, growth vs goal and pace, your words vs AI words per chapter
- [x] Character presence: entries × scenes/chapters heatmap; type, search, sort, minimum filters
- [x] Relationship graph: Cytoscape; colour by relation type, size by mentions, type filters, "as of chapter N"
- [x] Timeline: told order vs story order, point-of-view bands, author-set story position
- [x] Structure overview: words per chapter/scene by POV, optional metric line (dialogue, entries, tension)
- [x] Plot threads: swimlanes, open threads highlighted
- [x] `tension` extractor (Rust, schema/prompt/parser/dirty check), Settings toggle
- [x] Docs: SPEC §1.2/§5.3, `docs/sdk.md`, `docs/data-format.md`, `docs/hello-view/`

### Acceptance (M5) — run in the real app over CDP, 2026-11-06
Throwaway project: the first 10 chapters of the 50k fixture (15,538 words), read by Ternary Bonsai 9B (about 40 s a chapter): 38 entries, 37 events, 23 threads, 17 relations, a tension value on every scene. 28 of 28 scripted checks passed.
- [x] **A new view needs one module folder only.** `docs/hello-view/` added to the registry gives a seventh view and its command, with no other edit (test).
- [x] **Every view traces to its passage in ≤ 2 clicks (check 10).** Presence cell → scene with the mention selected (1 click); graph relation → "Read the passage" → words selected (2); timeline event → passage (2); thread dot → scene (1); structure bar → that chapter's scene (1); name → bible entry (1–2).
- [x] **Export.** Each view saved CSV, PNG and SVG; the PNG has the PNG signature, the SVGs contain the chart (the graph SVG has nodes and lines), and the CSV has a header and rows.
- [x] **Live theme.** Switching to the dark theme re-colours open charts without reopening.
- [x] **Tension feeds Structure.** Choosing "Tension" adds a line with values (3, 3, 5, 3, 3, 5, 3, 3, 3, 5).
- [x] **Story order.** Moving an event later changed its Happens number and made it canon.
- [~] **Render < 500 ms at 150k words (§14).** A synthetic project of 155,520 words, 162 scenes, 60 entries and 90 relations, first open of each view after a reload: Writing progress 6 ms (already showing), Character presence 422 ms (the matrix query is 189 ms of it), Relationship graph 271 ms (3.0 s before the layout was cut to 250 iterations), Structure 101 ms. Timeline and Plot threads were timed there only empty (65 and 115 ms): the model had not read that project. On the real 10-chapter data they took about 0.4 s including the keypresses.

### Findings
- **Cose layout took 3 s for 60 nodes**; fixed (fewer iterations, no label-size packing).
- **Native folder pickers cannot be driven over CDP**, and `__TAURI_INTERNALS__` is read-only, so the driver answers through `window.__marginExportDir` (checked in `saveInsightFile`). The charts also keep `__marginChart`, `__marginCy` and `__marginExport` on their element for the driver.
- A y-axis with a 0.5 offset hid the Timeline's lane labels; fixed.
- The 9B model left point of view blank for almost every scene of *Pride and Prejudice* (omniscient narration), so Structure shows one "No point of view" series; the view is right, the data is thin. Tension values are coarse (3 and 5).
- Timeline events all start hollow (no story position): the model gives labels only, which is why the author-placed order exists.

### Tests (M5)
- Rust: 199 pass (6 new: tension schema/prompt/parse, tension end to end and re-read, story order, inspector/CSV, export file names); clippy and fmt clean.
- Vitest: 155 pass (54 new: a model test file per view, module discovery, Hello view, CSV/file names); typecheck and ESLint clean. `npm audit --omit=dev`: 0.
