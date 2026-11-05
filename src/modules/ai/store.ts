/**
 * AI connection state for the status bar and the Model panel, kept in sync
 * by `ai.status` events (SPEC §6.7).
 */
import { create } from "zustand";

import { ai, events, workbench, type AiConfig, type AiStatus, type ModelPanel } from "@margin/sdk";

interface AiStore {
  status: AiStatus;
  detail: string;
  config: AiConfig | null;
  panel: ModelPanel | null;
}

export const useAi = create<AiStore>(() => ({ status: "notRunning", detail: "", config: null, panel: null }));

export const STATUS_LABEL: Record<AiStatus, string> = {
  off: "Off",
  starting: "Starting",
  ready: "Ready",
  thinking: "Thinking",
  pausedTyping: "Paused (typing)",
  notRunning: "Model not running",
};

export function errMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export async function refreshConfig(): Promise<AiConfig | null> {
  try {
    const config = await ai.config.get();
    useAi.setState({ config });
    return config;
  } catch {
    return null;
  }
}

export async function refreshPanel(): Promise<void> {
  try {
    const panel = await ai.panel();
    useAi.setState({ panel, status: panel.status, detail: panel.detail });
  } catch {
    /* the panel is informational; the next event retries */
  }
}

export async function saveConfig(patch: Partial<AiConfig>): Promise<void> {
  const current = useAi.getState().config ?? (await refreshConfig());
  if (!current) return;
  try {
    const config = await ai.config.set({ ...current, ...patch });
    useAi.setState({ config });
  } catch (e) {
    workbench.notify({ message: `Could not save model settings: ${errMessage(e)}` });
  }
}

/** Module activation: returns a cleanup function. */
export function startAi(): () => void {
  const off = events.on("ai.status", (e) => {
    useAi.setState({ status: e.status, detail: e.detail });
  });
  void (async () => {
    const config = await refreshConfig();
    await refreshPanel();
    if (config?.enabled && config.autoStart) {
      // A missing server is shown in the status bar; no dialog (SPEC §14).
      await ai.connect().catch(() => undefined);
    }
  })();
  return off;
}
