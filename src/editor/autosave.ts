/**
 * Autosave and recovery-journal scheduling for one open document
 * (SPEC §11.3). Pure logic with injected IO so it can be tested with fake
 * timers.
 *
 * - The file is saved `idleMs` after typing stops, on blur, on tab close,
 *   and at least every `maxDirtyMs` during continuous typing.
 * - While dirty, a recovery-journal entry is written at most every
 *   `journalMs`, so a crash loses at most that much typing (SPEC §14: ≤ 2 s).
 * - Content is captured lazily (only when a timer fires), never per keystroke.
 */
import type { PmNode, SaveResult } from "../sdk/bindings";

export type SaveStatus = "saved" | "dirty" | "saving" | "conflict" | "error";

export interface SaveState {
  status: SaveStatus;
  error?: string;
  lastSavedAt?: number;
}

export interface AutosaveDeps {
  getContent(): PmNode;
  save(content: PmNode, force: boolean): Promise<SaveResult>;
  journal(content: PmNode): Promise<unknown>;
  onState(state: SaveState): void;
  onSaved?(result: SaveResult): void;
  /** Distinguishes a conflict error from other failures. */
  isConflict(err: unknown): boolean;
}

export interface AutosaveOptions {
  idleMs: number;
  journalMs: number;
  maxDirtyMs: number;
}

export const DEFAULT_AUTOSAVE: AutosaveOptions = { idleMs: 2000, journalMs: 750, maxDirtyMs: 30_000 };

export class AutosaveController {
  private version = 0;
  private savedVersion = 0;
  private journaledVersion = 0;
  private dirtySince: number | null = null;
  private idleTimer: ReturnType<typeof setTimeout> | undefined;
  private journalTimer: ReturnType<typeof setTimeout> | undefined;
  private inFlight: Promise<void> | null = null;
  private state: SaveState = { status: "saved" };
  private disposed = false;

  constructor(
    private readonly deps: AutosaveDeps,
    private readonly opts: AutosaveOptions = DEFAULT_AUTOSAVE,
  ) {}

  get isDirty(): boolean {
    return this.version !== this.savedVersion;
  }

  get current(): SaveState {
    return this.state;
  }

  private setState(s: SaveState): void {
    this.state = s;
    this.deps.onState(s);
  }

  /** Call on every document change. Cheap: only (re)arms timers. */
  markDirty(): void {
    if (this.disposed) return;
    this.version++;
    const now = Date.now();
    if (this.dirtySince === null) this.dirtySince = now;
    if (this.state.status === "saved" || this.state.status === "error") {
      this.setState({ ...this.state, status: "dirty" });
    }
    clearTimeout(this.idleTimer);
    if (this.state.status === "conflict") return; // wait for the author's decision
    const overdue = now - this.dirtySince >= this.opts.maxDirtyMs;
    this.idleTimer = setTimeout(() => void this.flush(), overdue ? 0 : this.opts.idleMs);
    if (this.journalTimer === undefined) {
      this.journalTimer = setTimeout(() => {
        this.journalTimer = undefined;
        void this.writeJournal();
      }, this.opts.journalMs);
    }
  }

  private async writeJournal(): Promise<void> {
    if (this.disposed || !this.isDirty || this.journaledVersion === this.version) return;
    const v = this.version;
    try {
      await this.deps.journal(this.deps.getContent());
      this.journaledVersion = v;
    } catch (err) {
      console.warn("[margin] recovery journal write failed", err);
    }
  }

  /** Save now if dirty. Resolves when the file is written (or the save failed). */
  async flush(force = false): Promise<void> {
    clearTimeout(this.idleTimer);
    if (this.inFlight) {
      await this.inFlight;
      if (!this.isDirty) return;
    }
    if (!this.isDirty || (this.state.status === "conflict" && !force)) return;
    const v = this.version;
    const content = this.deps.getContent();
    this.setState({ ...this.state, status: "saving" });
    this.inFlight = (async () => {
      try {
        const res = await this.deps.save(content, force);
        this.savedVersion = v;
        this.deps.onSaved?.(res);
        if (this.version === v) {
          this.dirtySince = null;
          this.setState({ status: "saved", lastSavedAt: Date.now() });
        } else {
          // Typed while saving: stay dirty and save again after the next pause.
          this.dirtySince = Date.now();
          this.setState({ status: "dirty", lastSavedAt: Date.now() });
          clearTimeout(this.idleTimer);
          this.idleTimer = setTimeout(() => void this.flush(), this.opts.idleMs);
        }
      } catch (err) {
        if (this.deps.isConflict(err)) {
          this.setState({ ...this.state, status: "conflict", error: String((err as Error).message ?? err) });
        } else {
          this.setState({ ...this.state, status: "error", error: String((err as Error).message ?? err) });
          // Retry later; the journal keeps the text safe meanwhile.
          clearTimeout(this.idleTimer);
          this.idleTimer = setTimeout(() => void this.flush(), Math.max(this.opts.idleMs, 5000));
        }
      } finally {
        this.inFlight = null;
      }
    })();
    await this.inFlight;
  }

  /** The author chose to keep their version over an external change. */
  async keepMine(): Promise<void> {
    this.setState({ ...this.state, status: "dirty" });
    await this.flush(true);
  }

  /** The editor content was replaced from disk; nothing is unsaved. */
  markClean(): void {
    clearTimeout(this.idleTimer);
    this.savedVersion = this.version;
    this.journaledVersion = this.version;
    this.dirtySince = null;
    this.setState({ status: "saved", lastSavedAt: Date.now() });
  }

  dispose(): void {
    this.disposed = true;
    clearTimeout(this.idleTimer);
    clearTimeout(this.journalTimer);
  }
}
