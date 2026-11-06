// @vitest-environment jsdom
/**
 * Ghost text invariants (SPEC §7, §17 checks 2–5): the suggestion is a
 * decoration, never document text, never in the undo history, never saved;
 * it never appears where it must not; any key cancels generation; accept is
 * one undo step that removes exactly the accepted text.
 */
import { Editor } from "@tiptap/core";
import { undoDepth } from "@tiptap/pm/history";
import { TextSelection } from "@tiptap/pm/state";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  currentGhost,
  DISMISS_QUIET_MS,
  ghostControl,
  ghostSite,
  GhostText,
  type GhostClient,
  type GhostOptions,
  type GhostRequestArgs,
  type GhostWireState,
} from "./ghost";
import { aiRanges, Provenance } from "./provenance";
import { schemaExtensions } from "./schema";

let editor: Editor | null = null;
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  editor?.destroy();
  editor = null;
  vi.useRealTimers();
});

function fakeClient() {
  const handlers = new Set<(e: { requestId: string; state: GhostWireState; text: string }) => void>();
  const started: GhostRequestArgs[] = [];
  const cancelled: string[] = [];
  const state = { keystrokes: 0 };
  const client: GhostClient = {
    start: async (r) => void started.push(r),
    cancel: async (id) => void cancelled.push(id),
    keystroke: async () => void state.keystrokes++,
    subscribe: (h) => {
      handlers.add(h);
      return () => void handlers.delete(h);
    },
  };
  return {
    client,
    started,
    cancelled,
    state,
    emit: (requestId: string, st: GhostWireState, text = "") => handlers.forEach((h) => h({ requestId, state: st, text })),
  };
}

type Fake = ReturnType<typeof fakeClient>;

const CLOCK = { t: 1_000_000 };

function make(html: string, fake: Fake, over: Partial<GhostOptions> = {}): Editor {
  editor = new Editor({
    extensions: [
      ...schemaExtensions(),
      GhostText.configure({
        docId: "DOC",
        client: fake.client,
        enabled: () => true,
        ready: () => true,
        debounceMs: () => 600,
        length: () => "phrase",
        opacity: () => 0.4,
        italic: () => false,
        neverPush: () => false,
        typography: () => ({ quotes: true, dashes: true, ellipsis: true }),
        focused: () => true,
        now: () => CLOCK.t,
        ...over,
      }),
      Provenance,
    ],
    content: html,
  });
  editor.commands.focus("end");
  return editor;
}

const BODY = "<p>The ferry had gone. She stood at the edge and</p>";
const ghostEl = (ed: Editor) => ed.view.dom.querySelector(".ghost-text");
const docText = (ed: Editor) => ed.state.doc.textBetween(0, ed.state.doc.content.size, "\n\n", "");

/** Type like a keyboard: a key press, then the character. */
function typeText(ed: Editor, text: string): void {
  for (const ch of text) {
    ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: ch, bubbles: true }));
    const { from, to } = ed.state.selection;
    ed.view.dispatch(ed.state.tr.insertText(ch, from, to));
  }
}

function backspace(ed: Editor): void {
  ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Backspace", bubbles: true }));
  const { from } = ed.state.selection;
  ed.view.dispatch(ed.state.tr.delete(from - 1, from));
}

/** Ask for a suggestion and answer it, as the backend would. */
function suggest(ed: Editor, fake: Fake, text: string): string {
  const n = fake.started.length;
  ghostControl(ed.view)!.request(0, true);
  expect(fake.started.length).toBe(n + 1);
  const id = fake.started[n]!.requestId;
  fake.emit(id, "done", text);
  return id;
}

