/**
 * Words the author accepted from ghost-text suggestions this session
 * (SPEC §7.6: the status bar shows session words yours / AI-accepted).
 * The editor adds to it on accept; the stats module shows it.
 */
import { create } from "zustand";

interface AiSessionState {
  accepted: number;
}

export const useAiSession = create<AiSessionState>(() => ({ accepted: 0 }));

export const aiSession = {
  get: (): number => useAiSession.getState().accepted,
  add(words: number): void {
    if (words > 0) useAiSession.setState((s) => ({ accepted: s.accepted + words }));
  },
  reset(): void {
    useAiSession.setState({ accepted: 0 });
  },
};
