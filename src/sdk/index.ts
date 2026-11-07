/**
 * @margin/sdk — the only API modules may use (SPEC §5).
 *
 * Versioned: breaking changes bump the major part of `sdkVersion` and need a
 * migration note in `docs/sdk.md`. Pre-1.0 while M1–M5 shape it.
 */
export const sdkVersion = "0.5.0";

export * from "./extensions";
export { story, StoryError, type Story } from "./story";
export { ai, type GhostClient } from "./ai";
export { aiSession, useAiSession } from "./aiSession";
export {
  memory,
  openSource,
  useMemoryStatus,
  useMemoryVersion,
  BIBLE_ENTRY_VIEW,
  BIBLE_PANEL,
  CONTEXT_VIEW,
  MARGIN_PANEL,
  MEMORY_INSPECTOR_VIEW,
  type PassagePointer,
} from "./memory";
export { setMarginTabs, useMarginTabs, type RegisteredMarginTab } from "./marginTabs";
export { events, type EventType, type EventOf } from "./events";
export {
  useWorkbench,
  workbench,
  useActiveDocumentId,
  type Tab,
  type Pane,
  type PaneId,
  type Notice,
  type PromptRequest,
} from "./workbench";
export { useProject, useBinderTree, useProjectStore, findNode, locateNode, flattenTree } from "./project";
export { lifecycle } from "./lifecycle";
export {
  settings,
  useSettings,
  useSettingsStore,
  DEFAULT_SETTINGS,
  type AppSettings,
  type Theme,
  type EditorFont,
  type FocusMode,
} from "./settings";
export { uiState, useUiState } from "./uiState";
export { editorHost, type EditorHandleApi, type TextTarget } from "./editorHost";
export { newId } from "./ids";
export type * from "./bindings";
export type {
  AiConfig,
  AiStatus,
  AiToggle,
  BakeoffRun,
  BlindQuestion,
  Candidate,
  CandidateRun,
  Capabilities,
  Completion,
  ConnectionMode,
  DocProvenance,
  GhostLength,
  GhostRequest,
  GhostState,
  Typography,
  ProvenanceRecord,
  ProvenanceTotals,
  TextRange,
  ContextLogEntry,
  ContextLogSummary,
  ModelPanel,
  Preset,
  QualityScore,
  RunSummary,
  BakeoffState,
  SpeedResult,
  BackupConfig,
  BackupInfo,
  BackupStatus,
  BinderNode,
  BinderSectionView,
  BinderTarget,
  BinderTree,
  Comment,
  DayPoint,
  DayStats,
  DiffChunk,
  DocAtSnapshot,
  DocDiff,
  DocKind,
  DocMeta,
  DocMetaPatch,
  DocVersion,
  DocumentView,
  DomainEvent,
  Goals,
  HitRef,
  LabelDef,
  Locale,
  MarginError,
  MergeResult,
  NewProject,
  PmNode,
  ProjectInfo,
  ProjectPatch,
  ProjectType,
  RecoveryEntry,
  ReplaceReport,
  SaveResult,
  SearchHit,
  SearchQuery,
  SearchResult,
  SearchScopeKind,
  Section,
  SessionRecord,
  SnapshotInfo,
  SnapshotKind,
  SpellRange,
  StatsSummary,
  TrashNode,
} from "./bindings";