describe("a suggestion is a decoration only (SPEC §17 checks 4 and 5)", () => {
  it("is not in the document, the undo history, the word count or the saved content", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    const json = JSON.stringify(ed.getJSON());
    const html = ed.getHTML();
    suggest(ed, fake, " step back into the rain");
    expect(ghostEl(ed)?.textContent).toBe(" step back into the rain");
    expect(JSON.stringify(ed.getJSON())).toBe(json);
    expect(ed.getHTML()).toBe(html);
    expect(ed.getText()).not.toContain("rain");
    expect(docText(ed)).not.toContain("rain");
    expect(undoDepth(ed.state)).toBe(0);
    expect(aiRanges(ed.state)).toEqual([]);
  });

  it("marks the widget hidden from assistive tech and from the caret", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " and waited");
    const el = ghostEl(ed)!;
    expect(el.getAttribute("aria-hidden")).toBe("true");
    expect(el.getAttribute("contenteditable")).toBe("false");
  });

  it("shows streamed text as it grows and replaces it with the final text", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[0]!.requestId;
    fake.emit(id, "streaming", " step");
    expect(ghostEl(ed)?.textContent).toBe(" step");
    fake.emit(id, "streaming", " step back");
    expect(ghostEl(ed)?.textContent).toBe(" step back");
    fake.emit(id, "done", " step back into");
    expect(ghostEl(ed)?.textContent).toBe(" step back into");
    expect(undoDepth(ed.state)).toBe(0);
  });

  it("is clipped to the line when never-push is on, and unclipped when off", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake, { neverPush: () => true });
    suggest(ed, fake, " and waited");
    expect(ghostEl(ed)?.classList.contains("ghost-text--clip")).toBe(true);
    ed.destroy();
    const fake2 = fakeClient();
    const ed2 = make(BODY, fake2, { neverPush: () => false, italic: () => true });
    suggest(ed2, fake2, " and waited");
    expect(ghostEl(ed2)?.classList.contains("ghost-text--clip")).toBe(false);
    expect(ghostEl(ed2)?.classList.contains("ghost-text--italic")).toBe(true);
  });
});

describe("accepting (SPEC §17 check 4)", () => {
  it("inserts the text, clears the suggestion, and undo removes exactly that text", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    const before = docText(ed);
    suggest(ed, fake, " step back into the rain");
    expect(ghostControl(ed.view)!.accept("all")).toBe(true);
    expect(docText(ed)).toBe(before + " step back into the rain");
    expect(currentGhost(ed.state)).toBeNull();
    expect(ghostEl(ed)).toBeNull();
    // The cursor sits after the accepted text.
    expect(ed.state.selection.head).toBe(ed.state.doc.content.size - 1);
    expect(undoDepth(ed.state)).toBe(1);
    ed.commands.undo();
    expect(docText(ed)).toBe(before);
    expect(aiRanges(ed.state)).toEqual([]);
  });

  it("is its own undo step even right after typing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    typeText(ed, " the");
    const typed = docText(ed);
    // Past the moment a key press could still be in flight, but inside the history
    // group window (500 ms): without closing the group, undo would take the typing too.
    vi.advanceTimersByTime(200);
    suggest(ed, fake, " water");
    ghostControl(ed.view)!.accept("all");
    ed.commands.undo();
    expect(docText(ed)).toBe(typed);
  });

  it("records the accepted text as AI-assisted, and only that", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back into the rain");
    ghostControl(ed.view)!.accept("all");
    const marked = aiRanges(ed.state).map(([a, b]) => ed.state.doc.textBetween(a, b));
    expect(marked).toEqual([" step back into the rain"]);
    // Typing after it does not extend it; retyping inside it removes the retyped letters.
    typeText(ed, "!");
    expect(aiRanges(ed.state).map(([a, b]) => ed.state.doc.textBetween(a, b))).toEqual([" step back into the rain"]);
  });

  it("accepts one word at a time and keeps the rest showing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    const before = docText(ed);
    suggest(ed, fake, " step back into the rain");
    const control = ghostControl(ed.view)!;
    control.accept("word");
    expect(docText(ed)).toBe(before + " step");
    expect(ghostEl(ed)?.textContent).toBe(" back into the rain");
    control.accept("word");
    expect(docText(ed)).toBe(before + " step back");
    expect(ghostEl(ed)?.textContent).toBe(" into the rain");
    // Each word is an undo step of its own.
    expect(undoDepth(ed.state)).toBe(2);
    ed.commands.undo();
    expect(docText(ed)).toBe(before + " step");
    // Accepting the last word clears the suggestion.
    suggest(ed, fake, " alone");
    control.accept("word");
    expect(currentGhost(ed.state)).toBeNull();
  });

  it("does nothing when no suggestion is showing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    const before = docText(ed);
    expect(ghostControl(ed.view)!.accept("all")).toBe(false);
    expect(docText(ed)).toBe(before);
  });

  it("reports the accepted text so the session can count it", () => {
    const fake = fakeClient();
    const accepted: string[] = [];
    const ed = make(BODY, fake, { onAccept: (a) => accepted.push(a.text) });
    suggest(ed, fake, " step back");
    ghostControl(ed.view)!.accept("word");
    ghostControl(ed.view)!.accept("word");
    expect(accepted).toEqual([" step", " back"]);
  });
});

