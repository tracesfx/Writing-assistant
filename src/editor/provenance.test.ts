// @vitest-environment jsdom
/**
 * Provenance of accepted AI text (SPEC §7.6, §17 check 5): tracked as plugin
 * ranges, mapped through edits, never in the document, saved as text offsets
 * plus a hash.
 */
import { Editor } from "@tiptap/core";
import { undoDepth } from "@tiptap/pm/history";
import { afterEach, describe, expect, it } from "vitest";

import {
  aiRanges,
  aiWordCount,
  fromOffsets,
  normalize,
  offsetAt,
  plainText,
  posAt,
  Provenance,
  setAiRanges,
  sha256Hex,
  toOffsets,
  type Range,
} from "./provenance";
import { schemaExtensions } from "./schema";

let editor: Editor | null = null;
afterEach(() => {
  editor?.destroy();
  editor = null;
});

function make(html: string, highlight = false): Editor {
  editor = new Editor({
    extensions: [...schemaExtensions(), Provenance.configure({ highlight: () => highlight })],
    content: html,
  });
  return editor;
}

const ranges = (ed: Editor): Range[] => aiRanges(ed.state).map((r) => [...r] as unknown as Range);
const textOf = (ed: Editor, rs: readonly Range[]) => rs.map(([a, b]) => ed.state.doc.textBetween(a, b, "\n\n", ""));

describe("range arithmetic", () => {
  it("merges touching and overlapping ranges and drops empty ones", () => {
    expect(normalize([[5, 9], [1, 3], [3, 4], [8, 12], [20, 20]])).toEqual([[1, 4], [5, 12]]);
  });
});

describe("provenance ranges follow the text", () => {
  // <p>Hello brave new world</p>: "brave" is positions 7–12.
  const HTML = "<p>Hello brave new world</p>";

  it("shift when text is inserted before them", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[7, 12]]);
    ed.view.dispatch(ed.state.tr.insertText("Oh ", 1));
    expect(textOf(ed, ranges(ed))).toEqual(["brave"]);
  });

  it("split around text typed inside, which is not marked", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[7, 12]]);
    ed.view.dispatch(ed.state.tr.insertText("XY", 9));
    expect(ed.getText()).toBe("Hello brXYave new world");
    expect(textOf(ed, ranges(ed))).toEqual(["br", "ave"]);
  });

  it("lose a character the author retypes", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[7, 12]]);
    ed.view.dispatch(ed.state.tr.insertText("Z", 8, 9));
    expect(ed.getText()).toBe("Hello bZave new world");
    expect(textOf(ed, ranges(ed))).toEqual(["b", "ave"]);
  });

  it("do not grow when text is typed at either edge", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[7, 12]]);
    ed.view.dispatch(ed.state.tr.insertText("!", 12));
    ed.view.dispatch(ed.state.tr.insertText("*", 7));
    expect(textOf(ed, ranges(ed))).toEqual(["brave"]);
  });

  it("disappear with deleted text and close up around a deleted gap", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[1, 6], [7, 12]]);
    ed.view.dispatch(ed.state.tr.delete(6, 7));
    expect(ed.getText()).toBe("Hellobrave new world");
    expect(textOf(ed, ranges(ed))).toEqual(["Hellobrave"]);
    ed.view.dispatch(ed.state.tr.delete(1, 11));
    expect(ranges(ed)).toEqual([]);
  });

  it("belong to the paragraph they were in when it splits", () => {
    const ed = make(HTML);
    setAiRanges(ed.view, [[7, 12]]);
    ed.commands.setTextSelection(13);
    ed.commands.splitBlock();
    expect(textOf(ed, ranges(ed))).toEqual(["brave"]);
  });
});

