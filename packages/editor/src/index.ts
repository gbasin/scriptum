export {
  activeLineField,
  analyzeMarkdownTree,
  livePreviewExtension,
  markdownTreeField,
  type MarkdownTreeAnalysis,
} from "./live-preview/extension";
export {
  CollaborationProvider,
  createCollaborationProvider,
  type CollaborationProviderOptions,
  type CollaborationSocketProvider,
  type ProviderFactory,
  type ProviderStatus,
  type WebRtcProviderFactory,
} from "./collaboration/provider";
export {
  nameToColor,
  remoteCursorExtension,
  type RemoteCursorOptions,
  type RemotePeer,
} from "./collaboration/cursors";
export {
  commentHighlightExtension,
  commentHighlightState,
  setCommentHighlightRanges,
  type CommentDecorationRange,
  type CommentDecorationStatus,
} from "./comments/highlight";
export {
  commentGutterExtension,
  commentGutterState,
  setCommentGutterRanges,
} from "./comments/gutter";
export {
  ReconciliationDetector,
  RECONCILIATION_THRESHOLD_RATIO,
  RECONCILIATION_WINDOW_MS,
  shouldTriggerReconciliation,
  type ReconciliationDetectorOptions,
  type ReconciliationTrigger,
  type ReconciliationWindowStats,
  type SectionEditEvent,
  type SectionEditHistoryEntry,
} from "./reconciliation/detector";
export {
  RECONCILIATION_KEEP_BOTH_SEPARATOR,
  reconciliationInlineExtension,
  reconciliationInlineState,
  setReconciliationInlineEntries,
  type ReconciliationChoice,
  type ReconciliationInlineEntry,
  type ReconciliationInlineExtensionOptions,
  type ReconciliationInlineResolution,
  type ReconciliationInlineVersion,
} from "./reconciliation/inline-ui";
export {
  overlapIndicatorExtension,
  sectionOverlapIndicatorState,
  setSectionOverlaps,
  type SectionOverlapData,
  type SectionOverlapSection,
  type SectionOverlapSeverity,
} from "./section/overlap-indicator";
export {
  leaseBadgeExtension,
  leaseBadgeState,
  LeaseBadgeWidget,
  setLeases,
  type LeaseBadgeData,
} from "./section/lease-badge";
export {
  attributionExtension,
  attributionState,
  AttributionBadgeWidget,
  setAttributions,
  type EditorType,
  type SectionAttribution,
  type SectionContributor,
} from "./section/attribution";
export {
  applySlashCommand,
  slashCommandCompletions,
  slashCommands,
  slashCommandsExtension,
} from "./slash-commands/extension";
export {
  createSlashCommandInsertion,
  getSlashCommand,
  listSlashCommands,
  type SlashCommandDefinition,
  type SlashCommandInsertion,
  type SlashCommandName,
} from "./slash-commands/commands";