describe("keeping typing and moving (SPEC §7.2)", () => {
  it("shrinks while the typed characters match the start of the suggestion", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    typeText(ed, " ");
    expect(ghostEl(ed)?.textContent).toBe("step back");
    typeText(ed, "ste");
    expect(ghostEl(ed)?.textContent).toBe("p back");
    expect(currentGhost(ed.state)?.pos).toBe(ed.state.selection.head);
  });

  it("is dropped as soon as the typing differs", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    typeText(ed, " sx");
    expect(currentGhost(ed.state)).toBeNull();
    expect(ghostEl(ed)).toBeNull();
  });

  it("is dropped when the whole suggestion has been typed", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " go");
    typeText(ed, " go");
    expect(currentGhost(ed.state)).toBeNull();
  });

  it("is dismissed by a real Escape key press in the editor", () => {
    // ProseMirror swallows Escape, so the plugin has to take it before the app keymap can.
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    const down = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    ed.view.dom.dispatchEvent(down);
    expect(currentGhost(ed.state)).toBeNull();
    expect(ghostEl(ed)).toBeNull();
    expect(down.defaultPrevented).toBe(true);
    expect(docText(ed)).toBe("The ferry had gone. She stood at the edge and");
    // With nothing showing, Escape is left alone.
    const again = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    ed.view.dom.dispatchEvent(again);
    expect(fake.started).toHaveLength(1);
  });

  it("goes away when the editor loses the keyboard, and so does a request in flight", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    ed.view.dom.dispatchEvent(new FocusEvent("blur"));
    expect(currentGhost(ed.state)).toBeNull();
    expect(ghostEl(ed)).toBeNull();
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[1]!.requestId;
    ed.view.dom.dispatchEvent(new FocusEvent("blur"));
    expect(fake.cancelled).toContain(id);
    fake.emit(id, "done", " step");
    expect(ghostEl(ed)).toBeNull();
  });

  it("is dropped when the cursor moves", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    ed.view.dispatch(ed.state.tr.setSelection(TextSelection.create(ed.state.doc, 3)));
    expect(currentGhost(ed.state)).toBeNull();
  });
});

describe("where a suggestion may appear (SPEC §17 check 2)", () => {
  const site = (html: string, place: (ed: Editor) => void = (ed) => void ed.commands.focus("end")) => {
    const ed = make(html, fakeClient());
    place(ed);
    return ghostSite(ed.state);
  };

  it("appears at the end of a body paragraph with enough words", () => {
    const s = site(BODY);
    expect(s?.textBefore).toBe("The ferry had gone. She stood at the edge and");
  });

  it("never appears with a selection", () => {
    expect(site(BODY, (ed) => void ed.commands.setTextSelection({ from: 5, to: 9 }))).toBeNull();
  });

  it("never appears in a heading", () => {
    expect(site("<h2>The ferry had already gone</h2>")).toBeNull();
  });

  it("never appears in a block quote, list or styled block", () => {
    expect(site("<blockquote><p>The ferry had already gone</p></blockquote>")).toBeNull();
    expect(site("<ul><li><p>The ferry had already gone</p></li></ul>")).toBeNull();
  });

  it("needs the cursor at the end of the paragraph", () => {
    expect(site(BODY, (ed) => void ed.commands.setTextSelection(10))).toBeNull();
  });

  it("needs three words in the current or previous paragraph", () => {
    expect(site("<p>Too short</p>")).toBeNull();
    // A short paragraph after a long one is fine (the previous paragraph counts).
    expect(site("<p>The ferry had already gone.</p><p>Nobody</p>")).not.toBeNull();
    expect(site("<p>Gone.</p><p>Nobody</p>")).toBeNull();
  });

  it("joins the paragraphs before the cursor with a blank line", () => {
    const s = site("<p>First paragraph here.</p><p>Second one is longer</p>");
    expect(s?.textBefore).toBe("First paragraph here.\n\nSecond one is longer");
  });
});

