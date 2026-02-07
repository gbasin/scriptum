import { markdown } from "@codemirror/lang-markdown";
import { EditorState, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  type CommentDecorationRange,
  commentGutterExtension,
  commentHighlightExtension,
  createCollaborationProvider,
  type DropUploadProgress,
  dragDropUploadExtension,
  livePreviewExtension,
  nameToColor,
  reconciliationInlineExtension,
  remoteCursorExtension,
  setCommentGutterRanges,
  setCommentHighlightRanges,
  setReconciliationInlineEntries,
  slashCommandsExtension,
  type WebRtcProviderFactory,
} from "@scriptum/editor";
import type { Document as ScriptumDocument } from "@scriptum/shared";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { AvatarStack } from "../components/AvatarStack";
import { Breadcrumb } from "../components/editor/Breadcrumb";
import { type OpenDocumentTab, TabBar } from "../components/editor/TabBar";
import {
  type HistoryViewMode,
  TimelineSlider,
} from "../components/editor/TimelineSlider";
import { OfflineBanner } from "../components/OfflineBanner";
import { StatusBar } from "../components/StatusBar";
import { useDocumentsStore } from "../store/documents";
import { type PeerPresence, usePresenceStore } from "../store/presence";
import { useSyncStore } from "../store/sync";
import { useWorkspaceStore } from "../store/workspace";
import type { ScriptumTestState } from "../test/harness";
import {
  buildShareLinkUrl,
  createShareLinkRecord,
  expirationIsoFromOption,
  parseShareLinkMaxUses,
  sharePermissionLabel,
  type ShareLinkExpirationOption,
  type ShareLinkPermission,
  type ShareLinkTargetType,
  storeShareLinkRecord,
} from "./share-links";

const DEFAULT_DAEMON_WS_BASE_URL =
  (import.meta.env.VITE_SCRIPTUM_DAEMON_WS_URL as string | undefined) ??
  "ws://127.0.0.1:39091/yjs";
const DEFAULT_WEBRTC_SIGNALING_URL =
  (import.meta.env.VITE_SCRIPTUM_WEBRTC_SIGNALING_URL as string | undefined) ??
  null;
const REALTIME_E2E_MODE =
  (import.meta.env.VITE_SCRIPTUM_REALTIME_E2E as string | undefined) === "1";
const LOCAL_COMMENT_AUTHOR_ID = "local-user";
const LOCAL_COMMENT_AUTHOR_NAME = "You";
const UNKNOWN_COMMENT_AUTHOR_NAME = "Unknown";
const UNKNOWN_COMMENT_TIMESTAMP = "1970-01-01T00:00:00.000Z";
const FIXTURE_REMOTE_CLIENT_ID_BASE = 10_000;
const MAX_TIMELINE_SNAPSHOTS = 240;
const DROP_UPLOAD_SUCCESS_HIDE_DELAY_MS = 2_000;
const DROP_UPLOAD_FAILURE_HIDE_DELAY_MS = 4_000;

const DEFAULT_TEST_STATE: ScriptumTestState = {
  fixtureName: "default",
  docContent: "# Fixture Document",
  cursor: { line: 0, ch: 0 },
  remotePeers: [],
  syncState: "synced",
  pendingSyncUpdates: 0,
  reconnectProgress: null,
  gitStatus: { dirty: false, ahead: 0, behind: 0 },
  commentThreads: [],
  reconciliationEntries: [],
  shareLinksEnabled: false,
};

interface UnknownRecord {
  [key: string]: unknown;
}

interface InlineCommentMessage {
  authorName: string;
  authorUserId?: string;
  bodyMd: string;
  createdAt: string;
  id: string;
  isOwn: boolean;
}

export interface InlineCommentThread {
  endOffsetUtf16: number;
  id: string;
  messages: InlineCommentMessage[];
  startOffsetUtf16: number;
  status: "open" | "resolved";
}

interface ActiveTextSelection {
  from: number;
  line: number;
  selectedText: string;
  to: number;
}

export interface TimelineAuthor {
  color: string;
  id: string;
  name: string;
  type: "agent" | "human";
}

export interface TimelineSnapshotEntry {
  attribution: TimelineAuthor[];
  content: string;
}

export interface AuthorshipSegment {
  author: TimelineAuthor;
  text: string;
}

export interface TimelineDiffSegment {
  kind: "unchanged" | "removed" | "added";
  text: string;
}

const LOCAL_TIMELINE_AUTHOR: TimelineAuthor = {
  color: nameToColor(LOCAL_COMMENT_AUTHOR_NAME),
  id: LOCAL_COMMENT_AUTHOR_ID,
  name: LOCAL_COMMENT_AUTHOR_NAME,
  type: "human",
};

const UNKNOWN_REMOTE_TIMELINE_AUTHOR: TimelineAuthor = {
  color: nameToColor("Collaborator"),
  id: "remote-collaborator",
  name: "Collaborator",
  type: "human",
};

export function timelineAuthorFromPeer(
  peer: Pick<ScriptumTestState["remotePeers"][number], "name" | "type">,
): TimelineAuthor {
  return {
    color: nameToColor(peer.name),
    id: `peer:${peer.name.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "remote"}`,
    name: peer.name,
    type: peer.type,
  };
}

export function createTimelineSnapshotEntry(
  content: string,
  author: TimelineAuthor,
): TimelineSnapshotEntry {
  return {
    attribution: Array.from({ length: content.length }, () => author),
    content,
  };
}

function normalizedAttributionLength(
  entry: TimelineSnapshotEntry,
): TimelineAuthor[] {
  if (entry.attribution.length === entry.content.length) {
    return entry.attribution;
  }

  return Array.from(
    { length: entry.content.length },
    (_unused, index) => entry.attribution[index] ?? LOCAL_TIMELINE_AUTHOR,
  );
}

export function deriveTimelineSnapshotEntry(
  previousEntry: TimelineSnapshotEntry,
  nextContent: string,
  author: TimelineAuthor,
): TimelineSnapshotEntry {
  if (previousEntry.content === nextContent) {
    return {
      attribution: normalizedAttributionLength(previousEntry).slice(),
      content: previousEntry.content,
    };
  }

  const previousContent = previousEntry.content;
  const previousAttribution = normalizedAttributionLength(previousEntry);
  let prefixLength = 0;

  while (
    prefixLength < previousContent.length &&
    prefixLength < nextContent.length &&
    previousContent[prefixLength] === nextContent[prefixLength]
  ) {
    prefixLength += 1;
  }

  let suffixLength = 0;
  while (
    suffixLength < previousContent.length - prefixLength &&
    suffixLength < nextContent.length - prefixLength &&
    previousContent[previousContent.length - 1 - suffixLength] ===
      nextContent[nextContent.length - 1 - suffixLength]
  ) {
    suffixLength += 1;
  }

  const nextMiddleLength = Math.max(
    0,
    nextContent.length - prefixLength - suffixLength,
  );
  const prefixAttribution = previousAttribution.slice(0, prefixLength);
  const suffixAttribution =
    suffixLength > 0
      ? previousAttribution.slice(previousAttribution.length - suffixLength)
      : [];
  const middleAttribution = Array.from(
    { length: nextMiddleLength },
    () => author,
  );

  return {
    attribution: [
      ...prefixAttribution,
      ...middleAttribution,
      ...suffixAttribution,
    ],
    content: nextContent,
  };
}

export function buildAuthorshipSegments(
  entry: TimelineSnapshotEntry,
): AuthorshipSegment[] {
  const attribution = normalizedAttributionLength(entry);
  const { content } = entry;
  if (content.length === 0) {
    return [];
  }

  const segments: AuthorshipSegment[] = [];
  let currentAuthor = attribution[0] ?? LOCAL_TIMELINE_AUTHOR;
  let currentText = content[0] ?? "";

  for (let index = 1; index < content.length; index += 1) {
    const nextAuthor = attribution[index] ?? LOCAL_TIMELINE_AUTHOR;
    const nextCharacter = content[index] ?? "";

    if (nextAuthor.id === currentAuthor.id) {
      currentText += nextCharacter;
      continue;
    }

    segments.push({ author: currentAuthor, text: currentText });
    currentAuthor = nextAuthor;
    currentText = nextCharacter;
  }

  segments.push({ author: currentAuthor, text: currentText });
  return segments;
}

