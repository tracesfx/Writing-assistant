/**
 * Story Data Service client (SPEC §5.1). Wraps the generated IPC bindings,
 * turning `Result` values into resolved values or thrown `StoryError`s.
 */
import {
  commands,
  type BackupConfig,
  type BinderTarget,
  type Comment,
  type DocKind,
  type DocMetaPatch,
  type HitRef,
  type MarginError,
  type NewProject,
  type PmNode,
  type ProjectPatch,
  type SearchQuery,
} from "./bindings";

export class StoryError extends Error {
  readonly kind: MarginError["kind"];
  constructor(err: MarginError) {
    super("message" in err ? err.message : err.kind);
    this.kind = err.kind;
    this.name = "StoryError";
  }
}

const EMPTY_PATCH: DocMetaPatch = {
  title: null,
  kind: null,
  status: null,
  pov: null,
  labels: null,
  target: null,
  synopsis: null,
  include: null,
  ai: null,
};

const EMPTY_PROJECT_PATCH: ProjectPatch = {
  title: null,
  author: null,
  projectType: null,
  locale: null,
  goals: null,
  statuses: null,
  labels: null,
};

type Res<T> = { status: "ok"; data: T } | { status: "error"; error: MarginError };

async function unwrap<T>(p: Promise<Res<T>>): Promise<T> {
  const r = await p;
  if (r.status === "ok") return r.data;
  throw new StoryError(r.error);
}

export const story = {
  project: {
    get: () => unwrap(commands.projectGet()),
    create: (req: NewProject) => unwrap(commands.projectCreate(req)),
    open: (root: string) => unwrap(commands.projectOpen(root)),
    close: () => unwrap(commands.projectClose()),
    /** Fields left out are unchanged. */
    update: (patch: Partial<ProjectPatch>) => unwrap(commands.projectUpdate({ ...EMPTY_PROJECT_PATCH, ...patch })),
  },
  binder: {
    tree: () => unwrap(commands.binderTree()),
    move: (ids: string[], target: BinderTarget) => unwrap(commands.binderMove(ids, target)),
    trash: (ids: string[]) => unwrap(commands.binderTrash(ids)),
    restore: (ids: string[]) => unwrap(commands.binderRestore(ids)),
    /** Copies after the originals; returns the new top-level ids. */
    duplicate: (ids: string[]) => unwrap(commands.binderDuplicate(ids)),
  },
  documents: {
    get: (id: string) => unwrap(commands.documentGet(id)),
    meta: (id: string) => unwrap(commands.documentMeta(id)),
    save: (id: string, content: PmNode, force = false) => unwrap(commands.documentSave(id, content, force)),
    create: (kind: DocKind, title: string, target: BinderTarget) =>
      unwrap(commands.documentCreate(kind, title, target)),
    /** Fields left out are unchanged. */
    updateMeta: (id: string, patch: Partial<DocMetaPatch>) =>
      unwrap(commands.documentUpdateMeta(id, { ...EMPTY_PATCH, ...patch })),
    /** `before` stays in the document; `after` becomes a new one right after it. */
    /** Text on disk, without making it the known revision (for comparisons). */
    peek: (id: string) => unwrap(commands.documentPeek(id)),
    /** Replace the text outside the editor (undo of merge/split); open editors reload. */
    replace: (id: string, content: PmNode) => unwrap(commands.documentReplace(id, content)),
    split: (id: string, before: PmNode, after: PmNode, title = "") =>
      unwrap(commands.documentSplit(id, before, after, title)),
    /** Merge into the first (binder order); the others go to the Trash. */
    merge: (ids: string[]) => unwrap(commands.documentMerge(ids)),
  },
  recovery: {
    write: (id: string, content: PmNode) => unwrap(commands.journalWrite(id, content)),
    list: () => unwrap(commands.journalList()),
    get: (id: string) => unwrap(commands.journalGet(id)),
    discard: (id: string) => unwrap(commands.journalDiscard(id)),
  },
  notes: {
    /** Append a thought to the Inbox note; returns its id. */
    capture: (text: string) => unwrap(commands.capture(text)),
    scratchpad: () => unwrap(commands.scratchpadGet()),
    saveScratchpad: (text: string) => unwrap(commands.scratchpadPut(text)),
  },
  comments: {
    list: (docId: string) => unwrap(commands.commentsList(docId)),
    put: (docId: string, comment: Comment) => unwrap(commands.commentsPut(docId, comment)),
    delete: (docId: string, commentId: string) => unwrap(commands.commentsDelete(docId, commentId)),
  },
  search: {
    text: (query: SearchQuery) => unwrap(commands.search(query)),
    /** Snapshots history first; `snapshotId` undoes it via `history.restore`. */
    replace: (query: SearchQuery, replacement: string, hits: HitRef[]) =>
      unwrap(commands.searchReplace(query, replacement, hits)),
  },
  history: {
    snapshot: (name: string) => unwrap(commands.historySnapshot(name)),
    list: (limit = 500) => unwrap(commands.historyList(limit)),
    versions: (docId: string) => unwrap(commands.historyDocVersions(docId)),
    at: (docId: string, snapshot: string) => unwrap(commands.historyDocAt(docId, snapshot)),
    /** `null` = the current file on disk. */
    diff: (docId: string, from: string | null, to: string | null) => unwrap(commands.historyDiff(docId, from, to)),
    /** Returns the safety snapshot taken first (restore it to undo). */
    restore: (snapshot: string, docIds: string[], label: string) =>
      unwrap(commands.historyRestore(snapshot, docIds, label)),
  },
  backups: {
    status: () => unwrap(commands.backupStatus()),
    setConfig: (config: BackupConfig) => unwrap(commands.backupSetConfig(config)),
    now: () => unwrap(commands.backupNow()),
    /** Unpacks into a new folder; returns its path. */
    restore: (path: string) => unwrap(commands.backupRestore(path)),
  },
  stats: {
    summary: () => unwrap(commands.statsSummary()),
    activity: (seconds: number) => unwrap(commands.statsActivity(seconds)),
    sessions: () => unwrap(commands.statsSessions()),
  },
  spell: {
    check: (texts: string[]) => unwrap(commands.spellCheck(texts)),
    suggest: (word: string) => unwrap(commands.spellSuggest(word)),
    add: (word: string) => unwrap(commands.spellAdd(word)),
  },
  app: {
    session: () => unwrap(commands.appSession()),
    saveUi: (projectId: string, ui: string) => unwrap(commands.appSaveUi(projectId, ui)),
    saveSettings: (settings: string) => unwrap(commands.appSaveSettings(settings)),
    forgetRecent: (root: string) => unwrap(commands.appForgetRecent(root)),
    beforeClose: () => unwrap(commands.appBeforeClose()),
  },
};

export type Story = typeof story;