describe("triggering (SPEC §7.1)", () => {
  it("asks once the typing pause has passed, and not before", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    typeText(ed, " the");
    vi.advanceTimersByTime(599);
    expect(fake.started).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(fake.started).toHaveLength(1);
    const req = fake.started[0]!;
    expect(req.docId).toBe("DOC");
    expect(req.length).toBe("phrase");
    expect(req.variant).toBe(0);
    expect(req.textBefore.endsWith("edge and the")).toBe(true);
    expect(req.typography).toEqual({ quotes: true, dashes: true, ellipsis: true });
  });

  it("restarts the pause with every character", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    typeText(ed, " t");
    vi.advanceTimersByTime(500);
    typeText(ed, "h");
    vi.advanceTimersByTime(500);
    expect(fake.started).toHaveLength(0);
    vi.advanceTimersByTime(100);
    expect(fake.started).toHaveLength(1);
  });

  it("does not ask during a backspace streak, and asks again after typing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    backspace(ed);
    backspace(ed);
    vi.advanceTimersByTime(2000);
    expect(fake.started).toHaveLength(0);
    typeText(ed, "d");
    vi.advanceTimersByTime(600);
    expect(fake.started).toHaveLength(1);
  });

  it("does not ask when ghost text is off, in Flow mode, or while the model is not ready", () => {
    for (const over of [{ enabled: () => false }, { ready: () => false }]) {
      const fake = fakeClient();
      const ed = make(BODY, fake, over);
      typeText(ed, " the");
      vi.advanceTimersByTime(3000);
      expect(fake.started).toHaveLength(0);
      ed.destroy();
    }
  });

  it("does not ask when the editor does not have the keyboard, or during composition", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake, { focused: () => false });
    typeText(ed, " the");
    vi.advanceTimersByTime(3000);
    expect(fake.started).toHaveLength(0);
  });

  it("honours the configured pause, within 250–2000 ms", () => {
    for (const [asked, expected] of [[100, 250], [5000, 2000], [900, 900]] as const) {
      const fake = fakeClient();
      const ed = make(BODY, fake, { debounceMs: () => asked });
      typeText(ed, " the");
      vi.advanceTimersByTime(expected - 1);
      expect(fake.started).toHaveLength(0);
      vi.advanceTimersByTime(1);
      expect(fake.started).toHaveLength(1);
      ed.destroy();
    }
  });

  it("does not ask while a suggestion is already showing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    ghostControl(ed.view)!.request(0, false);
    expect(fake.started).toHaveLength(1);
  });

  it("stays quiet for 1.5 s after a dismiss at the same spot (SPEC §17 check 2)", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back");
    expect(ghostControl(ed.view)!.dismiss()).toBe(true);
    expect(currentGhost(ed.state)).toBeNull();
    const control = ghostControl(ed.view)!;
    CLOCK.t += DISMISS_QUIET_MS - 1;
    control.request(0, false);
    expect(fake.started).toHaveLength(1);
    CLOCK.t += 1;
    control.request(0, false);
    expect(fake.started).toHaveLength(2);
  });

  it("a dismiss does not silence other places", () => {
    const fake = fakeClient();
    const ed = make("<p>The ferry had gone and the light faded.</p><p>She turned away from it</p>", fake);
    suggest(ed, fake, " step back");
    ghostControl(ed.view)!.dismiss();
    typeText(ed, " now");
    vi.advanceTimersByTime(600);
    expect(fake.started).toHaveLength(2);
  });
});

