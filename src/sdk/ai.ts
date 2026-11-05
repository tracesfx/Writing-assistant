/**
 * AI client (SPEC §6): connection, status, the context log and the
 * bake-off. Wraps the generated IPC bindings like `story` does. None of
 * these calls touch manuscript text.
 */
import { commands, type AiConfig } from "./bindings";
import { unwrap } from "./story";

export const ai = {
  config: {
    get: () => unwrap(commands.aiConfigGet()),
    set: (config: AiConfig) => unwrap(commands.aiConfigSet(config)),
  },
  panel: () => unwrap(commands.aiPanel()),
  /** Start (managed) or connect (external) using the saved settings. */
  connect: () => unwrap(commands.aiConnect()),
  disconnect: () => unwrap(commands.aiDisconnect()),
  /** Tell the scheduler the author typed: cancels ghost text, pauses idle work. */
  keystroke: () => unwrap(commands.aiKeystroke()),
  /** One-off request for the Model panel. */
  try: (text: string) => unwrap(commands.aiTry(text)),
  contextLog: {
    list: (limit = 100) => unwrap(commands.contextLogList(limit)),
    get: (id: string) => unwrap(commands.contextLogGet(id)),
    clear: () => unwrap(commands.contextLogClear()),
  },
  bakeoff: {
    candidates: () => unwrap(commands.bakeoffCandidates()),
    start: (presetIds: string[], tune: boolean) => unwrap(commands.bakeoffStart(presetIds, tune)),
    cancel: () => unwrap(commands.bakeoffCancel()),
    state: () => unwrap(commands.bakeoffState()),
    runs: () => unwrap(commands.bakeoffRuns()),
    load: (runId: string) => unwrap(commands.bakeoffLoad(runId)),
    questions: (runId: string) => unwrap(commands.bakeoffQuestions(runId)),
    answer: (runId: string, index: number, label: string | null) => unwrap(commands.bakeoffAnswer(runId, index, label)),
    scores: (runId: string) => unwrap(commands.bakeoffScores(runId)),
    useModel: (runId: string, presetId: string) => unwrap(commands.bakeoffUseModel(runId, presetId)),
  },
};
