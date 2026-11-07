/**
 * Story Memory client (SPEC §5.1, §9): the bible, scene cards, mentions,
 * threads, retrieval and the Margin panel's AI. Wraps the generated IPC
 * bindings like `story` does. Reads are open to every module; writes go
 * through the Rust service, which enforces the canon rules (SPEC §9.1).
 */
import { create } from "zustand";

import {
  commands,
  type ExportFormat,
  type Idea,
  type IdeasRequest,
  type ItemRef,
  type MemoryConfig,
  type MemoryStatus,
  type PhrasingRequest,
  type Source,
  type Entity,
  type EntityPatch,
  type ThreadStatus,
} from "./bindings";
import { events } from "./events";
import { unwrap } from "./story";

export const memory = {
  status: () => unwrap(commands.memoryStatus()),
  config: {
    get: () => unwrap(commands.memoryConfigGet()),
    set: (config: MemoryConfig) => unwrap(commands.memoryConfigSet(config)),
  },
  /** Tell memory which scene the author is in: it is read first. */
  focus: (docId: string | null) => unwrap(commands.memoryFocus(docId)),
  reindexScene: (docId: string) => unwrap(commands.memoryReindexScene(docId)),
  rebuildAll: () => unwrap(commands.memoryRebuildAll()),
  /** "Forget this document" (SPEC §9.1); pass false to include it again. */
  forgetDocument: (docId: string, forget: boolean) => unwrap(commands.memoryForgetDocument(docId, forget)),
  excluded: () => unwrap(commands.memoryExcluded()),
  embedding: {
    start: () => unwrap(commands.embeddingStart()),
    stop: () => unwrap(commands.embeddingStop()),
  },

  entities: {
    list: (opts: { type?: string; query?: string } = {}) =>
      unwrap(commands.entitiesList(opts.type ?? null, opts.query ?? null)),
    get: (id: string) => unwrap(commands.entityGet(id)),
    create: (type: string, name: string, notes = "") => unwrap(commands.entityCreate(type, name, notes)),
    update: (id: string, patch: Partial<EntityPatch>) =>
      unwrap(
        commands.entityUpdate(id, {
          name: null,
          type: null,
          aliases: null,
          fields: null,
          tags: null,
          notes: null,
          threadStatus: null,
          ...patch,
        }),
      ),
    /** Moves the entry to the bible Trash; `restore` brings it back (undo). */
    delete: (id: string) => unwrap(commands.entityDelete(id)),
    restore: (entity: Entity) => unwrap(commands.entityRestore(entity)),
    types: () => unwrap(commands.bibleTypes()),
  },
  facts: {
    add: (entity: string, text: string, field: string | null = null, source: Source | null = null) =>
      unwrap(commands.factAdd(entity, text, field, source)),
    update: (entity: string, fact: string, text: string, field: string | null = null) =>
      unwrap(commands.factUpdate(entity, fact, text, field)),
    delete: (entity: string, fact: string) => unwrap(commands.factDelete(entity, fact)),
  },
  relations: {
    list: (opts: { entity?: string; type?: string } = {}) =>
      unwrap(commands.relationsList(opts.entity ?? null, opts.type ?? null)),
    graph: () => unwrap(commands.relationsGraph()),
    add: (from: string, to: string, kind: string, since: string | null = null) =>
      unwrap(commands.relationAdd(from, to, kind, since)),
    delete: (entity: string, relation: string) => unwrap(commands.relationDelete(entity, relation)),
  },
  mentions: {
    byEntity: (id: string) => unwrap(commands.mentionsByEntity(id)),
    matrix: (types: string[] = []) => unwrap(commands.mentionsMatrix(types)),
  },
  timeline: { events: () => unwrap(commands.timelineEvents()) },
  threads: { list: (status: ThreadStatus | null = null) => unwrap(commands.threadsList(status)) },
  scenes: { cards: () => unwrap(commands.sceneCards()) },
  metrics: { series: (key: string) => unwrap(commands.metricsSeries(key)) },
  /** Canon actions on the inferred records the model made. */
  makeCanon: (items: ItemRef[]) => unwrap(commands.memoryMakeCanon(items)),
  deleteInferred: (items: ItemRef[]) => unwrap(commands.memoryDeleteInferred(items)),
  inspector: { rows: () => unwrap(commands.inspectorRows()) },
  search: {
    text: (query: string, k = 8) => unwrap(commands.searchText(query, k)),
    semantic: (query: string, k = 8) => unwrap(commands.searchSemantic(query, k)),
  },
  /** Where a stored source points now (paragraph ids survive edits). */
  resolve: (source: Source) => unwrap(commands.resolveSource(source)),

  margin: {
    ideas: (req: IdeasRequest) => unwrap(commands.ideasGenerate(req)),
    expand: (docId: string, idea: string, textBefore: string) => unwrap(commands.ideaExpand(docId, idea, textBefore)),
    pinned: () => unwrap(commands.ideasPinned()),
    pin: (idea: Idea, docId: string) => unwrap(commands.ideaPin(idea, docId)),
    unpin: (id: string) => unwrap(commands.ideaUnpin(id)),
    toNote: (docId: string, text: string) => unwrap(commands.ideaToNote(docId, text)),
    ask: (question: string) => unwrap(commands.askBook(question)),
    phrasing: (req: PhrasingRequest) => unwrap(commands.phrasingGenerate(req)),
  },
  export: (dir: string, format: ExportFormat) => unwrap(commands.memoryExport(dir, format)),
  import: (path: string) => unwrap(commands.memoryImport(path)),
};

