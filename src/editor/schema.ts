/**
 * Editor schema: TipTap extensions for Margin's semantic styles (SPEC §12.1).
 * Node and mark names must match the Rust dialect bridge
 * (`src-tauri/src/dialect/pm.rs`).
 */
import { Extension, Mark, Node, mergeAttributes, nodeInputRule } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";

export const DIV_STYLES = ["epigraph", "attribution", "letter", "verse", "center", "textmsg"] as const;
export type DivStyle = (typeof DIV_STYLES)[number];

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    sceneBreak: { insertSceneBreak: () => ReturnType };
    styledBlock: { toggleStyledBlock: (style: DivStyle) => ReturnType };
    smallCaps: { toggleSmallCaps: () => ReturnType };
    noBreak: { toggleNoBreak: () => ReturnType };
  }
}

export const SceneBreak = Node.create({
  name: "sceneBreak",
  group: "block",
  atom: true,
  selectable: true,
  parseHTML() {
    return [{ tag: "hr" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["hr", mergeAttributes(HTMLAttributes, { class: "scene-break" })];
  },
  addCommands() {
    return {
      insertSceneBreak:
        () =>
        ({ chain }) =>
          chain().insertContent([{ type: this.name }, { type: "paragraph" }]).run(),
    };
  },
  addInputRules() {
    // `***` or `* * *` followed by a space at the start of an empty paragraph.
    return [nodeInputRule({ find: /^(?:\*\*\*|\* \* \*)\s$/, type: this.type })];
  },
});

export const StyledBlock = Node.create({
  name: "styledBlock",
  group: "block",
  content: "block+",
  defining: true,
  addAttributes() {
    return {
      style: {
        default: "center",
        parseHTML: (el) => el.getAttribute("data-style") ?? "center",
        renderHTML: (attrs) => ({ "data-style": String(attrs.style) }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "div[data-style]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["div", mergeAttributes(HTMLAttributes, { class: "styled-block" }), 0];
  },
  addCommands() {
    return {
      toggleStyledBlock:
        (style) =>
        ({ editor, commands, state, tr, dispatch }) => {
          if (!editor.isActive(this.name, { style })) return commands.wrapIn(this.name, { style });
          if (commands.lift(this.name)) return true;
          // Selections that cover the whole block (e.g. Select All) cannot be
          // lifted; unwrap every matching block in the selection instead.
          const { from, to } = state.selection;
          const hits: { pos: number; size: number; content: typeof state.doc.content }[] = [];
          state.doc.nodesBetween(from, to, (node, pos) => {
            if (node.type.name === this.name && node.attrs.style === style) {
              hits.push({ pos, size: node.nodeSize, content: node.content });
              return false;
            }
            return true;
          });
          if (hits.length === 0) return false;
          if (dispatch) {
            for (const h of hits.reverse()) tr.replaceWith(h.pos, h.pos + h.size, h.content);
          }
          return true;
        },
    };
  },
});

export const SmallCaps = Mark.create({
  name: "smallCaps",
  parseHTML() {
    return [{ tag: "span.sc" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: "sc" }), 0];
  },
  addCommands() {
    return {
      toggleSmallCaps:
        () =>
        ({ commands }) =>
          commands.toggleMark(this.name),
    };
  },
  addKeyboardShortcuts() {
    return { "Mod-Shift-c": () => this.editor.commands.toggleSmallCaps() };
  },
});

export const NoBreak = Mark.create({
  name: "noBreak",
  parseHTML() {
    return [{ tag: "span.nobreak" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: "nobreak" }), 0];
  },
  addCommands() {
    return {
      toggleNoBreak:
        () =>
        ({ commands }) =>
          commands.toggleMark(this.name),
    };
  },
});

/** Lists remember whether they are tight or loose so Markdown round-trips. */
export const ListTightness = Extension.create({
  name: "listTightness",
  addGlobalAttributes() {
    return [
      {
        types: ["bulletList", "orderedList"],
        attributes: {
          tight: {
            default: true,
            parseHTML: (el) => el.getAttribute("data-tight") !== "false",
            renderHTML: (attrs) => ({ "data-tight": attrs.tight ? "true" : "false" }),
          },
        },
      },
    ];
  },
});

/**
 * Footnote reference: an inline atom whose `text` attribute holds the
 * footnote's content as inline Markdown (the Rust bridge parses it). The
 * number is a CSS counter, so it is never stored.
 */
export const Footnote = Node.create({
  name: "footnote",
  group: "inline",
  inline: true,
  atom: true,
  selectable: true,
  addAttributes() {
    return {
      text: {
        default: "",
        parseHTML: (el) => el.getAttribute("data-text") ?? "",
        renderHTML: (attrs) => ({ "data-text": String(attrs.text ?? "") }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "sup.footnote" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["sup", mergeAttributes(HTMLAttributes, { class: "footnote" })];
  },
  renderText() {
    return "";
  },
  addNodeView() {
    return ({ node, getPos }) => {
      const dom = document.createElement("sup");
      dom.className = "footnote";
      dom.contentEditable = "false";
      dom.title = String(node.attrs.text || "Empty footnote");
      dom.setAttribute("role", "button");
      dom.setAttribute("aria-label", "Footnote");
      dom.addEventListener("mousedown", (e) => {
        e.preventDefault();
        const pos = typeof getPos === "function" ? getPos() : undefined;
        if (pos === undefined) return;
        dom.dispatchEvent(new CustomEvent("margin:footnote", { bubbles: true, detail: { pos } }));
      });
      return {
        dom,
        update: (n) => {
          if (n.type.name !== "footnote") return false;
          dom.title = String(n.attrs.text || "Empty footnote");
          return true;
        },
      };
    };
  },
});

/** Inline comment anchor; the body lives in `comments/<docId>.json`. */
export const CommentMark = Mark.create({
  name: "comment",
  inclusive: false,
  excludes: "",
  addAttributes() {
    return {
      id: {
        default: "",
        parseHTML: (el) => el.getAttribute("data-comment") ?? "",
        renderHTML: (attrs) => ({ "data-comment": String(attrs.id ?? "") }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-comment]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: "comment-anchor" }), 0];
  },
});

/** All schema extensions (no UI behaviour), shared by the editor and tests. */
export function schemaExtensions() {
  return [
    StarterKit.configure({
      code: false,
      codeBlock: false,
      strike: false,
      underline: false,
      horizontalRule: false,
      heading: { levels: [1, 2, 3, 4, 5, 6] },
      link: { openOnClick: false, autolink: false, linkOnPaste: false },
      trailingNode: false,
    }),
    SceneBreak,
    StyledBlock,
    SmallCaps,
    NoBreak,
    ListTightness,
    Footnote,
    CommentMark,
  ];
}

/**
 * Leaf text used for word counts: footnotes are invisible in the count
 * (the backend treats them as part of the adjacent word), line breaks split.
 */
export function wordLeafText(node: { type: { name: string } }): string {
  return node.type.name === "footnote" ? "" : " ";
}

/**
 * Leaf text that keeps positions aligned with backend offsets: a footnote is
 * U+FFFC and a hard break is a newline, each one character wide, like the
 * Rust dialect's plain text.
 */
export function offsetLeafText(node: { type: { name: string } }): string {
  return node.type.name === "footnote" ? "￼" : "\n";
}

/** Word count matching the backend (`dialect::count_words`). */
export function countWords(text: string): number {
  let n = 0;
  for (const tok of text.split(/[\s—–]+/u)) {
    if (/[\p{L}\p{N}]/u.test(tok)) n++;
  }
  return n;
}
