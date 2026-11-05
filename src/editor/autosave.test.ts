import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PmNode } from "../sdk/bindings";
import { AutosaveController, type AutosaveDeps, type SaveState } from "./autosave";

class ConflictError extends Error {}

function setup(overrides: Partial<AutosaveDeps> = {}) {
  const journal = vi.fn(async () => null);
  let n = 0;
  const states: SaveState[] = [];
  const deps = {
    getContent: vi.fn((): PmNode => ({ type: "doc", attrs: null, content: null, text: `v${++n}`, marks: null })),
    save: vi.fn(async () => ({ revision: "r", wordCount: 1 })),
    journal,
    onState: (s: SaveState) => states.push(s),
    isConflict: (e: unknown) => e instanceof ConflictError,
    ...overrides,
  };
  const ctl = new AutosaveController(deps, { idleMs: 2000, journalMs: 750, maxDirtyMs: 30_000 });
  return { ctl, deps, states, journal };
}

describe("AutosaveController", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("does not capture content per keystroke", () => {
    const { ctl, deps } = setup();
    for (let i = 0; i < 50; i++) ctl.markDirty();
    expect(deps.getContent).not.toHaveBeenCalled();
  });

  it("journals within 750 ms and saves 2 s after typing stops", async () => {
    const { ctl, deps, states } = setup();
    ctl.markDirty();
    expect(states.at(-1)?.status).toBe("dirty");
    await vi.advanceTimersByTimeAsync(750);
    expect(deps.journal).toHaveBeenCalledTimes(1);
    expect(deps.save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1250);
    expect(deps.save).toHaveBeenCalledTimes(1);
    expect(states.at(-1)?.status).toBe("saved");
    expect(ctl.isDirty).toBe(false);
  });

  it("keeps journaling during continuous typing (crash loses < 1 s)", async () => {
    const { ctl, deps, journal } = setup();
    // Type every 100 ms for 5 s: no save, but a journal entry at least every 750 ms.
    for (let t = 0; t < 5000; t += 100) {
      ctl.markDirty();
      await vi.advanceTimersByTimeAsync(100);
    }
    expect(deps.save).not.toHaveBeenCalled();
    expect(journal.mock.calls.length).toBeGreaterThanOrEqual(6);
  });

  it("saves during long continuous typing", async () => {
    const { ctl, deps } = setup();
    for (let t = 0; t < 31_000; t += 200) {
      ctl.markDirty();
      await vi.advanceTimersByTimeAsync(200);
    }
    expect(deps.save).toHaveBeenCalled();
  });

  it("stays dirty when typing happens during a save", async () => {
    let release: () => void = () => {};
    const save = vi.fn(
      () =>
        new Promise<{ revision: string; wordCount: number }>((resolve) => {
          release = () => resolve({ revision: "r", wordCount: 1 });
        }),
    );
    const { ctl, states } = setup({ save });
    ctl.markDirty();
    const flushing = ctl.flush();
    ctl.markDirty(); // typed while the save was in flight
    release();
    await flushing;
    expect(ctl.isDirty).toBe(true);
    expect(states.at(-1)?.status).toBe("dirty");
    await vi.advanceTimersByTimeAsync(2000);
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(save).toHaveBeenCalledTimes(2);
  });

  it("stops on conflict until the author decides, then force-saves", async () => {
    const save = vi.fn(async (_c: PmNode, force: boolean) => {
      if (!force) throw new ConflictError("changed on disk");
      return { revision: "r", wordCount: 1 };
    });
    const { ctl, states } = setup({ save });
    ctl.markDirty();
    await ctl.flush();
    expect(states.at(-1)?.status).toBe("conflict");
    ctl.markDirty();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(save).toHaveBeenCalledTimes(1);
    await ctl.keepMine();
    expect(save).toHaveBeenLastCalledWith(expect.anything(), true);
    expect(states.at(-1)?.status).toBe("saved");
  });

  it("retries after a failed save", async () => {
    let fail = true;
    const save = vi.fn(async () => {
      if (fail) throw new Error("disk full");
      return { revision: "r", wordCount: 1 };
    });
    const { ctl, states } = setup({ save });
    ctl.markDirty();
    await ctl.flush();
    expect(states.at(-1)?.status).toBe("error");
    fail = false;
    await vi.advanceTimersByTimeAsync(5000);
    expect(states.at(-1)?.status).toBe("saved");
  });

  it("markClean drops pending saves", async () => {
    const { ctl, deps } = setup();
    ctl.markDirty();
    ctl.markClean();
    await vi.advanceTimersByTimeAsync(5000);
    expect(deps.save).not.toHaveBeenCalled();
  });
});
