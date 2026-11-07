import type { JSONContent } from "@tiptap/core";
import { TextSelection } from "@tiptap/pm/state";
import { EditorContent, useEditor, type Editor } from "@tiptap/react";
import { useEffect, useReducer, useRef, useState } from "react";

import {
  ai,
  aiSession,
  editorHost,
  events,
  lifecycle,
  settings,
  story,
  uiState,
  useSettings,
  useWorkbench,
  type DocumentView as DocView,
  type PmNode,
  type TextTarget,
  type WorkspaceViewProps,
} from "@margin/sdk";

import { Bookmarks, bookmarkPositions } from "../../editor/bookmarks";
import { wordDiff, type DiffOp } from "../../editor/diff";
import { FocusMode, Typewriter } from "../../editor/focus";
import { GhostText, ghostControl } from "../../editor/ghost";
import {
  Provenance,
  aiRanges,
  aiWordCount,
  fromOffsets,
  offsetAt,
  plainText,
  setAiRanges,
  sha256Hex,
  toOffsets,
} from "../../editor/provenance";
import { countWords, schemaExtensions, wordLeafText } from "../../editor/schema";
import { Spellcheck, refreshSpelling } from "../../editor/spellcheck";
import { locate } from "../../editor/textmap";
import { SmartTypography } from "../../editor/typography";
import { getHub, joinHub, leaveHub, onLocalTransaction, replaceContent, subscribeHub, type DocHub } from "./hub";
import { bookmarksOf, placeOf, rememberEdit, rememberPlace, saveBookmarks } from "./positions";
import { editors, ignoredWords, useEditorStore } from "./state";

// ProseMirror JSON and the backend's PmNode share one shape; the types differ
// only in optionality, so convert at this boundary.
const toTiptap = (n: PmNode) => n as unknown as JSONContent;

type Loaded = { id: string; doc: DocView } | { id: string; error: string };

export function DocumentView({ tabId, resourceId, active, focused }: WorkspaceViewProps) {
  // Keyed by document id, so a stale result is simply not shown.
  const [loaded, setLoaded] = useState<Loaded | null>(null);

  useEffect(() => {
    if (!resourceId) return;
    let cancelled = false;
    // A document already open in another pane shares its live model.
    const hub = getHub(resourceId);
    const load = hub?.doc
      ? story.documents.meta(resourceId).then((meta) => ({
          meta,
          content: hub.doc!.toJSON() as PmNode,
          revision: "",
        }))
      : story.documents.get(resourceId);
    load
      .then((doc) => !cancelled && setLoaded({ id: resourceId, doc }))
      .catch((e: unknown) => !cancelled && setLoaded({ id: resourceId, error: e instanceof Error ? e.message : String(e) }));
    return () => {
      cancelled = true;
    };
  }, [resourceId]);

  if (!resourceId || loaded?.id !== resourceId) return <div className="editor-message" aria-busy="true" />;
  if ("error" in loaded) {
    return (
      <div className="editor-message" role="alert">
        This document could not be opened: {loaded.error}
      </div>
    );
  }
  return <EditorSurface key={resourceId} tabId={tabId} initial={loaded.doc} active={active} focused={focused} />;
}

interface SurfaceProps {
  tabId: string;
  initial: DocView;
  active: boolean;
  focused: boolean;
}

interface FootnoteEdit {
  pos: number;
  text: string;
  x: number;
  y: number;
}

interface SpellMenu {
  word: string;
  from: number;
  to: number;
  x: number;
  y: number;
  suggestions: string[] | null;
}

