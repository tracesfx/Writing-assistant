# Margin data format

A Margin project is a plain folder. Everything the author owns is Markdown, YAML, or JSON and is readable without the app. `.margin/` holds app-managed state and caches; deleting it loses no writing (except unsaved crash-recovery entries).

Every persisted file carries a `schemaVersion`. Forward migrations run when a project opens; a backup is taken first. A file whose `schemaVersion` is newer than the app understands is not opened.

```
My Novel/
  project.yaml              project metadata + binder ID tree
  manuscript/               parts, chapters, scenes (one .md file each)
  front-matter/  back-matter/  notes/  research/
  trash/                    trashed nodes (recoverable)
  comments/<docId>.json     comment bodies (anchors live in the document text)
  stats/sessions.json       words written per day, writing sessions
  scratchpad.md             the project scratchpad (never exported)
  dictionary.txt            personal spelling dictionary, one word per line
  .margin/
    recovery/<docId>.json   unsaved-edit journal (crash recovery)
    history/                hidden git repository of snapshots
```

Author data (comments, stats, scratchpad, dictionary) lives in plain project files, so it travels with the folder, is backed up, and appears in history. `.margin/` holds only app-managed state that can be rebuilt or is about the app itself.

Folders for the Story Bible (`bible/`), AI memory (`memory/`), and the remaining `.margin/` caches (SPEC §13) are added by later milestones and documented here then.

## `project.yaml` (schemaVersion 1)

```yaml
schemaVersion: 1
id: 01JC...               # ULID, never changes
title: My Novel
author: Jane Doe
type: fiction             # fiction | non-fiction | both
locale: en-US             # en-US | en-CA
statuses: [idea, draft, revised, final]
labels:
- name: harbor
  color: '#2e86ab'
goals:                    # all optional
  projectWords: 90000
  dailyWords: 1000
  sessionWords: 500
  deadline: 2027-06-01
binder:
  manuscript:             # ordered tree of ids; children optional
  - id: 01JC...P          # a part
    children:
    - id: 01JC...C        # a chapter
      children:
      - id: 01JC...S      # a scene
  frontMatter: []
  backMatter: []
  notes:
  - id: 01JC...N
  research: []
  trash:                  # newest first
  - id: 01JC...X
    children: []          # the subtree trashed with it
    from: { section: manuscript, parent: 01JC...C, index: 2 }
    trashedAt: 2026-11-04T10:00:00Z
```

- The binder tree holds **ids only**. Titles and properties live in each node's file.
- Unknown keys are preserved when Margin rewrites the file.
- On open, the binder is reconciled with the files on disk:
  - a file not in the tree is appended to the root of the section whose folder it is in (or to the Trash);
  - a tree entry whose file is gone is dropped, and its children move up a level;
  - a file that cannot be read keeps its place in the tree (hidden) until it is fixed, and is reported in the status bar.

## Document files (schemaVersion 1)

Every binder node (part, chapter, scene, folder, note) is one file named `<slug>--<id>.md` in its section's folder. The slug is derived from the title and changes when the title changes; the **id never changes** and is the only identity. Files without the `--<id>` suffix are accepted.

```markdown
---
schemaVersion: 1
id: 01J8Z3K6Q0000000000000000S
kind: scene               # part | chapter | scene | folder | note
title: The Arrival
status: draft             # optional; one of project.yaml statuses
pov: Mara                 # optional
labels: [harbor, act-1]   # optional
target: 2500              # optional word target
synopsis: Mara reaches Kessel and finds the harbor closed.   # optional
include: true             # include in export
ai: on                    # on | off (AI features for this document)
---
The harbor bells had stopped ringing by the time *Gull's Wake* came round the point.
```

- Unknown frontmatter keys are preserved.
- A Markdown file dropped into a section folder without frontmatter gets an id and `kind` (scene in `manuscript/`, note elsewhere) the next time the project opens. Its body is not changed.
- If two files share an id (e.g. a file copied in Explorer), the canonically named one keeps it and the copy gets a new id.

## Markdown dialect

Pandoc-compatible where possible. Margin writes a **canonical form**: reading a file Margin wrote and writing it again produces the same bytes.

