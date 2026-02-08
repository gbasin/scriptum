export {
  DocumentRoute,
  formatDropUploadProgress,
  uploadDroppedFileAsDataUrl,
} from "./document-route";

export {
  buildOpenDocumentTabs,
  nextDocumentIdAfterClose,
} from "../lib/document-utils";

export {
  appendReplyToThread,
  commentAnchorTopPx,
  commentRangesFromThreads,
  normalizeInlineCommentThreads,
  type InlineCommentThread,
  updateInlineCommentMessageBody,
  updateInlineCommentThreadStatus,
} from "../lib/inline-comments";

export {
  buildAuthorshipSegments,
  buildTimelineDiffSegments,
  createTimelineSnapshotEntry,
  deriveTimelineSnapshotEntry,
  type TimelineAuthor,
  timelineAuthorFromPeer,
} from "../lib/timeline";