describe("provenance never touches the document", () => {
  it("is not in the document JSON or HTML, nor in the undo history", () => {
    const ed = make("<p>One two three.</p>", true);
    const before = JSON.stringify(ed.getJSON());
    setAiRanges(ed.view, [[5, 8]]);
    expect(JSON.stringify(ed.getJSON())).toBe(before);
    expect(ed.getHTML()).not.toContain("ai-assisted");
    expect(ed.getText()).toBe("One two three.");
    expect(undoDepth(ed.state)).toBe(0);
  });

  it("is shown only when the highlight view is on", () => {
    const off = make("<p>One two three.</p>", false);
    setAiRanges(off.view, [[5, 8]]);
    expect(off.view.dom.querySelectorAll(".ai-assisted")).toHaveLength(0);
    off.destroy();
    const on = make("<p>One two three.</p>", true);
    setAiRanges(on.view, [[5, 8]]);
    const marks = on.view.dom.querySelectorAll(".ai-assisted");
    expect(marks).toHaveLength(1);
    expect(marks[0]?.textContent).toBe("two");
  });

  it("reports changes, including text moving under a range", () => {
    const seen: number[] = [];
    editor = new Editor({
      extensions: [...schemaExtensions(), Provenance.configure({ onChange: (r) => seen.push(r.length) })],
      content: "<p>One two three.</p>",
    });
    setAiRanges(editor.view, [[5, 8]]);
    expect(seen).toEqual([1]);
    editor.view.dispatch(editor.state.tr.insertText("So ", 1));
    expect(seen).toEqual([1, 1]);
    editor.view.dispatch(editor.state.tr.delete(1, 17));
    expect(seen).toEqual([1, 1, 0]);
  });
});

describe("saved form: text offsets and a hash", () => {
  const HTML = "<p>One two.</p><p></p><p>Three four five.</p>";

  it("round-trips ranges across paragraphs, empty paragraphs included", () => {
    const ed = make(HTML);
    const doc = ed.state.doc;
    expect(plainText(doc)).toBe("One two.\n\nThree four five.");
    // "two." is 5–9; "four five." is 19–29 in the third paragraph (the empty one is 10–12).
    const rs: Range[] = [[5, 9], [19, 29]];
    expect(textOf(ed, rs)).toEqual(["two.", "four five."]);
    const offsets = toOffsets(doc, rs);
    expect(offsets.map((o) => plainText(doc).slice(o.start, o.end))).toEqual(["two.", "four five."]);
    expect(fromOffsets(doc, offsets)).toEqual(rs);
  });

  it("finds the same text after the document is rebuilt without the empty paragraph", () => {
    const ed = make(HTML);
    const offsets = toOffsets(ed.state.doc, [[19, 29]]);
    ed.destroy();
    const again = make("<p>One two.</p><p>Three four five.</p>");
    const rs = fromOffsets(again.state.doc, offsets);
    expect(textOf(again, rs)).toEqual(["four five."]);
  });

  it("ignores empty paragraphs, so a trailing one does not change the hash", () => {
    const a = make("<p>One two.</p><p>Three.</p>");
    const b = make("<p></p><p>One two.</p><p></p><p></p><p>Three.</p><p></p>");
    expect(plainText(b.state.doc)).toBe(plainText(a.state.doc));
    expect(sha256Hex(plainText(b.state.doc))).toBe(sha256Hex(plainText(a.state.doc)));
  });

  it("converts positions and offsets both ways", () => {
    const ed = make("<p>ab</p><p>cd</p>");
    const doc = ed.state.doc;
    expect(offsetAt(doc, 1)).toBe(0);
    expect(offsetAt(doc, 3)).toBe(2);
    expect(offsetAt(doc, 5)).toBe(4); // start of "cd": after the blank line
    // A range starts inside its paragraph and ends right after its last character.
    expect(posAt(doc, 0, "start")).toBe(1);
    expect(posAt(doc, 4, "start")).toBe(5);
    expect(posAt(doc, 2, "end")).toBe(3);
    expect(posAt(doc, 6, "end")).toBe(7);
  });

  it("counts the words inside the ranges", () => {
    const ed = make("<p>One two three four.</p>");
    expect(aiWordCount(ed.state.doc, [[5, 14]])).toBe(2);
    expect(aiWordCount(ed.state.doc, [])).toBe(0);
  });

  it("the hash changes when the text changes", () => {
    const a = make("<p>One two.</p>");
    const h1 = sha256Hex(plainText(a.state.doc));
    a.view.dispatch(a.state.tr.insertText("x", 2));
    expect(sha256Hex(plainText(a.state.doc))).not.toBe(h1);
  });
});
