export {
  nameToColor,
  type RemoteCursorOptions,
  type RemotePeer,
  remoteCursorExtension,
} from "./collaboration/cursors";
export {
  CollaborationProvider,
  type CollaborationProviderOptions,
  type CollaborationSocketProvider,
  createCollaborationProvider,
  type ProviderFactory,
  type ProviderStatus,
  type WebRtcProviderFactory,
} from "./collaboration/provider";
export {
  commentGutterExtension,
  commentGutterState,
  setCommentGutterRanges,
} from "./comments/gutter";
export {
  type CommentDecorationRange,
  type CommentDecorationStatus,
  commentHighlightExtension,
  commentHighlightState,
  setCommentHighlightRanges,
} from "./comments/highlight";
export {
  type DragDropUploadOptions,
  type DroppedFileUploader,
  type DroppedFileUploadResult,
  type DropUploadProgress,
  dragDropUpload,
  dragDropUploadExtension,
  isImageFile,
  markdownForUploadedFile,
  uploadDroppedFiles,
} from "./drag-drop/extension";
export {
  footnotePreview,
  footnotePreviewDecorations,
  footnotePreviewExtension,
} from "./extensions/footnotes";
export {
  activeLineField,
  analyzeMarkdownTree,
  livePreviewExtension,
  type MarkdownTreeAnalysis,
  markdownTreeField,
} from "./live-preview/extension";
export {
  RECONCILIATION_THRESHOLD_RATIO,
  RECONCILIATION_WINDOW_MS,
  ReconciliationDetector,
  type ReconciliationDetectorOptions,
  type ReconciliationTrigger,
  type ReconciliationWindowStats,
  type SectionEditEvent,
  type SectionEditHistoryEntry,
  shouldTriggerReconciliation,
} from "./reconciliation/detector";
export {
  RECONCILIATION_KEEP_BOTH_SEPARATOR,
  type ReconciliationChoice,
  type ReconciliationInlineEntry,
  type ReconciliationInlineExtensionOptions,
  type ReconciliationInlineResolution,
  type ReconciliationInlineVersion,
  reconciliationInlineExtension,
  reconciliationInlineState,
  setReconciliationInlineEntries,
} from "./reconciliation/inline-ui";
export {
  AttributionBadgeWidget,
  attributionExtension,
  attributionState,
  type EditorType,
  type SectionAttribution,
  type SectionContributor,
  setAttributions,
} from "./section/attribution";
export {
  type LeaseBadgeData,
  LeaseBadgeWidget,
  leaseBadgeExtension,
  leaseBadgeState,
  setLeases,
} from "./section/lease-badge";
export {
  overlapIndicatorExtension,
  type SectionOverlapData,
  type SectionOverlapSection,
  type SectionOverlapSeverity,
  sectionOverlapIndicatorState,
  setSectionOverlaps,
} from "./section/overlap-indicator";
export {
  createSlashCommandInsertion,
  getSlashCommand,
  listSlashCommands,
  type SlashCommandDefinition,
  type SlashCommandInsertion,
  type SlashCommandName,
} from "./slash-commands/commands";
export {
  applySlashCommand,
  slashCommandCompletions,
  slashCommands,
  slashCommandsExtension,
} from "./slash-commands/extension";
