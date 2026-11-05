/**
 * Smart typography (SPEC §11.2): curly quotes, `--` → em dash, `...` →
 * ellipsis. Each rule can be turned off. Backspace or Ctrl+Z right after a
 * replacement restores the literal characters (TipTap's `undoInputRule`).
 */
import { Extension, InputRule } from "@tiptap/core";

export interface TypographyOptions {
  /** Read at input time, so settings changes apply immediately. */
  enabled: () => { quotes: boolean; dashes: boolean; ellipsis: boolean };
}

/** Characters after which a quote opens rather than closes. */
const OPENS_AFTER = /[\s([{—–“‘-]/u;

function charBefore(state: { doc: { textBetween(a: number, b: number, s?: string, l?: string): string } }, pos: number): string {
  if (pos <= 1) return "";
  return state.doc.textBetween(pos - 1, pos, "\n", "\n");
}

export function quoteFor(kind: "double" | "single", before: string): string {
  const open = before === "" || before === "\n" || OPENS_AFTER.test(before);
  if (kind === "double") return open ? "“" : "”";
  // Apostrophes inside words and after letters close; elsewhere a single quote opens.
  return open ? "‘" : "’";
}

export const SmartTypography = Extension.create<TypographyOptions>({
  name: "smartTypography",
  addOptions() {
    return { enabled: () => ({ quotes: true, dashes: true, ellipsis: true }) };
  },
  addInputRules() {
    const opts = this.options;
    const rule = (find: RegExp, which: "quotes" | "dashes" | "ellipsis", replace: (before: string) => string) =>
      new InputRule({
        find,
        handler: ({ state, range }) => {
          if (!opts.enabled()[which]) return null;
          const $from = state.doc.resolve(range.from);
          // Never inside code-like or atom contexts; all our text blocks qualify.
          if (!$from.parent.isTextblock) return null;
          const before = charBefore(state, range.from);
          state.tr.insertText(replace(before), range.from, range.to);
        },
      });
    return [
      rule(/"$/, "quotes", (b) => quoteFor("double", b)),
      rule(/'$/, "quotes", (b) => quoteFor("single", b)),
      rule(/--$/, "dashes", () => "—"),
      rule(/\.\.\.$/, "ellipsis", () => "…"),
    ];
  },
  addKeyboardShortcuts() {
    // Undo right after an automatic replacement restores the typed characters.
    return { "Mod-z": () => this.editor.commands.undoInputRule() };
  },
});
