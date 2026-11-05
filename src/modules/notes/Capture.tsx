/**
 * Quick capture (SPEC §11.4): Ctrl+Shift+N opens a small inline box; Enter
 * appends the thought to the Inbox note and returns to the text. It is a
 * status-bar contribution so it never covers the page.
 */
import { useState } from "react";
import { create } from "zustand";

import { editorHost, story, workbench } from "@margin/sdk";

const useCapture = create<{ open: boolean }>(() => ({ open: false }));

export function openCapture(): void {
  useCapture.setState({ open: true });
}

export function CaptureBox() {
  const open = useCapture((s) => s.open);
  const [text, setText] = useState("");
  if (!open) return null;
  const close = () => {
    useCapture.setState({ open: false });
    setText("");
    editorHost.active()?.focus();
  };
  const send = async () => {
    const t = text.trim();
    if (!t) return close();
    try {
      // Open editors are saved first, so the Inbox reloads without losing anything.
      await editorHost.flushAll();
      await story.notes.capture(t);
      workbench.notify({ message: "Sent to Inbox", timeoutMs: 2500 });
    } catch (e) {
      workbench.notify({ message: `Could not capture: ${e instanceof Error ? e.message : String(e)}` });
    }
    close();
  };
  return (
    <div className="capture-box" role="dialog" aria-label="Quick capture">
      <input
        autoFocus
        value={text}
        placeholder="A thought for the Inbox… (Enter to send, Esc to cancel)"
        aria-label="Thought to capture"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void send();
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            close();
          }
        }}
        onBlur={() => !text.trim() && close()}
      />
    </div>
  );
}