function EditorSurface({ tabId, initial, active, focused }: SurfaceProps) {
  const docId = initial.meta.id;
  const hubRef = useRef<DocHub | null>(null);
  const countTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const scrollRef = useRef<HTMLDivElement>(null);
  // AI-accepted ranges are saved once the text they refer to is on disk.
  const provDirty = useRef(false);
  const provTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const provQuiet = useRef(false);
  const provWrite = useRef<() => void>(() => undefined);
  const update = useEditorStore((s) => s.update);
  const [title, setTitle] = useState(initial.meta.title);
  const [footnote, setFootnote] = useState<FootnoteEdit | null>(null);
  const [spellMenu, setSpellMenu] = useState<SpellMenu | null>(null);
  const [compare, setCompare] = useState<DiffOp[] | null>(null);
  const [, rerender] = useReducer((x: number) => x + 1, 0);
  const s = useSettings((x) => x);

  const editor = useEditor(
    {
      extensions: [
        ...schemaExtensions(),
        SmartTypography.configure({
          enabled: () => {
            const st = settings.get();
            return { quotes: st.smartQuotes, dashes: st.smartDashes, ellipsis: st.smartEllipsis };
          },
        }),
        FocusMode.configure({ mode: () => settings.get().focusMode }),
        Typewriter.configure({ enabled: () => settings.get().typewriter }),
        Bookmarks.configure({ initial: bookmarksOf(docId), onChange: (p) => saveBookmarks(docId, p) }),
        GhostText.configure({
          docId,
          client: ai.ghost,
          enabled: () => {
            const st = settings.get();
            return st.ghostEnabled && !st.flowMode;
          },
          ready: () => ai.isReady(),
          debounceMs: () => settings.get().ghostDebounce,
          length: () => settings.get().ghostLength,
          opacity: () => settings.get().ghostOpacity,
          italic: () => settings.get().ghostItalic,
          neverPush: () => settings.get().ghostNeverPush,
          typography: () => {
            const st = settings.get();
            return { quotes: st.smartQuotes, dashes: st.smartDashes, ellipsis: st.smartEllipsis };
          },
          onAccept: ({ text }) => aiSession.add(countWords(text)),
        }),
        Provenance.configure({
          highlight: () => settings.get().highlightAi,
          onChange: () => {
            if (provQuiet.current) return;
            provDirty.current = true;
            clearTimeout(provTimer.current);
            provTimer.current = setTimeout(() => provWrite.current(), 800);
          },
        }),
        Spellcheck.configure({
          enabled: () => settings.get().spellcheck,
          check: (texts) => story.spell.check(texts),
          ignored: () => ignoredWords,
          onMenu: (m) => setSpellMenu({ ...m, suggestions: null }),
        }),
      ],
      content: toTiptap(initial.content),
      editorProps: {
        attributes: {
          class: "manuscript",
          spellcheck: "false",
          "aria-label": "Document text",
          "aria-multiline": "true",
          role: "textbox",
        },
      },
      onCreate: ({ editor }) => {
        hubRef.current = joinHub(docId, editor, initial.content);
        update(tabId, { docId, words: initial.meta.wordCount, save: hubRef.current.save });
      },
      onUpdate: ({ editor, transaction }) => {
        const hub = hubRef.current;
        if (!hub) return;
        onLocalTransaction(hub, editor, transaction);
        if (!transaction.docChanged) return;
        if (!transaction.getMeta("marginRemote")) rememberEdit(docId, editor.state.selection.head);
        // Word count is computed after a short pause, never per keystroke.
        clearTimeout(countTimer.current);
        countTimer.current = setTimeout(() => {
          const d = editor.state.doc;
          update(tabId, { docId, words: countWords(d.textBetween(0, d.content.size, "\n", wordLeafText)) });
        }, 400);
      },
      onSelectionUpdate: ({ editor }) => {
        const { from, to, empty } = editor.state.selection;
        update(tabId, {
          docId,
          selectionWords: empty ? 0 : countWords(editor.state.doc.textBetween(from, to, "\n", wordLeafText)),
        });
        rememberPlace(docId, { pos: editor.state.selection.head, scroll: scrollRef.current?.scrollTop ?? 0 });
      },
      onBlur: () => void hubRef.current?.autosave.flush(),
    },
    [docId],
  );

  // Save the AI-accepted ranges (SPEC §7.6) once the document itself is saved, so
  // the hash always matches the text on disk.
  function provenanceRecord() {
    if (!editor || editor.isDestroyed) return null;
    const doc = editor.state.doc;
    const ranges = aiRanges(editor.state);
    return {
      schemaVersion: 1,
      docId,
      textHash: sha256Hex(plainText(doc)),
      ranges: toOffsets(doc, ranges),
      aiWords: aiWordCount(doc, ranges),
    };
  }
  provWrite.current = () => {
    const hub = hubRef.current;
    if (!hub || !provDirty.current) return;
    if (hub.autosave.isDirty || hub.save.status !== "saved") return;
    const rec = provenanceRecord();
    if (!rec) return;
    provDirty.current = false;
    story.provenance.put(rec).catch(() => {
      provDirty.current = true;
    });
  };

  useEffect(() => {
    if (!editor) return;
    let cancelled = false;
    story.provenance
      .get(docId)
      .then((rec) => {
        if (cancelled || !rec || editor.isDestroyed) return;
        const doc = editor.state.doc;
        provQuiet.current = true;
        try {
          if (sha256Hex(plainText(doc)) === rec.textHash) {
            setAiRanges(editor.view, fromOffsets(doc, rec.ranges));
          } else {
            // The file changed outside Margin: the ranges no longer fit. Say so once.
            void story.provenance.put({ ...rec, ranges: [], aiWords: 0 }).catch(() => undefined);
            useWorkbench
              .getState()
              .notify({ message: `“${initial.meta.title}” changed outside Margin, so its AI-assisted marks were cleared.`, timeoutMs: 8000 });
          }
        } finally {
          provQuiet.current = false;
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [editor, docId]); // eslint-disable-line react-hooks/exhaustive-deps

  // Hub membership, command handles and the before-close flush live exactly
  // as long as this mount.
  useEffect(() => {
    if (!editor) return;
    const hub = hubRef.current ?? joinHub(docId, editor, initial.content);
    hubRef.current = hub;
    editors.set(tabId, { docId, editor, hub });
    const unsub = subscribeHub(hub, () => {
      useEditorStore.getState().update(tabId, { docId, save: hub.save });
      provWrite.current();
      rerender();
    });
    const unregister = lifecycle.onBeforeClose(async () => {
      await hub.autosave.flush();
      provWrite.current();
    });
    const unhost = editorHost.register({
      docId,
      tabId,
      selectedText: () => {
        const { from, to } = editor.state.selection;
        return editor.state.doc.textBetween(from, to, "\n\n", "");
      },
      textBeforeCursor: () => {
        const full = plainText(editor.state.doc).slice(0, offsetAt(editor.state.doc, editor.state.selection.head));
        return full.length > 12000 ? full.slice(-12000) : full;
      },
      selectionContext: () => {
        const { from, to } = editor.state.selection;
        const doc = editor.state.doc;
        const full = plainText(doc);
        const a = offsetAt(doc, from);
        const b = offsetAt(doc, to);
        return { text: full.slice(a, b), before: full.slice(Math.max(0, a - 1500), a), after: full.slice(b, b + 400) };
      },
      insertText: (text) => {
        editor.chain().focus().insertContent(text.split(/\n{2,}/).map((p) => ({ type: "paragraph", content: p ? [{ type: "text", text: p }] : [] }))).run();
      },
      reveal: (t) => revealIn(editor, t),
      setCursor: (pos) => setCursorIn(editor, pos),
      cursor: () => editor.state.selection.head,
      focus: () => editor.commands.focus(),
      revealComment: (id) => {
        const r = commentRange(editor, id);
        if (!r) return false;
        editor.view.dispatch(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, r.from, r.to)).scrollIntoView());
        editor.commands.focus(undefined, { scrollIntoView: true });
        return true;
      },
      removeComment: (id) => {
        const type = editor.schema.marks.comment;
        if (!type) return;
        const tr = editor.state.tr;
        editor.state.doc.descendants((node, pos) => {
          const m = node.marks.find((x) => x.type === type && x.attrs.id === id);
          if (m) tr.removeMark(pos, pos + node.nodeSize, m);
        });
        if (tr.docChanged) editor.view.dispatch(tr);
      },
      isDirty: () => hub.autosave.isDirty,
      flush: () => hub.autosave.flush(),
    });
    // Restore the remembered place, or reveal a pending target.
    const pending = editorHost.takePendingReveal(docId);
    requestAnimationFrame(() => {
      if (editor.isDestroyed) return;
      if (pending) {
        if ("pos" in pending) setCursorIn(editor, pending.pos);
        else revealIn(editor, pending);
        return;
      }
      const place = placeOf(docId);
      if (place) {
        setCursorIn(editor, place.pos, false);
        if (scrollRef.current) scrollRef.current.scrollTop = place.scroll;
      }
    });
    return () => {
      unhost();
      unregister();
      unsub();
      if (editors.get(tabId)?.editor === editor) editors.delete(tabId);
      clearTimeout(provTimer.current);
      if (provDirty.current) {
        const rec = provenanceRecord();
        if (rec) void hub.autosave.flush().then(() => story.provenance.put(rec)).catch(() => undefined);
      }
      leaveHub(hub, editor);
      hubRef.current = null;
    };
  }, [editor, tabId, docId, initial.content]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => useEditorStore.getState().remove(tabId), [tabId]);

  // Settings that change decorations or attributes: redraw.
  useEffect(() => {
    if (editor && !editor.isDestroyed) editor.view.dispatch(editor.state.tr.setMeta("marginSettings", true));
  }, [editor, s.focusMode, s.typewriter, s.ghostOpacity, s.ghostItalic, s.ghostNeverPush, s.highlightAi]);
  // Turning ghost text off (or Flow mode on) takes back a suggestion that is showing.
  useEffect(() => {
    if (!editor || editor.isDestroyed) return;
    if (!s.ghostEnabled || s.flowMode) ghostControl(editor.view)?.dismiss();
  }, [editor, s.ghostEnabled, s.flowMode]);
  useEffect(() => {
    if (editor && !editor.isDestroyed) refreshSpelling(editor.view);
  }, [editor, s.spellcheck]);

  // Focus the editor when its tab becomes the focused one.
  useEffect(() => {
    if (editor && active && focused && !editor.isFocused) editor.commands.focus(undefined, { scrollIntoView: false });
  }, [editor, active, focused]);

  // Remember the scroll position.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !editor) return;
    let t: ReturnType<typeof setTimeout> | undefined;
    const onScroll = () => {
      clearTimeout(t);
      t = setTimeout(() => rememberPlace(docId, { pos: editor.state.selection.head, scroll: el.scrollTop }), 300);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [editor, docId]);

  // Domain events: rename, text replaced by Margin, outside edits, dictionary changes.
  useEffect(() => {
    const offs = [
      events.on("binder.changed", (e) => {
        if (!e.docIds.includes(docId)) return;
        void story.documents.meta(docId).then((m) => setTitle(m.title)).catch(() => undefined);
      }),
      events.on("doc.contentReplaced", (e) => {
        if (!e.docIds.includes(docId)) return;
        void reloadFromDisk();
      }),
      events.on("doc.changedOnDisk", (e) => {
        const hub = hubRef.current;
        if (e.docId !== docId || !hub) return;
        if (!hub.autosave.isDirty && hub.save.status !== "saving") void reloadFromDisk();
        else {
          hub.externalChange = true;
          rerender();
        }
      }),
      events.on("project.settingsChanged", () => {
        if (editor && !editor.isDestroyed) refreshSpelling(editor.view, { clearCache: true });
      }),
    ];
    return () => offs.forEach((o) => o());
  }, [docId, editor]); // eslint-disable-line react-hooks/exhaustive-deps

  // Footnote markers ask to be edited.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !editor) return;
    const onFootnote = (e: Event) => {
      const { pos } = (e as CustomEvent<{ pos: number }>).detail;
      const node = editor.state.doc.nodeAt(pos);
      if (!node || node.type.name !== "footnote") return;
      const rect = (e.target as HTMLElement).getBoundingClientRect();
      setFootnote({ pos, text: String(node.attrs.text ?? ""), x: rect.left, y: rect.bottom + 4 });
    };
    const onClick = (e: MouseEvent) => {
      const anchor = (e.target as HTMLElement).closest("[data-comment]");
      const id = anchor?.getAttribute("data-comment");
      if (id) uiState.set("comments.focus", { docId, id, at: Date.now() });
    };
    el.addEventListener("margin:footnote", onFootnote);
    el.addEventListener("click", onClick);
    return () => {
      el.removeEventListener("margin:footnote", onFootnote);
      el.removeEventListener("click", onClick);
    };
  }, [editor, docId]);

  // A footnote just inserted by command opens for editing.
  useEffect(() => {
    const onNew = (e: Event) => {
      const { tabId: t, pos } = (e as CustomEvent<{ tabId: string; pos: number }>).detail;
      if (t !== tabId || !editor) return;
      requestAnimationFrame(() => {
        const dom = editor.view.nodeDOM(pos);
        const rect = dom instanceof HTMLElement ? dom.getBoundingClientRect() : { left: 200, bottom: 200 };
        setFootnote({ pos, text: "", x: rect.left, y: rect.bottom + 4 });
      });
    };
    window.addEventListener("margin:footnote-new", onNew);
    return () => window.removeEventListener("margin:footnote-new", onNew);
  }, [editor, tabId]);

  async function reloadFromDisk() {
    const hub = hubRef.current;
    if (!hub) return;
    const fresh = await story.documents.get(docId);
    replaceContent(hub, fresh.content);
    setCompare(null);
  }

  async function showCompare() {
    const hub = hubRef.current;
    if (!hub?.doc || !editor) return;
    // Compare the editor's text with the file on disk, word by word. Peeking
    // does not accept the disk version, so the conflict stays unresolved.
    const disk = editor.schema.nodeFromJSON(await story.documents.peek(docId));
    const text = (d: { textBetween(a: number, b: number, s: string, l: string): string; content: { size: number } }) =>
      d.textBetween(0, d.content.size, "\n\n", "");
    setCompare(wordDiff(text(disk), text(hub.doc)));
  }

  async function commitTitle(value: string) {
    const t = value.trim();
    if (!t || t === initial.meta.title) {
      setTitle(initial.meta.title);
      return;
    }
    try {
      const meta = await story.documents.updateMeta(docId, { title: t });
      setTitle(meta.title);
    } catch (e) {
      useWorkbench.getState().notify({ message: `Could not rename: ${e instanceof Error ? e.message : String(e)}` });
    }
  }

  const hub = hubRef.current;
  const status = hub?.save.status;
  const external = hub?.externalChange || status === "conflict";

  return (
    <div className="editor-scroll" ref={scrollRef}>
      {external && (
        <div className="inline-bar" role="status">
          <span>This document was changed outside Margin while you had unsaved edits.</span>
          <button onClick={() => void hub?.autosave.keepMine()}>Keep mine</button>
          <button onClick={() => void reloadFromDisk()}>Load from disk</button>
          <button onClick={() => void showCompare()}>Compare</button>
        </div>
      )}
      {compare && (
        <div className="compare-panel" role="region" aria-label="Your text compared with the file on disk">
          <div className="compare-head">
            <span>
              <del>Only on disk</del> · <ins>Only in your version</ins>
            </span>
            <button onClick={() => setCompare(null)}>Close</button>
          </div>
          <div className="diff-text">
            {compare.map((op, i) =>
              op.kind === "equal" ? <span key={i}>{op.text}</span> : op.kind === "insert" ? <ins key={i}>{op.text}</ins> : <del key={i}>{op.text}</del>,
            )}
          </div>
        </div>
      )}
      <div className="page">
        <input
          className="doc-title"
          aria-label="Document title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={(e) => void commitTitle(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === "ArrowDown") {
              e.preventDefault();
              editor?.commands.focus("start");
            } else if (e.key === "Escape") {
              setTitle(initial.meta.title);
              editor?.commands.focus();
            }
          }}
        />
        <EditorContent editor={editor} />
      </div>
      {footnote && editor && (
        <FootnotePopover
          edit={footnote}
          onChange={(text) => {
            const node = editor.state.doc.nodeAt(footnote.pos);
            if (node?.type.name !== "footnote") return;
            editor.view.dispatch(editor.state.tr.setNodeMarkup(footnote.pos, undefined, { ...node.attrs, text }));
          }}
          onClose={() => {
            setFootnote(null);
            editor.commands.focus();
          }}
        />
      )}
      {spellMenu && editor && (
        <SpellMenuView
          menu={spellMenu}
          onReplace={(w) => {
            editor.chain().focus().insertContentAt({ from: spellMenu.from, to: spellMenu.to }, w).run();
            setSpellMenu(null);
          }}
          onAdd={() => {
            void story.spell.add(spellMenu.word).then(() => {
              for (const h of editors.values()) refreshSpelling(h.editor.view, { clearCache: true });
            });
            setSpellMenu(null);
          }}
          onIgnore={() => {
            ignoredWords.add(spellMenu.word);
            for (const h of editors.values()) refreshSpelling(h.editor.view);
            setSpellMenu(null);
          }}
          onClose={() => setSpellMenu(null)}
          onSuggestions={(list) => setSpellMenu((m) => (m ? { ...m, suggestions: list } : m))}
        />
      )}
      <span hidden data-bookmarks={editor ? bookmarkPositions(editor.state).length : 0} />
    </div>
  );
}

