import { MARGIN_PANEL, ai, defineModule, editorHost, sdkVersion, useWorkbench, workbench } from "@margin/sdk";

import { AskTab } from "./AskTab";
import { IdeasTab } from "./IdeasTab";
import { MarginPanel } from "./MarginPanel";
import { PhrasingTab } from "./PhrasingTab";
import { autoDue, autoIdeas, loadPinned, requestIdeas, requestPhrasing, useMargin } from "./store";

const IDLE_MS = 20_000;

function show(tab: string) {
  useMargin.setState({ tab });
  workbench.showPanel(MARGIN_PANEL);
}

/**
 * Ideas on their own (SPEC §8.1): at the end of a paragraph, after 20 s without
 * typing, only while the panel is open on the Ideas tab. The panel never opens itself.
 */
function startAutoIdeas(): () => void {
  let lastKey = Date.now();
  const onKey = () => {
    lastKey = Date.now();
  };
  window.addEventListener("keydown", onKey, true);
  const timer = setInterval(() => {
    const sb = useWorkbench.getState().sidebars.right;
    if (!(sb.visible && sb.active === MARGIN_PANEL)) return;
    if (useMargin.getState().tab !== "margin.ideas" || !autoIdeas() || !ai.isReady()) return;
    if (Date.now() - lastKey < IDLE_MS) return;
    const h = editorHost.active();
    if (!h) return;
    const ctx = h.selectionContext();
    if (ctx.text) return;
    // At the end of a paragraph: nothing but a line break (or the end) follows the cursor.
    if (ctx.after.trim() !== "" && !ctx.after.startsWith("\n")) return;
    if (autoDue(h.textBeforeCursor())) void requestIdeas(true);
  }, 5000);
  void loadPinned();
  return () => {
    window.removeEventListener("keydown", onKey, true);
    clearInterval(timer);
  };
}

export default defineModule({
  id: "margin",
  name: "Margin panel",
  description: "A quiet side panel: ideas, questions about your book, and alternative phrasings. Never edits your text.",
  sdkVersion,
  activate: startAutoIdeas,
  contributes: {
    sidebarPanels: [{ id: MARGIN_PANEL, title: "Margin", side: "right", order: -10, component: MarginPanel }],
    marginTabs: [
      { id: "margin.ideas", title: "Ideas", order: 0, component: IdeasTab },
      { id: "margin.ask", title: "Ask my book", order: 1, component: AskTab },
      { id: "margin.phrasing", title: "Phrasing", order: 2, component: PhrasingTab },
    ],
    commands: [
      {
        id: "margin.toggle",
        title: "Show or hide the Margin panel",
        category: "View",
        keybinding: "Ctrl+Shift+M",
        run: () => workbench.togglePanel(MARGIN_PANEL),
      },
      {
        id: "margin.ideas",
        title: "Get ideas",
        category: "Margin",
        keybinding: "Ctrl+Shift+I",
        run: () => {
          show("margin.ideas");
          return requestIdeas();
        },
      },
      {
        id: "margin.ask",
        title: "Ask my book",
        category: "Margin",
        run: () => show("margin.ask"),
      },
      {
        id: "margin.phrasing",
        title: "Rephrase the selection",
        category: "Margin",
        keybinding: "Ctrl+Shift+P",
        run: () => {
          show("margin.phrasing");
          return requestPhrasing();
        },
      },
    ],
  },
});
