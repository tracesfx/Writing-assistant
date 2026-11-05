// @vitest-environment jsdom
import { Editor, type JSONContent } from "@tiptap/core";
import { afterEach, describe, expect, it } from "vitest";

import { countWords, schemaExtensions } from "./schema";

/**
 * A document in the exact shape the Rust bridge (`dialect/pm.rs`) produces
 * for every construct of the dialect. The editor must load and return it
 * unchanged, or text and formatting would drift between editor and disk.
 */
const BRIDGE_DOC: JSONContent = {
  type: "doc",
  content: [
    { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "Chapter heading" }] },
    {
      type: "paragraph",
      content: [
        { type: "text", text: "Plain " },
        { type: "text", text: "italic", marks: [{ type: "italic" }] },
        { type: "text", text: " " },
        { type: "text", text: "bold", marks: [{ type: "bold" }] },
        { type: "text", text: " " },
        { type: "text", text: "caps", marks: [{ type: "smallCaps" }] },
        { type: "text", text: " " },
        { type: "text", text: "kept together", marks: [{ type: "noBreak" }] },
        { type: "text", text: " " },
        { type: "text", text: "link", marks: [{ type: "link", attrs: { href: "https://example.com" } }] },
        { type: "hardBreak" },
        { type: "text", text: "second line" },
      ],
    },
    { type: "sceneBreak" },
    { type: "blockquote", content: [{ type: "paragraph", content: [{ type: "text", text: "Quoted." }] }] },
    {
      type: "styledBlock",
      attrs: { style: "epigraph" },
      content: [
        { type: "paragraph", content: [{ type: "text", text: "The sea is everything." }] },
        { type: "styledBlock", attrs: { style: "attribution" }, content: [{ type: "paragraph", content: [{ type: "text", text: "Verne" }] }] },
      ],
    },
    {
      type: "bulletList",
      attrs: { tight: true },
      content: [{ type: "listItem", content: [{ type: "paragraph", content: [{ type: "text", text: "one" }] }] }],
    },
    {
      type: "orderedList",
      attrs: { tight: false, start: 3 },
      content: [{ type: "listItem", content: [{ type: "paragraph", content: [{ type: "text", text: "three" }] }] }],
    },
  ],
};

/** Keep only the attributes the bridge reads, so editor defaults don't count as drift. */
function bridgeView(n: JSONContent): JSONContent {
  const keep: Record<string, string[]> = {
    heading: ["level"],
    styledBlock: ["style"],
    bulletList: ["tight"],
    orderedList: ["tight", "start"],
    link: ["href"],
  };
  const out: JSONContent = { type: n.type };
  const wanted = keep[n.type ?? ""];
  if (wanted && n.attrs) out.attrs = Object.fromEntries(wanted.map((k) => [k, n.attrs?.[k]]));
  if (n.text !== undefined) out.text = n.text;
  if (n.marks?.length) out.marks = n.marks.map((m) => bridgeView(m as JSONContent)) as JSONContent["marks"];
  if (n.content?.length) out.content = n.content.map(bridgeView);
  return out;
}

let editor: Editor | null = null;
afterEach(() => {
  editor?.destroy();
  editor = null;
});

describe("editor schema", () => {
  it("loads every dialect construct from the bridge without loss", () => {
    editor = new Editor({ extensions: schemaExtensions(), content: BRIDGE_DOC });
    expect(bridgeView(editor.getJSON())).toEqual(bridgeView(BRIDGE_DOC));
  });

  it("toggles semantic styles", () => {
    editor = new Editor({ extensions: schemaExtensions(), content: "<p>Line</p>" });
    editor.commands.selectAll();
    editor.commands.toggleStyledBlock("verse");
    expect(editor.getJSON().content?.[0]?.type).toBe("styledBlock");
    editor.commands.toggleStyledBlock("verse");
    expect(editor.getJSON().content?.[0]?.type).toBe("paragraph");
    editor.commands.insertSceneBreak();
    expect(editor.getJSON().content?.some((n) => n.type === "sceneBreak")).toBe(true);
  });
});

describe("countWords", () => {
  it("matches the backend rules", () => {
    expect(countWords("Hello, world — it's nine o'clock.")).toBe(5);
    expect(countWords("well—known … ok")).toBe(3);
    expect(countWords("   ")).toBe(0);
  });
});