/** Extent of the text carrying comment `id`. */
function commentRange(editor: Editor, id: string): { from: number; to: number } | null {
  let from = -1;
  let to = -1;
  editor.state.doc.descendants((node, pos) => {
    if (node.isText && node.marks.some((m) => m.type.name === "comment" && m.attrs.id === id)) {
      if (from < 0) from = pos;
      to = pos + node.nodeSize;
    }
  });
  return from < 0 ? null : { from, to };
}

function setCursorIn(editor: Editor, pos: number, scroll = true): void {
  const size = editor.state.doc.content.size;
  const p = Math.max(0, Math.min(pos, size));
  const sel = TextSelection.near(editor.state.doc.resolve(p));
  const tr = editor.state.tr.setSelection(sel);
  editor.view.dispatch(scroll ? tr.scrollIntoView() : tr);
  editor.commands.focus(undefined, { scrollIntoView: scroll });
}

function revealIn(editor: Editor, t: TextTarget): void {
  const r = locate(editor.state.doc, t, t.text);
  if (!r) return;
  const sel = TextSelection.create(editor.state.doc, r.from, r.to);
  editor.view.dispatch(editor.state.tr.setSelection(sel).scrollIntoView());
  editor.commands.focus(undefined, { scrollIntoView: true });
}

function FootnotePopover({
  edit,
  onChange,
  onClose,
}: {
  edit: FootnoteEdit;
  onChange: (text: string) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState(edit.text);
  return (
    <div className="popover footnote-popover" style={{ left: Math.max(8, edit.x - 20), top: edit.y }} role="dialog" aria-label="Footnote">
      <label className="popover-label" htmlFor="footnote-text">
        Footnote
      </label>
      <textarea
        id="footnote-text"
        autoFocus
        rows={3}
        value={text}
        placeholder="Footnote text (*italic* works)"
        onChange={(e) => {
          setText(e.target.value);
          onChange(e.target.value.replace(/\n+/g, " "));
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape" || (e.key === "Enter" && !e.shiftKey)) {
            e.preventDefault();
            onClose();
          }
        }}
        onBlur={onClose}
      />
    </div>
  );
}