describe("every key cancels generation (SPEC §2.2, §17 check 3)", () => {
  it("a key press cancels the request in flight and tells the scheduler", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[0]!.requestId;
    expect(fake.state.keystrokes).toBe(0);
    ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
    expect(fake.state.keystrokes).toBe(1);
    expect(fake.cancelled).toEqual([id]);
    // Anything the backend still sends for it is ignored.
    fake.emit(id, "streaming", " step");
    fake.emit(id, "done", " step back");
    expect(ghostEl(ed)).toBeNull();
  });

  it("counts every character, not just the first", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    typeText(ed, " the end");
    expect(fake.state.keystrokes).toBe(8);
  });

  it("modifier keys alone are not keystrokes", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    for (const key of ["Shift", "Control", "Alt", "Meta"]) {
      ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    }
    expect(fake.state.keystrokes).toBe(0);
  });

  it("an edit made without a key event still makes an in-flight answer stale", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[0]!.requestId;
    ed.view.dispatch(ed.state.tr.insertText("x", ed.state.selection.head));
    expect(fake.cancelled).toContain(id);
    fake.emit(id, "done", " step");
    expect(ghostEl(ed)).toBeNull();
  });

  it("a cursor move makes an in-flight answer stale", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[0]!.requestId;
    ed.view.dispatch(ed.state.tr.setSelection(TextSelection.create(ed.state.doc, 4)));
    expect(fake.cancelled).toContain(id);
    fake.emit(id, "done", " step");
    expect(currentGhost(ed.state)).toBeNull();
  });

  it("ignores answers to requests it did not ask for", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    fake.emit("someone-else", "done", " step");
    expect(currentGhost(ed.state)).toBeNull();
  });

  it("an empty or failed answer shows nothing, and takes back what streamed", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    const id = fake.started[0]!.requestId;
    fake.emit(id, "streaming", " step");
    expect(ghostEl(ed)).not.toBeNull();
    fake.emit(id, "empty");
    expect(ghostEl(ed)).toBeNull();
    ghostControl(ed.view)!.request(0, true);
    fake.emit(fake.started[1]!.requestId, "failed");
    expect(ghostEl(ed)).toBeNull();
  });

  it("a newer request cancels the older one", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    const control = ghostControl(ed.view)!;
    control.request(0, true);
    const first = fake.started[0]!.requestId;
    control.request(0, true);
    expect(fake.cancelled).toContain(first);
    expect(fake.started[1]!.requestId).not.toBe(first);
  });
});

describe("a request never overtakes the key press before it", () => {
  /** A client whose keystroke notification stays in flight until released. */
  function slowKeystroke() {
    const fake = fakeClient();
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => (release = r));
    fake.client.keystroke = async () => {
      fake.state.keystrokes++;
      await gate;
    };
    return { fake, release };
  }

  it("holds a request made right after a key until that key has been delivered", async () => {
    vi.useRealTimers();
    const { fake, release } = slowKeystroke();
    const ed = make(BODY, fake);
    suggest(ed, fake, " first");
    // Alt+]: the key goes down (notification in flight), then the alternative is asked for.
    ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "]", altKey: true, bubbles: true }));
    ghostControl(ed.view)!.cycle(1);
    expect(fake.started).toHaveLength(1);
    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(fake.started).toHaveLength(2);
    expect(fake.started[1]!.variant).toBe(1);
    // And the answer is shown as usual.
    fake.emit(fake.started[1]!.requestId, "done", " second");
    expect(ghostEl(ed)?.textContent).toBe(" second");
  });

  it("does not start a held request that was superseded while it waited", async () => {
    vi.useRealTimers();
    const { fake, release } = slowKeystroke();
    const ed = make(BODY, fake);
    suggest(ed, fake, " first");
    ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "]", altKey: true, bubbles: true }));
    ghostControl(ed.view)!.cycle(1);
    // The author types before the notification is delivered.
    ed.view.dom.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(fake.started).toHaveLength(1);
  });

  it("sends at once when no key press is in flight", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.request(0, true);
    expect(fake.started).toHaveLength(1);
  });
});