| Element | Syntax |
|---|---|
| Paragraph | one line of text; paragraphs separated by a blank line |
| Hard line break | `\` at the end of the line |
| Italic / bold | `*x*` / `**x**` (`_x_`, `__x__` are read too) |
| Small caps | `[x]{.sc}` |
| No break | `[x]{.nobreak}` |
| Link | `[x](url)` or `[x](<url with spaces>)` |
| Scene break | a line containing only `***` (`* * *`, `---`, `___` are read too) |
| Headings | `#` to `######` |
| Block quote | `> ` |
| Lists | `- item` and `1. item`; nested by indentation. Loose lists (blank lines between items) stay loose |
| Block styles | fenced divs `::: epigraph`, `attribution`, `letter`, `verse`, `center`, `textmsg`, closed by `:::`. They may nest |
| Footnote | `[^1]` in the text, with the definition `[^1]: text` after the paragraphs |
| Comment anchor | `<!-- c:<commentId> -->text<!-- /c -->` around the commented text |

Escaping: `\ * _ [ ] ` < ` are always escaped in text, and characters that would start a block at the beginning of a line (`#`, `>`, `-`, `+`, `1.`, `:::`, …) are escaped there.

Edge cases of the canonical form:

- Empty paragraphs are not stored (Markdown cannot express them).
- Leading and trailing spaces of a paragraph or line are not stored.
- Whitespace at the edge of italic or bold is moved outside it (`*word *` → `*word* `).
- Where CommonMark `*` delimiters cannot express the formatting exactly (rare overlaps of emphasis with small caps, links, or punctuation inside a word), that paragraph's emphasis is written as `[x]{.em}` / `[x]{.strong}`. Margin reads these back as italic and bold, so nothing is lost.
- Where two emphasis spans overlap at a space, that space may lose its (invisible) emphasis.

Footnotes:

- Margin numbers footnotes `1, 2, 3…` in order of appearance and writes every definition at the end of the document. Labels read from other tools (`[^note-a]`) are accepted and renumbered.
- A footnote holds one paragraph of inline text (italic, bold, small caps, links). It cannot contain another footnote.
- A reference with no definition is kept as literal text `[^x]`; a definition nothing refers to is kept as a paragraph.

Comment anchors are HTML comments, so other Markdown tools ignore them. The comment's body, state and dates are in `comments/<docId>.json`. Splitting, merging or duplicating documents moves (or copies) the records with their anchors.

## `.margin/recovery/<docId>.json` (schemaVersion 1)

Written at most every 750 ms while a document has unsaved edits, and deleted by the next successful save. If one is left over at startup and differs from the file, Margin offers to restore it with a non-modal banner.

```json
{
  "schemaVersion": 1,
  "docId": "01J8...",
  "title": "The Arrival",
  "savedAt": "2026-11-04T10:00:03Z",
  "baseRevision": "a1b2c3…",
  "content": { "type": "doc", "content": [ … ProseMirror JSON … ] }
}
```

## `comments/<docId>.json` (schemaVersion 1)

```json
{
  "schemaVersion": 1,
  "comments": [
    { "id": "01M6…", "body": "Is this needed?", "resolved": false,
      "createdAt": "2026-11-05T02:27:07Z", "updatedAt": "2026-11-05T02:27:09Z" }
  ]
}
```

A record whose anchor is gone from the text is kept until the author deletes it ("Show text" then reports that the text is gone).

## `stats/sessions.json` (schemaVersion 1)

```json
{
  "schemaVersion": 1,
  "days": { "2026-11-04": { "words": 1240, "activeSeconds": 3600 } },
  "sessions": [ { "start": "2026-11-04T09:00:00-05:00", "end": "2026-11-04T10:12:00-05:00", "words": 812, "activeSeconds": 2700 } ]
}
```

- `words` is the net change in manuscript word count from the author's edits (deleting counts negative). Notes and research do not count. Moving, duplicating, splitting or merging documents does not count.
- `activeSeconds` is writing time, counted in 15-second steps in which the author typed in the editor.
- The newest 1,000 sessions are kept.

