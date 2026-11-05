import { describe, expect, it } from "vitest";

import { dispatch } from "../../sdk/events";
import { buildKeymap, normalizeChord, resolve } from "../../app/keymap";
import aiModule from "./module";
import { STATUS_LABEL, startAi, useAi } from "./store";

describe("AI status", () => {
  it("follows ai.status events and stops listening when the module is deactivated", () => {
    const stop = startAi();
    dispatch({ v: 1, seq: 1, event: { type: "ai.status", status: "ready", detail: "Ready" } });
    expect(useAi.getState().status).toBe("ready");
    dispatch({ v: 1, seq: 2, event: { type: "ai.status", status: "thinking", detail: "Working" } });
    expect(useAi.getState().status).toBe("thinking");
    stop();
    dispatch({ v: 1, seq: 3, event: { type: "ai.status", status: "off", detail: "AI is turned off" } });
    expect(useAi.getState().status).toBe("thinking");
  });

  it("labels every state the status bar can show (SPEC §6.7)", () => {
    expect(Object.values(STATUS_LABEL).sort()).toEqual(
      ["Off", "Paused (typing)", "Ready", "Starting", "Thinking", "Model not running"].sort(),
    );
  });
});

describe("AI module", () => {
  it("binds the global kill switch to Ctrl+Shift+Space (SPEC §2.10)", () => {
    const commands = aiModule.contributes?.commands ?? [];
    const toggle = commands.find((c) => c.id === "ai.toggle");
    expect(toggle?.keybinding).toBe("Ctrl+Shift+Space");
    const map = buildKeymap(commands);
    const ev = new KeyboardEvent("keydown", { key: " ", code: "Space", ctrlKey: true, shiftKey: true, cancelable: true });
    expect(normalizeChord("Ctrl+Shift+Space")).toBe("ctrl+shift+space");
    expect(resolve(map, ev)?.id).toBe("ai.toggle");
  });

  it("puts one quiet indicator in the status bar and the Models and Bake-off pages in settings", () => {
    expect(aiModule.contributes?.statusBarItems).toHaveLength(1);
    expect(aiModule.contributes?.settingsPages?.map((p) => p.id)).toEqual(["models", "bakeoff"]);
  });
});