export function buildTimelineDiffSegments(
  currentContent: string,
  snapshotContent: string,
): TimelineDiffSegment[] {
  if (currentContent.length === 0 && snapshotContent.length === 0) {
    return [];
  }
  if (currentContent === snapshotContent) {
    return [{ kind: "unchanged", text: snapshotContent }];
  }

  let prefixLength = 0;
  while (
    prefixLength < currentContent.length &&
    prefixLength < snapshotContent.length &&
    currentContent[prefixLength] === snapshotContent[prefixLength]
  ) {
    prefixLength += 1;
  }

  let suffixLength = 0;
  while (
    suffixLength < currentContent.length - prefixLength &&
    suffixLength < snapshotContent.length - prefixLength &&
    currentContent[currentContent.length - 1 - suffixLength] ===
      snapshotContent[snapshotContent.length - 1 - suffixLength]
  ) {
    suffixLength += 1;
  }

  const prefix = currentContent.slice(0, prefixLength);
  const removed = currentContent.slice(
    prefixLength,
    currentContent.length - suffixLength,
  );
  const added = snapshotContent.slice(
    prefixLength,
    snapshotContent.length - suffixLength,
  );
  const suffix =
    suffixLength > 0
      ? snapshotContent.slice(snapshotContent.length - suffixLength)
      : "";

  const segments: TimelineDiffSegment[] = [];
  if (prefix.length > 0) {
    segments.push({ kind: "unchanged", text: prefix });
  }
  if (removed.length > 0) {
    segments.push({ kind: "removed", text: removed });
  }
  if (added.length > 0) {
    segments.push({ kind: "added", text: added });
  }
  if (suffix.length > 0) {
    segments.push({ kind: "unchanged", text: suffix });
  }

  return segments;
}

function pluralizeFiles(count: number): string {
  return count === 1 ? "file" : "files";
}

export function formatDropUploadProgress(progress: DropUploadProgress): string {
  if (progress.phase === "completed") {
    if (progress.failedFiles > 0) {
      return `Uploaded ${progress.completedFiles}/${progress.totalFiles} ${pluralizeFiles(progress.totalFiles)}. ${progress.failedFiles} failed.`;
    }
    return `Uploaded ${progress.totalFiles} ${pluralizeFiles(progress.totalFiles)} and inserted markdown links.`;
  }

  const fileName = progress.currentFileName ?? "file";
  return `Uploading ${progress.completedFiles + 1}/${progress.totalFiles}: ${fileName} (${progress.currentFilePercent}%)`;
}

export function uploadDroppedFileAsDataUrl(
  file: File,
  onProgress: (percent: number) => void,
): Promise<{ url: string }> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => {
      reject(new Error(`failed to read dropped file: ${file.name}`));
    };
    reader.onprogress = (event) => {
      if (!event.lengthComputable || event.total <= 0) {
        return;
      }
      onProgress((event.loaded / event.total) * 100);
    };
    reader.onload = () => {
      if (typeof reader.result !== "string") {
        reject(new Error(`failed to encode dropped file: ${file.name}`));
        return;
      }
      onProgress(100);
      resolve({ url: reader.result });
    };
    reader.readAsDataURL(file);
  });
}

function asRecord(value: unknown): UnknownRecord | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as UnknownRecord;
}

function readNumber(
  record: UnknownRecord,
  keys: readonly string[],
): number | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return null;
}

function readString(
  record: UnknownRecord,
  keys: readonly string[],
): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }
  return null;
}

function normalizeInlineCommentMessages(
  value: unknown,
): InlineCommentMessage[] {
  const rawMessages = Array.isArray(value) ? value : value ? [value] : [];
  const messages: InlineCommentMessage[] = [];

  for (const rawMessage of rawMessages) {
    const messageRecord = asRecord(rawMessage);
    if (!messageRecord) {
      continue;
    }
    const id = readString(messageRecord, ["id"]);
    const bodyMd = readString(messageRecord, ["bodyMd", "body_md", "message"]);
    if (!id || !bodyMd) {
      continue;
    }

    const authorRecord = asRecord(messageRecord.author);
    const authorUserId =
      readString(messageRecord, ["authorUserId", "author_user_id", "userId"]) ??
      (authorRecord
        ? readString(authorRecord, ["id", "userId", "user_id"])
        : null);
    const explicitIsOwn = messageRecord.isOwn;
    const isOwn =
      typeof explicitIsOwn === "boolean"
        ? explicitIsOwn
        : authorUserId === LOCAL_COMMENT_AUTHOR_ID;
    const authorName =
      readString(messageRecord, ["authorName", "author_name", "author"]) ??
      (authorRecord
        ? readString(authorRecord, ["name", "display_name", "displayName"])
        : null) ??
      (isOwn ? LOCAL_COMMENT_AUTHOR_NAME : UNKNOWN_COMMENT_AUTHOR_NAME);
    const createdAt =
      readString(messageRecord, ["createdAt", "created_at", "timestamp"]) ??
      UNKNOWN_COMMENT_TIMESTAMP;

    messages.push({
      authorName,
      ...(authorUserId ? { authorUserId } : {}),
      bodyMd,
      createdAt,
      id,
      isOwn,
    });
  }

  return messages;
}

function normalizeInlineCommentThread(
  value: unknown,
): InlineCommentThread | null {
  const record = asRecord(value);
  if (!record) {
    return null;
  }

  const threadRecord = asRecord(record.thread) ?? record;
  const id = readString(threadRecord, ["id"]);
  const startOffsetUtf16 = readNumber(threadRecord, [
    "startOffsetUtf16",
    "start_offset_utf16",
  ]);
  const endOffsetUtf16 = readNumber(threadRecord, [
    "endOffsetUtf16",
    "end_offset_utf16",
  ]);
  if (!id || startOffsetUtf16 === null || endOffsetUtf16 === null) {
    return null;
  }
  if (endOffsetUtf16 <= startOffsetUtf16) {
    return null;
  }

  const statusRaw = readString(threadRecord, ["status"]) ?? "open";
  const status: InlineCommentThread["status"] =
    statusRaw === "resolved" ? "resolved" : "open";

  const messages = normalizeInlineCommentMessages(
    record.messages ?? record.message ?? threadRecord.messages,
  );

  return {
    endOffsetUtf16,
    id,
    messages,
    startOffsetUtf16,
    status,
  };
}

export function normalizeInlineCommentThreads(
  values: unknown[],
): InlineCommentThread[] {
  const threads: InlineCommentThread[] = [];
  const seenThreadIds = new Set<string>();

  for (const value of values) {
    const thread = normalizeInlineCommentThread(value);
    if (!thread || seenThreadIds.has(thread.id)) {
      continue;
    }

    seenThreadIds.add(thread.id);
    threads.push(thread);
  }

  return threads;
}

export function commentRangesFromThreads(
  threads: readonly InlineCommentThread[],
): CommentDecorationRange[] {
  return threads.map((thread) => ({
    from: thread.startOffsetUtf16,
    status: thread.status,
    threadId: thread.id,
    to: thread.endOffsetUtf16,
  }));
}

export function appendReplyToThread(
  threads: readonly InlineCommentThread[],
  threadId: string,
  message: InlineCommentMessage,
): InlineCommentThread[] {
  let didAppend = false;
  const nextThreads = threads.map((thread) => {
    if (thread.id !== threadId) {
      return thread;
    }
    didAppend = true;
    return {
      ...thread,
      messages: [...thread.messages, message],
    };
  });

  return didAppend ? nextThreads : [...threads];
}

export function updateInlineCommentMessageBody(
  threads: readonly InlineCommentThread[],
  threadId: string,
  messageId: string,
  nextBodyMd: string,
): InlineCommentThread[] {
  const nextBody = nextBodyMd.trim();
  if (!nextBody) {
    return [...threads];
  }

  let didUpdate = false;
  const nextThreads = threads.map((thread) => {
    if (thread.id !== threadId) {
      return thread;
    }

    const nextMessages = thread.messages.map((message) => {
      if (message.id !== messageId || !message.isOwn) {
        return message;
      }
      didUpdate = true;
      return {
        ...message,
        bodyMd: nextBody,
      };
    });

    return didUpdate
      ? {
          ...thread,
          messages: nextMessages,
        }
      : thread;
  });

  return didUpdate ? nextThreads : [...threads];
}

