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
