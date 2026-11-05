/**
 * App lifecycle hooks. Modules register work that must finish before the
 * window closes or the project switches (e.g. flushing unsaved text).
 */
type Flush = () => Promise<void>;

const flushers = new Set<Flush>();

export const lifecycle = {
  /** Register a flush handler. Returns an unregister function. */
  onBeforeClose(f: Flush): () => void {
    flushers.add(f);
    return () => {
      flushers.delete(f);
    };
  },
  /** Run every flush handler; failures are logged, never thrown. */
  async flushAll(): Promise<void> {
    await Promise.all(
      [...flushers].map((f) =>
        f().catch((err: unknown) => console.error("[margin] flush before close failed", err)),
      ),
    );
  },
};
