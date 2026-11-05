/**
 * Scratchpad (SPEC §11.4): a persistent per-project notes pane, stored in
 * `scratchpad.md` and never exported. Saved a moment after typing stops and
 * when it loses focus.
 */
import { useEffect, useRef, useState } from "react";

import { lifecycle, story, useProject, type SidebarPanelProps } from "@margin/sdk";

export function ScratchpadPanel(_props: SidebarPanelProps) {
  const project = useProject();
  const [text, setText] = useState<string | null>(null);
  const dirty = useRef(false);
  const latest = useRef("");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    if (!project) return;
    let live = true;
    story.notes
      .scratchpad()
      .then((t) => {
        if (!live) return;
        latest.current = t;
        setText(t);
      })
      .catch(() => live && setText(""));
    return () => {
      live = false;
    };
  }, [project?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const flush = async () => {
    clearTimeout(timer.current);
    if (!dirty.current) return;
    dirty.current = false;
    await story.notes.saveScratchpad(latest.current).catch(() => {
      dirty.current = true;
    });
  };

  useEffect(() => lifecycle.onBeforeClose(flush), []);
  useEffect(() => () => void flush(), []);

  return (
    <div className="panel scratchpad">
      <div className="panel-toolbar">
        <span className="panel-title">Scratchpad</span>
        <span className="muted small">Never exported</span>
      </div>
      <textarea
        className="scratchpad-text"
        aria-label="Scratchpad"
        placeholder="Loose notes, lists, half-ideas…"
        disabled={text === null}
        value={text ?? ""}
        onChange={(e) => {
          setText(e.target.value);
          latest.current = e.target.value;
          dirty.current = true;
          clearTimeout(timer.current);
          timer.current = setTimeout(() => void flush(), 800);
        }}
        onBlur={() => void flush()}
      />
    </div>
  );
}