describe("alternatives (SPEC §7.2)", () => {
  it("fetches the next one lazily, up to three, and steps back and forth", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " first");
    const control = ghostControl(ed.view)!;
    control.cycle(1);
    expect(fake.started).toHaveLength(2);
    expect(fake.started[1]!.variant).toBe(1);
    fake.emit(fake.started[1]!.requestId, "done", " second");
    expect(ghostEl(ed)?.textContent).toBe(" second");
    control.cycle(1);
    expect(fake.started[2]!.variant).toBe(2);
    fake.emit(fake.started[2]!.requestId, "done", " third");
    expect(ghostEl(ed)?.textContent).toBe(" third");
    // No fourth.
    control.cycle(1);
    expect(fake.started).toHaveLength(3);
    expect(ghostEl(ed)?.textContent).toBe(" third");
    // Back through the ones already fetched, without asking again.
    control.cycle(-1);
    expect(ghostEl(ed)?.textContent).toBe(" second");
    control.cycle(-1);
    expect(ghostEl(ed)?.textContent).toBe(" first");
    control.cycle(-1);
    expect(ghostEl(ed)?.textContent).toBe(" first");
    control.cycle(1);
    expect(ghostEl(ed)?.textContent).toBe(" second");
    expect(fake.started).toHaveLength(3);
    // Accepting takes the one that is showing.
    control.accept("all");
    expect(docText(ed).endsWith("edge and second")).toBe(true);
  });

  it("keeps the current suggestion when the alternative repeats it", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " first");
    ghostControl(ed.view)!.cycle(1);
    fake.emit(fake.started[1]!.requestId, "done", " first");
    expect(ghostEl(ed)?.textContent).toBe(" first");
  });

  it("tries once more when an alternative repeats, then gives up quietly", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " first");
    ghostControl(ed.view)!.cycle(1);
    fake.emit(fake.started[1]!.requestId, "done", " first");
    // One automatic retry, for the same alternative.
    expect(fake.started).toHaveLength(3);
    expect(fake.started[2]!.variant).toBe(1);
    expect(ghostEl(ed)?.textContent).toBe(" first");
    fake.emit(fake.started[2]!.requestId, "done", " first");
    expect(fake.started).toHaveLength(3);
    expect(ghostEl(ed)?.textContent).toBe(" first");
    // A retry that differs is shown.
    ghostControl(ed.view)!.cycle(1);
    fake.emit(fake.started[3]!.requestId, "done", " first");
    fake.emit(fake.started[4]!.requestId, "done", " other");
    expect(ghostEl(ed)?.textContent).toBe(" other");
  });

  it("asks for a suggestion when none is showing", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    ghostControl(ed.view)!.cycle(1);
    expect(fake.started).toHaveLength(1);
    expect(fake.started[0]!.variant).toBe(0);
    ghostControl(ed.view)!.cycle(-1);
    expect(fake.started).toHaveLength(1);
  });
});

describe("a plain document stays plain (SPEC §17 check 5)", () => {
  it("the serialised document never contains a suggestion or provenance, before or after accepting", () => {
    const fake = fakeClient();
    const ed = make(BODY, fake);
    suggest(ed, fake, " step back into the rain");
    expect(JSON.stringify(ed.getJSON())).not.toContain("rain");
    ghostControl(ed.view)!.accept("word");
    const saved = JSON.stringify(ed.getJSON());
    expect(saved).toContain("step");
    expect(saved).not.toMatch(/ghost|provenance|ai-assisted|ghostAccept/i);
    expect(ed.getHTML()).not.toMatch(/ghost|ai-assisted/i);
  });
});
