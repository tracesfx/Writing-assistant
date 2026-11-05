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

## M2 — next
Before starting, ask the user for: the `llama-server` binary path, model file paths, and the endpoint/API shape of their Ternary Bonsai harness (CLAUDE.md).