function SpellMenuView({
  menu,
  onReplace,
  onAdd,
  onIgnore,
  onClose,
  onSuggestions,
}: {
  menu: SpellMenu;
  onReplace: (w: string) => void;
  onAdd: () => void;
  onIgnore: () => void;
  onClose: () => void;
  onSuggestions: (s: string[]) => void;
}) {
  useEffect(() => {
    if (menu.suggestions !== null) return;
    story.spell
      .suggest(menu.word)
      .then(onSuggestions)
      .catch(() => onSuggestions([]));
  }, [menu.word]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".spell-menu")) onClose();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [onClose]);
  return (
    <div className="popover spell-menu menu" style={{ left: menu.x, top: menu.y }} role="menu">
      {menu.suggestions === null && <div className="menu-note">Looking…</div>}
      {menu.suggestions?.length === 0 && <div className="menu-note">No suggestions</div>}
      {menu.suggestions?.map((s) => (
        <button key={s} role="menuitem" className="menu-item strong" onClick={() => onReplace(s)}>
          {s}
        </button>
      ))}
      <div className="menu-sep" />
      <button role="menuitem" className="menu-item" onClick={onAdd}>
        Add “{menu.word}” to dictionary
      </button>
      <button role="menuitem" className="menu-item" onClick={onIgnore}>
        Ignore this session
      </button>
    </div>
  );
}
