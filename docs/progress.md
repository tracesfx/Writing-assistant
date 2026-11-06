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
- Quality is what separates the passing models, and that is the author's judgment: the blind test (10 questions, A–F labels) is in Settings → Bake-off → "Earlier runs". The active model is the SPEC default (Gemma 4 26B-A4B, tuned arguments saved) until the author picks.

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
- [ ] **Default model confirmed or replaced** — speed done; waits for the blind vote (see above).
- [x] **Without the model server running, everything non-AI works** (SPEC §17 check 8, early): the app runs with no server, the status bar shows "AI: Model not running", no dialog appears.
- [x] **Keystroke stops generation, seen on a real server** (SPEC §17 check 3, at scheduler level; the editor side is M3): 0.3 ms to `Cancelled`, `/slots` idle within 1 s.
- [x] **No network except 127.0.0.1** (SPEC §17 check 12, point in time): no non-loopback sockets from `margin.exe` or its WebView2 processes; the client rejects non-loopback endpoints (unit test).

### Tests
- Rust: 106 pass (44 new in `ai::`), clippy and fmt clean. Vitest 29 pass, typecheck and ESLint clean.
- Live (opt-in `MARGIN_LIVE_MODEL=1`): a keystroke ended a real generation in 0.3 ms (`Finished(Cancelled)`), and `/slots` showed slot 0 idle within 1 s. This is SPEC §17 check 3, confirmed against a real server.