export function updateInlineCommentThreadStatus(
  threads: readonly InlineCommentThread[],
  threadId: string,
  status: InlineCommentThread["status"],
): InlineCommentThread[] {
  let didUpdate = false;
  const nextThreads = threads.map((thread) => {
    if (thread.id !== threadId) {
      return thread;
    }
    if (thread.status === status) {
      return thread;
    }
    didUpdate = true;
    return {
      ...thread,
      status,
    };
  });

  return didUpdate ? nextThreads : [...threads];
}

export function commentAnchorTopPx(line: number): number {
  if (!Number.isFinite(line)) {
    return 12;
  }
  return Math.max(12, (Math.max(1, Math.floor(line)) - 1) * 22 + 12);
}

function documentTitleFromPath(path: string): string {
  const segments = path
    .split("/")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  return segments[segments.length - 1] ?? path;
}

export function buildOpenDocumentTabs(
  openDocuments: readonly ScriptumDocument[],
  workspaceId: string | undefined,
  activeDocumentId: string | undefined,
  activeDocumentPath: string,
): OpenDocumentTab[] {
  const workspaceOpenDocuments = workspaceId
    ? openDocuments.filter((document) => document.workspaceId === workspaceId)
    : [];
  const tabs = workspaceOpenDocuments.map((document) => ({
    id: document.id,
    path: document.path,
    title: document.title,
  }));

  if (activeDocumentId && !tabs.some((tab) => tab.id === activeDocumentId)) {
    tabs.unshift({
      id: activeDocumentId,
      path: activeDocumentPath,
      title: documentTitleFromPath(activeDocumentPath),
    });
  }

  return tabs;
}

export function nextDocumentIdAfterClose(
  tabs: readonly OpenDocumentTab[],
  closingDocumentId: string,
): string | null {
  const closingIndex = tabs.findIndex((tab) => tab.id === closingDocumentId);
  if (closingIndex < 0) {
    return null;
  }

  const remainingTabs = tabs.filter((tab) => tab.id !== closingDocumentId);
  if (remainingTabs.length === 0) {
    return null;
  }

  const nextIndex = Math.max(0, closingIndex - 1);
  return remainingTabs[nextIndex]?.id ?? remainingTabs[0]?.id ?? null;
}

function readFixtureState(): ScriptumTestState {
  if (typeof window === "undefined" || !window.__SCRIPTUM_TEST__) {
    return DEFAULT_TEST_STATE;
  }
  return window.__SCRIPTUM_TEST__.getState();
}

