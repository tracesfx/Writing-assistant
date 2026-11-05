// @vitest-environment jsdom
/**
 * View-only editor plugins (SPEC §11.2, §11.4): smart typography, focus mode,
 * bookmarks and spellcheck. Decorations must never reach the document or the
 * undo history; typography must be undoable back to the literal characters.
 */
import { Editor } from "@tiptap/core";
import { undoDepth } from "@tiptap/pm/history";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BOOKMARK_SAVE_MS, bookmarkPositions, Bookmarks, nextBookmark, toggleBookmark } from "./bookmarks";
import { FocusMode, sentenceAt } from "./focus";
import { schemaExtensions } from "./schema";
import { clearSpellCache, refreshSpelling, Spellcheck } from "./spellcheck";
import { quoteFor, SmartTypography } from "./typography";

let editor: Editor | null = null;
afterEach(() => {
  editor?.destroy();
  editor = null;
});

/** Type `text` one character at a time through the input-rule path, like a keyboard. */
function type(ed: Editor, text: string): void {
  for (const ch of text) {
    const { from, to } = ed.state.selection;
    const handled = ed.view.someProp("handleTextInput", (f) => f(ed.view, from, to, ch, () => ed.state.tr.insertText(ch, from, to)));
    if (!handled) ed.view.dispatch(ed.state.tr.insertText(ch, from, to));
  }
}

const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms));

describe("smart typography", () => {
  it("curls quotes, and makes dashes and ellipses", () => {
    editor = new Editor({ extensions: [...schemaExtensions(), SmartTypography], content: "<p></p>" });
    editor.commands.focus("end");
    type(editor, `"Nobody waits," she said -- it's true...`);
    expect(editor.getText()).toBe("“Nobody waits,” she said — it’s true…");
  });

  it("restores the literal characters on undo right after a replacement", () => {
    editor = new Editor({ extensions: [...schemaExtensions(), SmartTypography], content: "<p></p>" });
    editor.commands.focus("end");
    type(editor, "a--");
    expect(editor.getText()).toBe("a—");
    editor.commands.undoInputRule();
    expect(editor.getText()).toBe("a--");
  });

  it("respects the per-rule settings", () => {
    editor = new Editor({
      extensions: [...schemaExtensions(), SmartTypography.configure({ enabled: () => ({ quotes: false, dashes: true, ellipsis: false }) })],
      content: "<p></p>",
    });
    editor.commands.focus("end");
    type(editor, `"x" -- ...`);
    expect(editor.getText()).toBe(`"x" — ...`);
  });

  it("chooses opening or closing quotes from the character before", () => {
    expect(quoteFor("double", "")).toBe("“");
    expect(quoteFor("double", " ")).toBe("“");
    expect(quoteFor("double", "s")).toBe("”");
    expect(quoteFor("single", "t")).toBe("’");
    expect(quoteFor("single", "(")).toBe("‘");
  });
});

describe("focus mode", () => {
  it("finds the sentence around the caret", () => {
    const text = "First one. Second one! Third?";
    expect(text.slice(...sentenceAt(text, 3))).toBe("First one.");
    expect(text.slice(...sentenceAt(text, 14)).trim()).toBe("Second one!");
  });

  it("decorates the current paragraph without touching the document", () => {
    editor = new Editor({
      extensions: [...schemaExtensions(), FocusMode.configure({ mode: () => "sentence" })],
      content: "<p>One. Two.</p><p>Three.</p>",
    });
    const before = JSON.stringify(editor.getJSON());
    editor.commands.setTextSelection(2);
    expect(editor.view.dom.classList.contains("focus-mode--sentence")).toBe(true);
    expect(editor.view.dom.querySelectorAll(".focus-current")).toHaveLength(1);
    expect(editor.view.dom.querySelector(".focus-sentence")?.textContent).toBe("One.");
    expect(JSON.stringify(editor.getJSON())).toBe(before);
    expect(editor.getHTML()).not.toContain("focus-");
  });
});

describe("bookmarks", () => {
  it("are view state: decorated, mapped through edits, never in the document or undo history", async () => {
    const changes: number[][] = [];
    editor = new Editor({
      extensions: [...schemaExtensions(), Bookmarks.configure({ onChange: (p) => changes.push(p) })],
      content: "<p>One.</p><p>Two.</p>",
    });
    const before = JSON.stringify(editor.getJSON());
    editor.commands.setTextSelection(8); // inside "Two."
    toggleBookmark(editor.view);
    expect(bookmarkPositions(editor.state)).toEqual([6]);
    expect(editor.view.dom.querySelectorAll(".bookmarked")).toHaveLength(1);
    expect(JSON.stringify(editor.getJSON())).toBe(before);
    expect(undoDepth(editor.state)).toBe(0);
    await tick();
    expect(changes).toEqual([[6]]);

    // An edit before the bookmark moves it along with its paragraph, and the
    // new position is reported once typing pauses.
    vi.useFakeTimers();
    editor.commands.insertContentAt(0, "<p>Zero.</p>");
    const moved = bookmarkPositions(editor.state)[0]!;
    expect(editor.state.doc.nodeAt(moved)?.textContent).toBe("Two.");
    expect(changes).toHaveLength(1);
    vi.advanceTimersByTime(BOOKMARK_SAVE_MS);
    vi.useRealTimers();
    expect(changes.at(-1)).toEqual([moved]);

    editor.commands.setTextSelection(1);
    expect(nextBookmark(editor.view)).toBe(true);
    expect(editor.state.selection.$head.parent.textContent).toBe("Two.");
  });
});

describe("spellcheck", () => {
  it("marks misspellings as decorations only", async () => {
    clearSpellCache();
    editor = new Editor({
      extensions: [
        ...schemaExtensions(),
        Spellcheck.configure({
          enabled: () => true,
          check: async (texts) => texts.map((t) => (t.includes("teh") ? [{ start: t.indexOf("teh"), end: t.indexOf("teh") + 3 }] : [])),
          ignored: () => new Set(),
        }),
      ],
      content: "<p>Over teh harbor.</p>",
    });
    const before = JSON.stringify(editor.getJSON());
    refreshSpelling(editor.view);
    await tick(20);
    const marks = editor.view.dom.querySelectorAll(".misspelled");
    expect(marks).toHaveLength(1);
    expect(marks[0]?.textContent).toBe("teh");
    expect(JSON.stringify(editor.getJSON())).toBe(before);
    expect(editor.getHTML()).not.toContain("misspelled");
    expect(undoDepth(editor.state)).toBe(0);
  });

  it("skips ignored words", async () => {
    clearSpellCache();
    editor = new Editor({
      extensions: [
        ...schemaExtensions(),
        Spellcheck.configure({
          enabled: () => true,
          check: async (texts) => texts.map(() => [{ start: 0, end: 4 }]),
          ignored: () => new Set(["Mara"]),
        }),
      ],
      content: "<p>Mara stood.</p>",
    });
    refreshSpelling(editor.view);
    await tick(20);
    expect(editor.view.dom.querySelectorAll(".misspelled")).toHaveLength(0);
  });
});