interface MemoryStore {
  /** Bumps on every `memory.updated` and project change: refetch when it moves. */
  version: number;
  status: MemoryStatus | null;
}

const useStore = create<MemoryStore>(() => ({ version: 0, status: null }));
let following = false;

async function refreshStatus(): Promise<void> {
  try {
    useStore.setState({ status: await memory.status() });
  } catch {
    useStore.setState({ status: null });
  }
}

function follow(): void {
  if (following) return;
  following = true;
  events.on("memory.updated", () => {
    useStore.setState((s) => ({ version: s.version + 1 }));
    void refreshStatus(); // the entry count changed
  });
  events.on("index.progress", () => void refreshStatus());
  events.on("project.opened", () => {
    useStore.setState((s) => ({ version: s.version + 1 }));
    // The backend loads memory in the background right after opening.
    setTimeout(() => void refreshStatus(), 400);
  });
  events.on("project.closed", () => useStore.setState((s) => ({ version: s.version + 1, status: null })));
  events.on("doc.saved", () => void refreshStatus());
  events.on("binder.changed", () => useStore.setState((s) => ({ version: s.version + 1 })));
  void refreshStatus();
}

/** Re-render (and refetch) whenever memory changes. */
export function useMemoryVersion(): number {
  follow();
  return useStore((s) => s.version);
}

/** Indexing state for the status bar and the Memory panels. */
export function useMemoryStatus(): MemoryStatus | null {
  follow();
  return useStore((s) => s.status);
}

// --- Shared view ids and navigation (the SDK is the contract between modules) --------

export const BIBLE_ENTRY_VIEW = "bible.entry";
export const MEMORY_INSPECTOR_VIEW = "memory.inspector";
export const CONTEXT_VIEW = "ai.context";
export const MARGIN_PANEL = "margin.panel";
export const BIBLE_PANEL = "bible.panel";

/** Anything that points at a passage: a stored source or a citation. */
export interface PassagePointer {
  doc: string;
  para?: string | null;
  block?: number | null;
  quote: string;
}

/**
 * Open the scene a source points at and select the quoted words (SPEC §9.6:
 * every item traces to its passage in two clicks). Falls back to opening the
 * scene when the paragraph can no longer be found.
 */
export async function openSource(src: PassagePointer): Promise<void> {
  const { editorHost } = await import("./editorHost");
  const { workbench } = await import("./workbench");
  try {
    const hit = await memory.resolve({
      doc: src.doc,
      para: src.para ?? null,
      block: src.block ?? null,
      quote: src.quote,
    } as Source);
    if (!hit) {
      workbench.openDocument(src.doc);
      return;
    }
    const q = src.quote.replace(/…$/, "").trim();
    const i = q ? hit.text.indexOf(q) : -1;
    editorHost.openAt(
      hit.doc,
      i >= 0
        ? { block: hit.block, start: i, end: i + q.length, text: q }
        : { block: hit.block, start: 0, end: hit.text.length, text: hit.text },
    );
  } catch {
    workbench.openDocument(src.doc);
  }
}