## `scratchpad.md` and `dictionary.txt`

Plain text. The scratchpad is free-form Markdown that is never exported. The dictionary is one word per line, added to the bundled US or Canadian dictionary (SCOWL, via wooorm `dictionary-en` / `dictionary-en-ca`) chosen by `locale` in `project.yaml`.

## `.margin/history/`

A bare git repository (one branch, `main`) written with libgit2. Each commit is a snapshot of every project file outside `.margin/`. The commit message is the snapshot's label, followed by a trailer `Margin-Kind: session | auto | named | safety`. Margin snapshots when a project opens, every 15 minutes of changes, before replace-all and restore (`safety`), and when the author names one. Deleting the folder loses history but no writing.

## Backups (outside the project)

Zip archives of the project folder without `.margin/`, named `<project title> <YYYY-MM-DD HHMMSS>.zip`, in `<backup folder>/<project title>/`. The default backup folder is `DocumentsMargin Backups`. Backups run hourly when something changed, and when Margin closes. Kept: the 10 newest, the newest of each day for 14 days, and the newest of each week for 8 weeks. Restoring unzips into a new folder next to the project; it never overwrites it.

## App state (outside the project)

`%APPDATA%/app.margin.studio/app-state.json` holds the recent-projects list, app settings (theme, editor, keymap, disabled modules), the backup configuration (`enabled`, `dir`, `intervalMinutes`), and, per project id, an opaque UI-session blob (open tabs, split, sidebars, module UI state such as cursor positions and bookmarks) used for session restore.

## `ai-config.json` (app config folder, schemaVersion 1)

Next to `app-state.json`. Model connection and presets: `enabled` (the master switch), `mode` (`managed` | `external`), `port` (managed, default 8080), `externalUrl` (must be loopback), `presetId`, `autoStart`, `idleSeconds` (idle work waits this long after the last keystroke), `contextLogLimit`, `presetRevision` and `presets`. A preset holds `template` (`chatMl` | `gemma` | `mistral`), `thinking` (`none` | `prefill` | `templateKwarg`), `stop`, `contextSize` (total, `-c`), `parallel` (slots), `binary`, `model`, `args`, optional per-role `sampling`. Built-in presets are merged in by `id`. When `presetRevision` is older than the app's, the built-ins' template, thinking switch, stop tokens, name and notes are refreshed; the author's paths, arguments, context size, slots and sampling are kept.

## `.margin/context-log/<ulid>.json` (schemaVersion 1)

One file per AI request, the newest last (ULIDs sort by time). A ring buffer: only the newest `contextLogLimit` (default 500) are kept. Fields: `id`, `timestamp`, `role` (`ghost` | `ideas` | `ask` | `extraction` | `test`), `priority` (`P0`–`P2`), `slot`, `model`, `sections` (the prompt split into labelled parts `[1]`–`[6]` of SPEC §7.4, each with `label`, `text`, `tokens`), `rawPrompt` (the exact string sent to the server), `rawOutput`, `outcome` (`completed` | `cancelled` | `failed`), `error`, and `timings` (`ttftMs`, `totalMs`, `promptTokens`, `cachedTokens`, `outputTokens`, `decodeTps`). Bake-off requests are not logged here; their prompts and outputs are in the run file.

## `.margin/bakeoff/<ulid>.json` (schemaVersion 1)

One bake-off run (without an open project: `<app config folder>/bakeoff/`). `runId`, `startedAt`, `source` (`sample` | `manuscript`), `finished`, `candidates` and `questions`. Each candidate has `speed` (VRAM, load time, cold and warm first-token times, decode speed, sentence time, the `--n-cpu-moe` used, the arguments, template warnings, and which go/no-go limits were missed) and `samples` (ten continuations). Each question has the text before the cut and its `options` shuffled as A, B, C…, each tagged with the `presetId` that wrote it, plus the author's `answer`. The file is not hidden from the author, but the app shows model names only after every question is answered.

## Writes

Every file is written atomically: write a temp file `.<name>.<ulid>.margin-tmp` in the same folder, fsync it, then rename it over the target. A crash leaves either the old or the new file. Leftover temp files are deleted when the project opens.