function makeClientId(prefix: string): string {
  if (
    typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
  ) {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Math.random().toString(16).slice(2)}`;
}

function cursorOffsetFromLineCh(
  markdown: string,
  cursor: { line: number; ch: number },
): number {
  const lines = markdown.split("\n");
  const lineIndex = Math.max(
    0,
    Math.min(lines.length - 1, Math.floor(cursor.line)),
  );
  let offset = 0;
  for (let index = 0; index < lineIndex; index += 1) {
    offset += lines[index].length + 1;
  }
  const column = Math.max(
    0,
    Math.min(lines[lineIndex]?.length ?? 0, Math.floor(cursor.ch)),
  );
  return offset + column;
}

function badgeSuffix(name: string): string {
  const normalized = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || "peer";
}

function resolveGlobalWebRtcProviderFactory():
  | WebRtcProviderFactory
  | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }

  const candidate = (window as unknown as { WebrtcProvider?: unknown })
    .WebrtcProvider;
  if (typeof candidate !== "function") {
    return undefined;
  }

  return ({ doc, room, url }) => {
    const Ctor = candidate as {
      new (
        room: string,
        doc: unknown,
        options: { signaling: string[]; connect: boolean },
      ): unknown;
    };
    return new Ctor(room, doc, {
      signaling: [url],
      connect: false,
    }) as ReturnType<WebRtcProviderFactory>;
  };
}

export function DocumentRoute() {
  const { workspaceId, documentId } = useParams();
  const navigate = useNavigate();
  const closeDocument = useDocumentsStore((state) => state.closeDocument);
  const documents = useDocumentsStore((state) => state.documents);
  const openDocuments = useDocumentsStore((state) => state.openDocuments);
  const setActiveDocumentForWorkspace = useDocumentsStore(
    (state) => state.setActiveDocumentForWorkspace,
  );
  const workspaces = useWorkspaceStore((state) => state.workspaces);
  const [fixtureState, setFixtureState] = useState<ScriptumTestState>(() =>
    readFixtureState(),
  );
  const fixtureModeEnabled =
    typeof window !== "undefined" && Boolean(window.__SCRIPTUM_TEST__);
  const [inlineCommentThreads, setInlineCommentThreads] = useState<
    InlineCommentThread[]
  >(() => normalizeInlineCommentThreads(readFixtureState().commentThreads));
  const [activeSelection, setActiveSelection] =
    useState<ActiveTextSelection | null>(null);
  const [isCommentPopoverOpen, setCommentPopoverOpen] = useState(false);
  const [pendingCommentBody, setPendingCommentBody] = useState("");
  const [isShareDialogOpen, setShareDialogOpen] = useState(false);
  const [shareTargetType, setShareTargetType] =
    useState<ShareLinkTargetType>("document");
  const [sharePermission, setSharePermission] =
    useState<ShareLinkPermission>("view");
  const [shareExpirationOption, setShareExpirationOption] =
    useState<ShareLinkExpirationOption>("none");
  const [shareMaxUsesInput, setShareMaxUsesInput] = useState("3");
  const [generatedShareUrl, setGeneratedShareUrl] = useState("");
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingMessageBody, setEditingMessageBody] = useState("");
  const [dropUploadProgress, setDropUploadProgress] =
    useState<DropUploadProgress | null>(null);
  const activeEditors = fixtureModeEnabled
    ? fixtureState.remotePeers.length + 1
    : 1;
  const [syncState, setSyncState] = useState<ScriptumTestState["syncState"]>(
    fixtureModeEnabled ? fixtureState.syncState : "reconnecting",
  );
  const setPresencePeers = usePresenceStore((state) => state.setPeers);
  const pendingChanges = useSyncStore((state) => state.pendingChanges);
  const [cursor, setCursor] = useState(fixtureState.cursor);
  const [daemonWsBaseUrl] = useState(DEFAULT_DAEMON_WS_BASE_URL);
  const [webrtcSignalingUrl] = useState(DEFAULT_WEBRTC_SIGNALING_URL);
  const [webrtcProviderFactory] = useState<WebRtcProviderFactory | undefined>(
    () => resolveGlobalWebRtcProviderFactory(),
  );
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const isApplyingTimelineSnapshotRef = useRef(false);
  const collaborationProviderRef = useRef<ReturnType<
    typeof createCollaborationProvider
  > | null>(null);
  const fixtureRemoteClientIdsRef = useRef<number[]>([]);
  const timelineRemotePeersRef = useRef(fixtureState.remotePeers);
  const [timelineEntries, setTimelineEntries] = useState<
    TimelineSnapshotEntry[]
  >([
    createTimelineSnapshotEntry(fixtureState.docContent, LOCAL_TIMELINE_AUTHOR),
  ]);
  const [timelineIndex, setTimelineIndex] = useState(0);
  const [timelineViewMode, setTimelineViewMode] =
    useState<HistoryViewMode>("authorship");
  const roomId = useMemo(
    () =>
      `${workspaceId ?? "unknown-workspace"}:${documentId ?? "unknown-document"}`,
    [workspaceId, documentId],
  );
  const currentDocument = useMemo(
    () =>
      documentId
        ? (documents.find((candidate) => candidate.id === documentId) ?? null)
        : null,
    [documentId, documents],
  );
  const currentDocumentPath = currentDocument?.path ?? documentId ?? "unknown";
  const openTabs = useMemo(
    () =>
      buildOpenDocumentTabs(
        openDocuments,
        workspaceId,
        documentId,
        currentDocumentPath,
      ),
    [currentDocumentPath, documentId, openDocuments, workspaceId],
  );
  const workspaceLabel = useMemo(() => {
    if (!workspaceId) {
      return "Unknown workspace";
    }
    return (
      workspaces.find((workspace) => workspace.id === workspaceId)?.name ??
      workspaceId
    );
  }, [workspaceId, workspaces]);
  const presencePeers = useMemo<PeerPresence[]>(
    () =>
      fixtureState.remotePeers.map((peer) => ({
        activeDocumentPath: currentDocumentPath,
        color: nameToColor(peer.name),
        cursor: {
          column: peer.cursor.ch,
          line: peer.cursor.line,
          sectionId: peer.section ?? null,
        },
        lastSeenAt: new Date(Date.now()).toISOString(),
        name: peer.name,
        type: peer.type,
      })),
    [currentDocumentPath, fixtureState.remotePeers],
  );
  useEffect(() => {
    setPresencePeers(presencePeers);
    return () => {
      setPresencePeers([]);
    };
  }, [presencePeers, setPresencePeers]);
  const overlapSummary = useMemo(() => {
    const bySection = new Map<string, typeof fixtureState.remotePeers>();
    for (const peer of fixtureState.remotePeers) {
      const section = peer.section?.trim();
      if (!section) {
        continue;
      }
      const peers = bySection.get(section) ?? [];
      peers.push(peer);
      bySection.set(section, peers);
    }

    const warningSections = Array.from(bySection.entries())
      .filter(([, peers]) => peers.length >= 2)
      .map(([section, peers]) => ({ peers, section }));

    const severity: "none" | "info" | "warning" =
      warningSections.length > 0
        ? "warning"
        : fixtureState.remotePeers.length > 0
          ? "info"
          : "none";

    return { severity, warningSections };
  }, [fixtureState.remotePeers]);
  useEffect(() => {
    timelineRemotePeersRef.current = fixtureState.remotePeers;
  }, [fixtureState.remotePeers]);
  const commentRanges = useMemo(
    () => commentRangesFromThreads(inlineCommentThreads),
    [inlineCommentThreads],
  );
  const commentAnchorTop = activeSelection
    ? commentAnchorTopPx(activeSelection.line)
    : 12;
  const activeThread = useMemo(() => {
    if (!activeSelection) {
      return null;
    }
    return (
      inlineCommentThreads.find(
        (thread) =>
          thread.startOffsetUtf16 === activeSelection.from &&
          thread.endOffsetUtf16 === activeSelection.to,
      ) ?? null
    );
  }, [activeSelection, inlineCommentThreads]);
  const canComposeInActiveThread =
    !activeThread || activeThread.status === "open";
  const pendingSyncUpdates = fixtureModeEnabled
    ? fixtureState.pendingSyncUpdates
    : pendingChanges;
  const reconnectProgress = fixtureModeEnabled
    ? fixtureState.reconnectProgress
    : null;
  const shareLinksEnabled = fixtureModeEnabled && fixtureState.shareLinksEnabled;

  useEffect(() => {
    if (documentId) {
      return;
    }
    setShareTargetType("workspace");
  }, [documentId]);

  useEffect(() => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      return;
    }

    setFixtureState(api.getState());
    return api.subscribe((nextState) => setFixtureState(nextState));
  }, []);

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }
    setSyncState(fixtureState.syncState);
    setCursor(fixtureState.cursor);
    setInlineCommentThreads(
      normalizeInlineCommentThreads(fixtureState.commentThreads),
    );
  }, [
    fixtureModeEnabled,
    fixtureState.commentThreads,
    fixtureState.cursor,
    fixtureState.syncState,
  ]);

  useEffect(() => {
    if (!dropUploadProgress || dropUploadProgress.phase !== "completed") {
      return;
    }

    const delay =
      dropUploadProgress.failedFiles > 0
        ? DROP_UPLOAD_FAILURE_HIDE_DELAY_MS
        : DROP_UPLOAD_SUCCESS_HIDE_DELAY_MS;
    const timeout = window.setTimeout(() => {
      setDropUploadProgress(null);
    }, delay);
    return () => {
      window.clearTimeout(timeout);
    };
  }, [dropUploadProgress]);

  useEffect(() => {
    if (!workspaceId || !documentId) {
      return;
    }
    setActiveDocumentForWorkspace(workspaceId, documentId);
  }, [documentId, setActiveDocumentForWorkspace, workspaceId]);

  useEffect(() => {
    const host = editorHostRef.current;
    if (!host) {
      return;
    }

    host.innerHTML = "";
    setDropUploadProgress(null);
    const provider = createCollaborationProvider({
      connectOnCreate: false,
      room: roomId,
      url: daemonWsBaseUrl,
      webrtcSignalingUrl: webrtcSignalingUrl ?? undefined,
      webrtcProviderFactory,
    });
    collaborationProviderRef.current = provider;

    if (fixtureState.docContent.length > 0) {
      provider.yText.insert(0, fixtureState.docContent);
    }

    provider.provider.on("status", ({ status }) => {
      if (fixtureModeEnabled) {
        return;
      }
      setSyncState(status === "connected" ? "synced" : "reconnecting");
    });
    if (!fixtureModeEnabled) {
      provider.connect();
      setSyncState("reconnecting");
    }
    if (REALTIME_E2E_MODE) {
      const localAwarenessName = `User ${provider.provider.awareness.clientID}`;
      provider.provider.awareness.setLocalStateField("user", {
        color: nameToColor(localAwarenessName),
        name: localAwarenessName,
        type: "human",
      });
      provider.provider.awareness.setLocalStateField("cursor", {
        anchor: 0,
        head: 0,
      });
    }

    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: fixtureState.docContent,
        extensions: [
          markdown(),
          livePreviewExtension(),
          slashCommandsExtension(),
          reconciliationInlineExtension(),
          commentHighlightExtension(),
          commentGutterExtension(),
          provider.extension(),
          remoteCursorExtension({ awareness: provider.provider.awareness }),
          dragDropUploadExtension({
            onError: (_error, _file) => {
              // Progress UI includes failure counts; no extra UI surface needed here.
            },
            onProgress: (progress) => {
              setDropUploadProgress(progress);
            },
            uploadFile: uploadDroppedFileAsDataUrl,
          }),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged && !isApplyingTimelineSnapshotRef.current) {
              const nextContent = update.state.doc.toString();
              const isRemoteTransaction = update.transactions.some(
                (transaction) =>
                  Boolean(transaction.annotation(Transaction.remote)),
              );
              const nextAuthor = isRemoteTransaction
                ? timelineRemotePeersRef.current[0]
                  ? timelineAuthorFromPeer(timelineRemotePeersRef.current[0])
                  : UNKNOWN_REMOTE_TIMELINE_AUTHOR
                : LOCAL_TIMELINE_AUTHOR;

              setTimelineEntries((currentEntries) => {
                const latestEntry =
                  currentEntries[currentEntries.length - 1] ??
                  createTimelineSnapshotEntry("", LOCAL_TIMELINE_AUTHOR);
                if (latestEntry.content === nextContent) {
                  return currentEntries;
                }

                const nextEntry = deriveTimelineSnapshotEntry(
                  latestEntry,
                  nextContent,
                  nextAuthor,
                );
                const nextEntries = [...currentEntries, nextEntry];
                if (nextEntries.length > MAX_TIMELINE_SNAPSHOTS) {
                  nextEntries.splice(
                    0,
                    nextEntries.length - MAX_TIMELINE_SNAPSHOTS,
                  );
                }

                setTimelineIndex(nextEntries.length - 1);
                return nextEntries;
              });
            }

            if (!update.selectionSet) {
              return;
            }

            const mainSelection = update.state.selection.main;
            const line = update.state.doc.lineAt(mainSelection.head);
            setCursor({
              ch: mainSelection.head - line.from,
              line: line.number - 1,
            });
            if (REALTIME_E2E_MODE) {
              provider.provider.awareness.setLocalStateField("cursor", {
                anchor: mainSelection.anchor,
                head: mainSelection.head,
              });
            }

            if (mainSelection.empty) {
              setActiveSelection(null);
              setCommentPopoverOpen(false);
              return;
            }

            const selectedText = update.state.sliceDoc(
              mainSelection.from,
              mainSelection.to,
            );
            if (selectedText.trim().length === 0) {
              setActiveSelection(null);
              setCommentPopoverOpen(false);
              return;
            }

            setActiveSelection({
              from: mainSelection.from,
              line: update.state.doc.lineAt(mainSelection.from).number,
              selectedText,
              to: mainSelection.to,
            });
          }),
        ],
      }),
    });
    editorViewRef.current = view;
    setTimelineEntries([
      createTimelineSnapshotEntry(
        fixtureState.docContent,
        LOCAL_TIMELINE_AUTHOR,
      ),
    ]);
    setTimelineIndex(0);

    return () => {
      editorViewRef.current = null;
      collaborationProviderRef.current = null;
      view.destroy();
      provider.destroy();
    };
  }, [daemonWsBaseUrl, fixtureModeEnabled, roomId]);

  useEffect(() => {
    const view = editorViewRef.current;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: [
        setCommentHighlightRanges.of(commentRanges),
        setCommentGutterRanges.of(commentRanges),
      ],
    });
  }, [commentRanges]);

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }

    const view = editorViewRef.current;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: [
        setReconciliationInlineEntries.of(fixtureState.reconciliationEntries),
      ],
    });
  }, [fixtureModeEnabled, fixtureState.reconciliationEntries]);

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }

    const view = editorViewRef.current;
    const provider = collaborationProviderRef.current;
    if (!view || !provider) {
      return;
    }

    const currentText = view.state.doc.toString();
    if (currentText !== fixtureState.docContent) {
      view.dispatch({
        changes: {
          from: 0,
          insert: fixtureState.docContent,
          to: view.state.doc.length,
        },
      });
    }

    const yLength = provider.yText.length;
    if (yLength > 0) {
      provider.yText.delete(0, yLength);
    }
    if (fixtureState.docContent.length > 0) {
      provider.yText.insert(0, fixtureState.docContent);
    }
  }, [fixtureModeEnabled, fixtureState.docContent]);

  const scrubToTimelineIndex = (nextIndex: number) => {
    const snapshot = timelineEntries[nextIndex]?.content;
    if (snapshot === undefined) {
      return;
    }

    setTimelineIndex(nextIndex);
    const view = editorViewRef.current;
    if (!view) {
      return;
    }

    const currentContent = view.state.doc.toString();
    if (currentContent === snapshot) {
      return;
    }

    isApplyingTimelineSnapshotRef.current = true;
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: snapshot,
      },
    });
    isApplyingTimelineSnapshotRef.current = false;
  };

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }

    const provider = collaborationProviderRef.current;
    if (!provider) {
      return;
    }

    const awareness = provider.provider.awareness;
    const states = awareness.getStates();
    const previousClientIds = fixtureRemoteClientIdsRef.current;
    for (const clientId of previousClientIds) {
      states.delete(clientId);
    }

    const nextClientIds: number[] = [];
    fixtureState.remotePeers.forEach((peer, index) => {
      const clientId = FIXTURE_REMOTE_CLIENT_ID_BASE + index;
      const cursorOffset = cursorOffsetFromLineCh(
        fixtureState.docContent,
        peer.cursor,
      );
      states.set(clientId, {
        cursor: { anchor: cursorOffset, head: cursorOffset },
        user: {
          color: nameToColor(peer.name),
          name: peer.name,
        },
      });
      nextClientIds.push(clientId);
    });
    fixtureRemoteClientIdsRef.current = nextClientIds;

    awareness.emit("change", [
      {
        added: nextClientIds.filter(
          (clientId) => !previousClientIds.includes(clientId),
        ),
        removed: previousClientIds.filter(
          (clientId) => !nextClientIds.includes(clientId),
        ),
        updated: nextClientIds.filter((clientId) =>
          previousClientIds.includes(clientId),
        ),
      },
      "fixture",
    ]);
  }, [fixtureModeEnabled, fixtureState.docContent, fixtureState.remotePeers]);

  useEffect(() => {
    setEditingMessageId(null);
    setEditingMessageBody("");
  }, [activeThread?.id, isCommentPopoverOpen]);

  const persistCommentThreads = (
    mutator: (threads: readonly InlineCommentThread[]) => InlineCommentThread[],
  ) => {
    setInlineCommentThreads((currentThreads) => {
      const nextThreads = mutator(currentThreads);
      if (fixtureModeEnabled) {
        window.__SCRIPTUM_TEST__?.setCommentThreads(nextThreads);
      }
      return nextThreads;
    });
  };

  const submitInlineComment = () => {
    if (!activeSelection) {
      return;
    }
    const messageBody = pendingCommentBody.trim();
    if (!messageBody) {
      return;
    }

    const nextMessage: InlineCommentMessage = {
      authorName: LOCAL_COMMENT_AUTHOR_NAME,
      authorUserId: LOCAL_COMMENT_AUTHOR_ID,
      bodyMd: messageBody,
      createdAt: new Date(Date.now()).toISOString(),
      id: makeClientId("message"),
      isOwn: true,
    };

    const activeThreadId = activeThread?.id;
    persistCommentThreads((currentThreads) => {
      if (activeThreadId) {
        return appendReplyToThread(currentThreads, activeThreadId, nextMessage);
      }

      const nextThread: InlineCommentThread = {
        endOffsetUtf16: activeSelection.to,
        id: makeClientId("thread"),
        messages: [nextMessage],
        startOffsetUtf16: activeSelection.from,
        status: "open",
      };
      return [...currentThreads, nextThread];
    });
    setPendingCommentBody("");
    if (!activeThread) {
      setCommentPopoverOpen(false);
    }
  };

  const beginEditingMessage = (message: InlineCommentMessage) => {
    if (!message.isOwn) {
      return;
    }
    setEditingMessageId(message.id);
    setEditingMessageBody(message.bodyMd);
  };

  const saveEditedMessage = () => {
    const threadId = activeThread?.id;
    const messageId = editingMessageId;
    if (!threadId || !messageId) {
      return;
    }

    persistCommentThreads((currentThreads) =>
      updateInlineCommentMessageBody(
        currentThreads,
        threadId,
        messageId,
        editingMessageBody,
      ),
    );
    setEditingMessageId(null);
    setEditingMessageBody("");
  };

  const setThreadStatus = (
    threadId: string,
    status: InlineCommentThread["status"],
  ) => {
    persistCommentThreads((currentThreads) =>
      updateInlineCommentThreadStatus(currentThreads, threadId, status),
    );
  };

  const resolveThread = (threadId: string) => {
    setThreadStatus(threadId, "resolved");
  };

  const reopenThread = (threadId: string) => {
    setThreadStatus(threadId, "open");
  };

  const openShareDialog = () => {
    setGeneratedShareUrl("");
    setShareDialogOpen(true);
  };

  const closeShareDialog = () => {
    setShareDialogOpen(false);
  };

  const generateShareLink = () => {
    if (!workspaceId || typeof window === "undefined") {
      return;
    }

    const resolvedTargetType: ShareLinkTargetType =
      shareTargetType === "document" && documentId ? "document" : "workspace";
    const resolvedTargetId =
      resolvedTargetType === "document"
        ? (documentId ?? workspaceId)
        : workspaceId;
    const token = makeClientId("share");
    const record = createShareLinkRecord({
      token,
      targetType: resolvedTargetType,
      targetId: resolvedTargetId,
      permission: sharePermission,
      expiresAt: expirationIsoFromOption(shareExpirationOption),
      maxUses: parseShareLinkMaxUses(shareMaxUsesInput),
    });

    storeShareLinkRecord(record);
    setGeneratedShareUrl(buildShareLinkUrl(record.token, window.location.origin));
  };

  const selectTab = (nextDocumentId: string) => {
    if (!workspaceId) {
      return;
    }
    setActiveDocumentForWorkspace(workspaceId, nextDocumentId);
    navigate(`/workspace/${workspaceId}/document/${nextDocumentId}`);
  };

  const closeTab = (closingDocumentId: string) => {
    if (!workspaceId) {
      return;
    }
    const nextDocumentId = nextDocumentIdAfterClose(
      openTabs,
      closingDocumentId,
    );
    closeDocument(closingDocumentId);

    if (closingDocumentId !== documentId) {
      return;
    }

    setActiveDocumentForWorkspace(workspaceId, nextDocumentId);
    if (nextDocumentId) {
      navigate(`/workspace/${workspaceId}/document/${nextDocumentId}`);
      return;
    }
    navigate(`/workspace/${workspaceId}`);
  };

  const activeTimelineEntry =
    timelineEntries[timelineIndex] ??
    timelineEntries[timelineEntries.length - 1] ??
    createTimelineSnapshotEntry("", LOCAL_TIMELINE_AUTHOR);
  const latestTimelineEntry =
    timelineEntries[timelineEntries.length - 1] ??
    createTimelineSnapshotEntry("", LOCAL_TIMELINE_AUTHOR);
  const timelineAuthorshipSegments = useMemo(
    () => buildAuthorshipSegments(activeTimelineEntry),
    [activeTimelineEntry],
  );
  const timelineDiffSegments = useMemo(
    () =>
      buildTimelineDiffSegments(
        latestTimelineEntry.content,
        activeTimelineEntry.content,
      ),
    [activeTimelineEntry.content, latestTimelineEntry.content],
  );
  const timelineHasDiff = useMemo(
    () =>
      timelineDiffSegments.some(
        (segment) => segment.kind === "added" || segment.kind === "removed",
      ),
    [timelineDiffSegments],
  );
  const timelineLegendAuthors = useMemo(() => {
    const authorsById = new Map<string, TimelineAuthor>();
    timelineAuthorshipSegments.forEach((segment) => {
      authorsById.set(segment.author.id, segment.author);
    });
    return Array.from(authorsById.values());
  }, [timelineAuthorshipSegments]);

  return (
    <section aria-label="Document workspace">
      <h1 data-testid="document-title">
        Document: {workspaceId ?? "unknown"}/{documentId ?? "unknown"}
      </h1>
      <TabBar
        activeDocumentId={documentId ?? null}
        onCloseTab={closeTab}
        onSelectTab={selectTab}
        tabs={openTabs}
      />
      <Breadcrumb path={currentDocumentPath} workspaceLabel={workspaceLabel} />
      <OfflineBanner status={syncState} reconnectProgress={reconnectProgress} />

      {shareLinksEnabled ? (
        <section aria-label="Share links" data-testid="share-links-panel">
          <h2>Share Links</h2>
          <button
            data-testid="share-link-open"
            onClick={openShareDialog}
            type="button"
          >
            Share
          </button>

          {isShareDialogOpen ? (
            <section
              aria-label="Share link dialog"
              data-testid="share-link-dialog"
              style={{
                background: "#ffffff",
                border: "1px solid #d1d5db",
                borderRadius: "0.5rem",
                marginTop: "0.5rem",
                maxWidth: "26rem",
                padding: "0.75rem",
              }}
            >
              <label htmlFor="share-link-target">Target</label>
              <select
                data-testid="share-link-target"
                id="share-link-target"
                onChange={(event) =>
                  setShareTargetType(
                    event.target.value === "workspace" ? "workspace" : "document",
                  )
                }
                value={shareTargetType}
              >
                <option value="workspace">Workspace</option>
                <option disabled={!documentId} value="document">
                  Document
                </option>
              </select>

              <label htmlFor="share-link-permission">Permission</label>
              <select
                data-testid="share-link-permission"
                id="share-link-permission"
                onChange={(event) =>
                  setSharePermission(
                    event.target.value === "edit" ? "edit" : "view",
                  )
                }
                value={sharePermission}
              >
                <option value="view">Viewer</option>
                <option value="edit">Editor</option>
              </select>

              <label htmlFor="share-link-expiration">Expiration</label>
              <select
                data-testid="share-link-expiration"
                id="share-link-expiration"
                onChange={(event) =>
                  setShareExpirationOption(
                    event.target.value === "24h"
                      ? "24h"
                      : event.target.value === "7d"
                        ? "7d"
                        : "none",
                  )
                }
                value={shareExpirationOption}
              >
                <option value="none">Never</option>
                <option value="24h">24 hours</option>
                <option value="7d">7 days</option>
              </select>

              <label htmlFor="share-link-max-uses">Max uses</label>
              <input
                data-testid="share-link-max-uses"
                id="share-link-max-uses"
                min={1}
                onChange={(event) => setShareMaxUsesInput(event.target.value)}
                type="number"
                value={shareMaxUsesInput}
              />

              <div
                style={{
                  display: "flex",
                  gap: "0.5rem",
                  justifyContent: "flex-end",
                  marginTop: "0.5rem",
                }}
              >
                <button
                  data-testid="share-link-close"
                  onClick={closeShareDialog}
                  type="button"
                >
                  Close
                </button>
                <button
                  data-testid="share-link-generate"
                  onClick={generateShareLink}
                  type="button"
                >
                  Generate link
                </button>
              </div>

              {generatedShareUrl ? (
                <div style={{ marginTop: "0.5rem" }}>
                  <p data-testid="share-link-summary">
                    Link grants {sharePermissionLabel(sharePermission)} access
                    to{" "}
                    {shareTargetType === "workspace" ? "workspace" : "document"}.
                  </p>
                  <input
                    data-testid="share-link-url"
                    readOnly
                    value={generatedShareUrl}
                  />
                </div>
              ) : null}
            </section>
          ) : null}
        </section>
      ) : null}

      <section aria-label="Editor surface" data-testid="editor-surface">
        <h2>Editor</h2>
        {fixtureModeEnabled ? (
          <pre data-testid="editor-content">{fixtureState.docContent}</pre>
        ) : null}

        {dropUploadProgress ? (
          <p
            aria-live="polite"
            data-testid="drop-upload-progress"
            role="status"
            style={{
              background:
                dropUploadProgress.phase === "completed" &&
                dropUploadProgress.failedFiles > 0
                  ? "#fee2e2"
                  : "#dbeafe",
              border:
                dropUploadProgress.phase === "completed" &&
                dropUploadProgress.failedFiles > 0
                  ? "1px solid #fca5a5"
                  : "1px solid #93c5fd",
              borderRadius: "0.375rem",
              color:
                dropUploadProgress.phase === "completed" &&
                dropUploadProgress.failedFiles > 0
                  ? "#991b1b"
                  : "#1e3a8a",
              fontSize: "0.8rem",
              marginBottom: "0.5rem",
              padding: "0.375rem 0.5rem",
            }}
          >
            {formatDropUploadProgress(dropUploadProgress)}
          </p>
        ) : null}

        <div style={{ position: "relative" }}>
          <div
            data-testid="editor-host"
            ref={editorHostRef}
            style={{
              border: "1px solid #d1d5db",
              borderRadius: "0.5rem",
              minHeight: "20rem",
              overflow: "hidden",
            }}
          />

          {activeSelection ? (
            <button
              aria-label={
                activeThread?.status === "resolved"
                  ? "Resolved comment thread"
                  : "Add comment"
              }
              data-testid="comment-margin-button"
              onClick={() => setCommentPopoverOpen((isOpen) => !isOpen)}
              style={{
                alignItems: "center",
                background:
                  activeThread?.status === "resolved" ? "#f3f4f6" : "#fde68a",
                border:
                  activeThread?.status === "resolved"
                    ? "1px solid #9ca3af"
                    : "1px solid #f59e0b",
                borderRadius: "9999px",
                cursor: "pointer",
                display: "inline-flex",
                fontSize: "0.75rem",
                fontWeight: 600,
                gap: "0.25rem",
                minHeight: "1.5rem",
                minWidth:
                  activeThread?.status === "resolved" ? "1.5rem" : undefined,
                padding:
                  activeThread?.status === "resolved"
                    ? "0.25rem"
                    : "0.25rem 0.5rem",
                position: "absolute",
                right: "0.5rem",
                top: `${commentAnchorTop}px`,
              }}
              type="button"
            >
              {activeThread?.status === "resolved" ? (
                <span
                  aria-hidden="true"
                  data-testid="comment-margin-resolved-dot"
                  style={{
                    background: "#6b7280",
                    borderRadius: "9999px",
                    display: "inline-block",
                    height: "0.5rem",
                    width: "0.5rem",
                  }}
                />
              ) : (
                "Comment"
              )}
            </button>
          ) : null}

          {isCommentPopoverOpen && activeSelection ? (
            <section
              aria-label="Comment popover"
              data-testid="comment-popover"
              style={{
                background: "#ffffff",
                border: "1px solid #d1d5db",
                borderRadius: "0.5rem",
                boxShadow: "0 8px 18px rgba(15, 23, 42, 0.12)",
                maxWidth: "20rem",
                padding: "0.75rem",
                position: "absolute",
                right: "0.5rem",
                top: `${commentAnchorTop + 32}px`,
                width: "100%",
                zIndex: 1,
              }}
            >
              <p
                data-testid="comment-selection-preview"
                style={{
                  background: "rgba(250, 204, 21, 0.28)",
                  borderRadius: "0.25rem",
                  fontSize: "0.75rem",
                  margin: "0 0 0.5rem",
                  padding: "0.375rem",
                }}
              >
                {activeSelection.selectedText}
              </p>

              {activeThread ? (
                activeThread.status === "resolved" ? (
                  <section
                    aria-label="Collapsed resolved thread"
                    data-testid="comment-thread-collapsed"
                    style={{
                      alignItems: "center",
                      borderBottom: "1px solid #e5e7eb",
                      display: "flex",
                      gap: "0.5rem",
                      marginBottom: "0.5rem",
                      paddingBottom: "0.5rem",
                    }}
                  >
                    <span
                      aria-hidden="true"
                      data-testid="comment-thread-collapsed-dot"
                      style={{
                        background: "#6b7280",
                        borderRadius: "9999px",
                        display: "inline-block",
                        flexShrink: 0,
                        height: "0.45rem",
                        width: "0.45rem",
                      }}
                    />
                    <span style={{ color: "#4b5563", fontSize: "0.75rem" }}>
                      Thread resolved
                    </span>
                    <button
                      data-testid="comment-thread-reopen"
                      onClick={() => reopenThread(activeThread.id)}
                      style={{ marginLeft: "auto" }}
                      type="button"
                    >
                      Reopen
                    </button>
                  </section>
                ) : (
                  <section
                    aria-label="Thread replies"
                    data-testid="comment-thread-replies"
                    style={{
                      borderBottom: "1px solid #e5e7eb",
                      marginBottom: "0.5rem",
                      maxHeight: "12rem",
                      overflowY: "auto",
                      paddingBottom: "0.5rem",
                    }}
                  >
                    <div
                      style={{
                        alignItems: "center",
                        display: "flex",
                        justifyContent: "space-between",
                        marginBottom: "0.5rem",
                      }}
                    >
                      <strong style={{ fontSize: "0.75rem" }}>Thread</strong>
                      <button
                        data-testid="comment-thread-resolve"
                        onClick={() => resolveThread(activeThread.id)}
                        type="button"
                      >
                        Resolve
                      </button>
                    </div>
                    {activeThread.messages.length === 0 ? (
                      <p
                        style={{
                          color: "#64748b",
                          fontSize: "0.75rem",
                          margin: 0,
                        }}
                      >
                        No replies yet.
                      </p>
                    ) : (
                      <ol style={{ listStyle: "none", margin: 0, padding: 0 }}>
                        {activeThread.messages.map((message) => {
                          const isEditing = editingMessageId === message.id;
                          return (
                            <li
                              key={message.id}
                              style={{
                                border: "1px solid #e5e7eb",
                                borderRadius: "0.375rem",
                                marginBottom: "0.375rem",
                                padding: "0.375rem",
                              }}
                            >
                              <div
                                style={{
                                  alignItems: "center",
                                  display: "flex",
                                  fontSize: "0.75rem",
                                  gap: "0.375rem",
                                  justifyContent: "space-between",
                                  marginBottom: "0.25rem",
                                }}
                              >
                                <strong>{message.authorName}</strong>
                                <time dateTime={message.createdAt}>
                                  {message.createdAt}
                                </time>
                              </div>
                              {isEditing ? (
                                <>
                                  <textarea
                                    data-testid="comment-edit-input"
                                    onChange={(event) =>
                                      setEditingMessageBody(event.target.value)
                                    }
                                    rows={3}
                                    style={{ display: "block", width: "100%" }}
                                    value={editingMessageBody}
                                  />
                                  <div
                                    style={{
                                      display: "flex",
                                      gap: "0.5rem",
                                      justifyContent: "flex-end",
                                      marginTop: "0.375rem",
                                    }}
                                  >
                                    <button
                                      data-testid="comment-edit-cancel"
                                      onClick={() => {
                                        setEditingMessageId(null);
                                        setEditingMessageBody("");
                                      }}
                                      type="button"
                                    >
                                      Cancel
                                    </button>
                                    <button
                                      data-testid="comment-edit-save"
                                      onClick={saveEditedMessage}
                                      type="button"
                                    >
                                      Save
                                    </button>
                                  </div>
                                </>
                              ) : (
                                <>
                                  <p style={{ margin: 0 }}>{message.bodyMd}</p>
                                  {message.isOwn ? (
                                    <button
                                      data-testid={`comment-edit-${message.id}`}
                                      onClick={() =>
                                        beginEditingMessage(message)
                                      }
                                      style={{ marginTop: "0.25rem" }}
                                      type="button"
                                    >
                                      Edit
                                    </button>
                                  ) : null}
                                </>
                              )}
                            </li>
                          );
                        })}
                      </ol>
                    )}
                  </section>
                )
              ) : null}

              {canComposeInActiveThread ? (
                <>
                  <label htmlFor="inline-comment-input">
                    {activeThread ? "Reply" : "Comment"}
                  </label>
                  <textarea
                    data-testid="comment-input"
                    id="inline-comment-input"
                    onChange={(event) =>
                      setPendingCommentBody(event.target.value)
                    }
                    rows={3}
                    style={{
                      display: "block",
                      marginTop: "0.25rem",
                      width: "100%",
                    }}
                    value={pendingCommentBody}
                  />
                </>
              ) : (
                <p
                  data-testid="comment-thread-resolved-note"
                  style={{
                    color: "#6b7280",
                    fontSize: "0.75rem",
                    margin: "0 0 0.5rem",
                  }}
                >
                  This thread is resolved.
                </p>
              )}

              <div
                style={{
                  display: "flex",
                  gap: "0.5rem",
                  justifyContent: "flex-end",
                  marginTop: "0.5rem",
                }}
              >
                <button
                  onClick={() => setCommentPopoverOpen(false)}
                  type="button"
                >
                  Cancel
                </button>
                {canComposeInActiveThread ? (
                  <button
                    data-testid="comment-submit"
                    onClick={submitInlineComment}
                    type="button"
                  >
                    {activeThread ? "Add reply" : "Add comment"}
                  </button>
                ) : null}
              </div>
            </section>
          ) : null}
        </div>
      </section>

      <section aria-label="Comment threads" data-testid="comment-threads">
        <h2>Comments</h2>
        {inlineCommentThreads.length === 0 ? (
          <p>No comments yet.</p>
        ) : (
          <ul>
            {inlineCommentThreads.map((thread) => (
              <li key={thread.id}>
                <div
                  style={{
                    alignItems: "center",
                    display: "flex",
                    gap: "0.5rem",
                    marginBottom: "0.25rem",
                  }}
                >
                  <strong>
                    {thread.status === "resolved" ? "Resolved" : "Open"}
                  </strong>{" "}
                  <span>
                    ({thread.startOffsetUtf16}-{thread.endOffsetUtf16})
                  </span>
                  <button
                    data-testid={
                      thread.status === "resolved"
                        ? `comment-thread-reopen-${thread.id}`
                        : `comment-thread-resolve-${thread.id}`
                    }
                    onClick={() =>
                      thread.status === "resolved"
                        ? reopenThread(thread.id)
                        : resolveThread(thread.id)
                    }
                    type="button"
                  >
                    {thread.status === "resolved" ? "Reopen" : "Resolve"}
                  </button>
                </div>
                {thread.status === "resolved" ? (
                  <div
                    data-testid={`comment-thread-collapsed-${thread.id}`}
                    style={{
                      alignItems: "center",
                      color: "#6b7280",
                      display: "inline-flex",
                      fontSize: "0.75rem",
                      gap: "0.375rem",
                    }}
                  >
                    <span
                      aria-hidden="true"
                      data-testid={`comment-thread-collapsed-dot-${thread.id}`}
                      style={{
                        background: "#6b7280",
                        borderRadius: "9999px",
                        display: "inline-block",
                        height: "0.4rem",
                        width: "0.4rem",
                      }}
                    />
                    Collapsed in margin
                  </div>
                ) : (
                  thread.messages.map((message) => (
                    <article key={message.id}>
                      <p style={{ marginBottom: "0.125rem" }}>
                        <strong>{message.authorName}</strong>{" "}
                        <time dateTime={message.createdAt}>
                          {message.createdAt}
                        </time>
                      </p>
                      <p style={{ marginTop: 0 }}>{message.bodyMd}</p>
                    </article>
                  ))
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Presence stack" data-testid="presence-stack">
        <h2>Presence</h2>
        <AvatarStack peers={presencePeers} />
        {fixtureState.remotePeers.length === 0 ? (
          <p>No collaborators connected.</p>
        ) : (
          <ul>
            {fixtureState.remotePeers.map((peer) => (
              <li key={`${peer.name}-${peer.cursor.line}-${peer.cursor.ch}`}>
                {peer.name} ({peer.type})
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Section overlap" data-testid="overlap-indicator">
        <h2>Section overlap</h2>
        <p
          data-severity={overlapSummary.severity}
          data-testid="overlap-severity"
        >
          Severity: {overlapSummary.severity}
        </p>
        {overlapSummary.severity === "warning" ? (
          <ul>
            {overlapSummary.warningSections.map(({ peers, section }) => (
              <li key={section}>
                <strong>{section}</strong>
                <div>
                  {peers.map((peer) => (
                    <span
                      data-peer-type={peer.type}
                      data-testid={`attribution-badge-${badgeSuffix(peer.name)}`}
                      key={`${section}:${peer.name}`}
                      style={{
                        backgroundColor:
                          peer.type === "agent" ? "#dbeafe" : "#dcfce7",
                        border: "1px solid #93c5fd",
                        borderRadius: "9999px",
                        display: "inline-flex",
                        fontSize: "0.7rem",
                        fontWeight: 700,
                        marginRight: "0.375rem",
                        padding: "0.1rem 0.45rem",
                      }}
                    >
                      {peer.type === "agent" ? "AGENT" : "HUMAN"}: {peer.name}
                    </span>
                  ))}
                </div>
              </li>
            ))}
          </ul>
        ) : overlapSummary.severity === "info" ? (
          <div>
            <p>No direct section collisions detected.</p>
            <div>
              {fixtureState.remotePeers.map((peer) => (
                <span
                  data-peer-type={peer.type}
                  data-testid={`attribution-badge-${badgeSuffix(peer.name)}`}
                  key={`info:${peer.name}`}
                  style={{
                    backgroundColor:
                      peer.type === "agent" ? "#dbeafe" : "#dcfce7",
                    border: "1px solid #93c5fd",
                    borderRadius: "9999px",
                    display: "inline-flex",
                    fontSize: "0.7rem",
                    fontWeight: 700,
                    marginRight: "0.375rem",
                    padding: "0.1rem 0.45rem",
                  }}
                >
                  {peer.type === "agent" ? "AGENT" : "HUMAN"}: {peer.name}
                </span>
              ))}
            </div>
          </div>
        ) : (
          <p>Awaiting collaborators.</p>
        )}
      </section>

      <StatusBar
        syncState={syncState}
        cursor={cursor}
        activeEditors={activeEditors}
        pendingUpdates={pendingSyncUpdates}
        reconnectProgress={reconnectProgress}
      />
      <section
        aria-label="History browsing view"
        data-testid="history-view-panel"
        style={{
          borderTop: "1px solid #d1d5db",
          marginTop: "0.75rem",
          paddingTop: "0.5rem",
        }}
      >
        {timelineViewMode === "authorship" ? (
          <>
            <h3 style={{ fontSize: "0.875rem", margin: "0 0 0.375rem" }}>
              Author-colored highlights
            </h3>
            <p
              style={{
                color: "#4b5563",
                fontSize: "0.75rem",
                margin: "0 0 0.375rem",
              }}
            >
              Scrub snapshots to inspect per-character attribution.
            </p>
            <div
              data-testid="history-authorship-legend"
              style={{
                alignItems: "center",
                display: "flex",
                flexWrap: "wrap",
                gap: "0.375rem",
                marginBottom: "0.375rem",
              }}
            >
              {timelineLegendAuthors.map((author) => (
                <span
                  data-testid={`history-authorship-author-${author.id}`}
                  key={author.id}
                  style={{
                    alignItems: "center",
                    border: `1px solid ${author.color}66`,
                    borderRadius: "9999px",
                    color: author.color,
                    display: "inline-flex",
                    fontSize: "0.7rem",
                    fontWeight: 700,
                    gap: "0.25rem",
                    padding: "0.1rem 0.4rem",
                  }}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      background: author.color,
                      borderRadius: "9999px",
                      display: "inline-block",
                      height: "0.4rem",
                      width: "0.4rem",
                    }}
                  />
                  {author.name}
                </span>
              ))}
            </div>
            {timelineAuthorshipSegments.length === 0 ? (
              <p data-testid="history-authorship-empty" style={{ margin: 0 }}>
                No content yet.
              </p>
            ) : (
              <pre
                data-testid="history-authorship-preview"
                style={{
                  background: "#f8fafc",
                  border: "1px solid #e5e7eb",
                  borderRadius: "0.375rem",
                  fontFamily:
                    "ui-monospace, SFMono-Regular, SFMono, Menlo, monospace",
                  fontSize: "0.8rem",
                  margin: 0,
                  overflowX: "auto",
                  padding: "0.5rem",
                  whiteSpace: "pre-wrap",
                }}
              >
                {timelineAuthorshipSegments.map((segment, index) => (
                  <span
                    data-testid={`history-authorship-segment-${index}`}
                    key={`${segment.author.id}:${index}`}
                    style={{ color: segment.author.color }}
                  >
                    {segment.text}
                  </span>
                ))}
              </pre>
            )}
          </>
        ) : (
          <>
            <h3 style={{ fontSize: "0.875rem", margin: "0 0 0.375rem" }}>
              Diff from current
            </h3>
            <p
              style={{
                color: "#4b5563",
                fontSize: "0.75rem",
                margin: "0 0 0.375rem",
              }}
            >
              Comparing the selected snapshot against the latest document state.
            </p>
            {!timelineHasDiff ? (
              <p data-testid="history-diff-empty" style={{ margin: 0 }}>
                Selected snapshot matches current version.
              </p>
            ) : (
              <pre
                data-testid="history-diff-preview"
                style={{
                  background: "#f8fafc",
                  border: "1px solid #e5e7eb",
                  borderRadius: "0.375rem",
                  fontFamily:
                    "ui-monospace, SFMono-Regular, SFMono, Menlo, monospace",
                  fontSize: "0.8rem",
                  margin: 0,
                  overflowX: "auto",
                  padding: "0.5rem",
                  whiteSpace: "pre-wrap",
                }}
              >
                {timelineDiffSegments.map((segment, index) => (
                  <span
                    data-kind={segment.kind}
                    data-testid={`history-diff-segment-${index}`}
                    key={`${segment.kind}:${index}`}
                    style={{
                      background:
                        segment.kind === "added"
                          ? "#dcfce7"
                          : segment.kind === "removed"
                            ? "#fee2e2"
                            : "transparent",
                      color:
                        segment.kind === "added"
                          ? "#166534"
                          : segment.kind === "removed"
                            ? "#991b1b"
                            : "#1f2937",
                      textDecoration:
                        segment.kind === "removed" ? "line-through" : "none",
                    }}
                  >
                    {segment.text}
                  </span>
                ))}
              </pre>
            )}
          </>
        )}
      </section>
      <TimelineSlider
        max={timelineEntries.length - 1}
        onChange={scrubToTimelineIndex}
        onViewModeChange={setTimelineViewMode}
        value={timelineIndex}
        viewMode={timelineViewMode}
      />
    </section>
  );
}
