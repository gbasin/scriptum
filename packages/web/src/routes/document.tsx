import { EditorView } from "@codemirror/view";
import {
  type DropUploadProgress,
  nameToColor,
  setReconciliationInlineEntries,
  type WebRtcProviderFactory,
} from "@scriptum/editor";
import type {
  CommentMessage,
  CommentThread,
  WorkspaceEditorFontFamily,
} from "@scriptum/shared";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { AvatarStack } from "../components/AvatarStack";
import { CommentPopover, type ThreadWithMessages } from "../components/comments/CommentPopover";
import { Breadcrumb } from "../components/editor/Breadcrumb";
import { TabBar } from "../components/editor/TabBar";
import {
  type HistoryViewMode,
  TimelineSlider,
} from "../components/editor/TimelineSlider";
import { DiffView } from "../components/history/DiffView";
import { OfflineBanner } from "../components/OfflineBanner";
import { SkeletonBlock } from "../components/Skeleton";
import { StatusBar } from "../components/StatusBar";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { ShareDialog } from "../components/share/ShareDialog";
import { useScriptumEditor } from "../hooks/useScriptumEditor";
import { useToast } from "../hooks/useToast";
import type { CreateCommentInput } from "../lib/api-client";
import {
  buildOpenDocumentTabs,
  nextDocumentIdAfterClose,
} from "../lib/document-utils";
import {
  appendReplyToThread,
  commentAnchorTopPx,
  commentRangesFromThreads,
  LOCAL_COMMENT_AUTHOR_ID,
  LOCAL_COMMENT_AUTHOR_NAME,
  normalizeInlineCommentThreads,
  toCommentMessage,
  toInlineCommentThread,
  toThreadWithMessages,
  type InlineCommentMessage,
  type InlineCommentThread,
  UNKNOWN_COMMENT_TIMESTAMP,
  updateInlineCommentMessageBody,
  updateInlineCommentThreadStatus,
} from "../lib/inline-comments";
import {
  authorshipMapFromTimelineEntry,
  buildAuthorshipSegments,
  buildTimelineDiffSegments,
  createTimelineSnapshotEntry,
  deriveTimelineSnapshotEntry,
  LOCAL_TIMELINE_AUTHOR,
  timelineAuthorFromPeer,
  type TimelineSnapshotEntry,
  UNKNOWN_REMOTE_TIMELINE_AUTHOR,
} from "../lib/timeline";
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
  type ShareLinkExpirationOption,
  type ShareLinkPermission,
  type ShareLinkTargetType,
  sharePermissionLabel,
  storeShareLinkRecord,
} from "./share-links";
import styles from "./document.module.css";

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

const DEFAULT_DAEMON_WS_BASE_URL =
  (import.meta.env.VITE_SCRIPTUM_DAEMON_WS_URL as string | undefined) ??
  "ws://127.0.0.1:39091/yjs";
const DEFAULT_WEBRTC_SIGNALING_URL =
  (import.meta.env.VITE_SCRIPTUM_WEBRTC_SIGNALING_URL as string | undefined) ??
  null;
const REALTIME_E2E_MODE =
  (import.meta.env.VITE_SCRIPTUM_REALTIME_E2E as string | undefined) === "1";
const FIXTURE_REMOTE_CLIENT_ID_BASE = 10_000;
const MAX_TIMELINE_SNAPSHOTS = 240;
const DROP_UPLOAD_SUCCESS_HIDE_DELAY_MS = 2_000;
const DROP_UPLOAD_FAILURE_HIDE_DELAY_MS = 4_000;
const DEFAULT_EDITOR_FONT_FAMILY: WorkspaceEditorFontFamily = "mono";
const DEFAULT_EDITOR_TAB_SIZE = 2;
const DEFAULT_EDITOR_LINE_NUMBERS = true;
const MIN_EDITOR_TAB_SIZE = 1;
const MAX_EDITOR_TAB_SIZE = 8;

interface EditorRuntimeConfig {
  fontFamily: WorkspaceEditorFontFamily;
  tabSize: number;
  lineNumbers: boolean;
}

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

interface ActiveTextSelection {
  from: number;
  line: number;
  selectedText: string;
  to: number;
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

function resolveEditorFontFamily(value: unknown): WorkspaceEditorFontFamily {
  return value === "sans" || value === "serif" || value === "mono"
    ? value
    : DEFAULT_EDITOR_FONT_FAMILY;
}

function resolveEditorTabSize(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isFinite(value) ||
    !Number.isInteger(value)
  ) {
    return DEFAULT_EDITOR_TAB_SIZE;
  }
  return Math.max(MIN_EDITOR_TAB_SIZE, Math.min(MAX_EDITOR_TAB_SIZE, value));
}

function resolveEditorLineNumbers(value: unknown): boolean {
  return typeof value === "boolean" ? value : DEFAULT_EDITOR_LINE_NUMBERS;
}

function editorFontFamilyStack(fontFamily: WorkspaceEditorFontFamily): string {
  if (fontFamily === "sans") {
    return "var(--font-sans)";
  }
  if (fontFamily === "serif") {
    return 'ui-serif, "Iowan Old Style", "Times New Roman", serif';
  }
  return "var(--font-mono)";
}

function editorTypographyTheme(fontFamily: WorkspaceEditorFontFamily) {
  const fontStack = editorFontFamilyStack(fontFamily);
  return EditorView.theme({
    "&": { fontFamily: fontStack },
    ".cm-content": { fontFamily: fontStack },
    ".cm-gutters": { fontFamily: fontStack },
  });
}

function EditorRuntimeErrorThrower(props: { error: Error | null }) {
  if (props.error) {
    throw props.error;
  }
  return null;
}

export function DocumentRoute() {
  const { workspaceId, documentId } = useParams();
  const navigate = useNavigate();
  const toast = useToast();
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
  const [isShareDialogOpen, setShareDialogOpen] = useState(false);
  const [shareTargetType, setShareTargetType] =
    useState<ShareLinkTargetType>("document");
  const [sharePermission, setSharePermission] =
    useState<ShareLinkPermission>("view");
  const [shareExpirationOption, setShareExpirationOption] =
    useState<ShareLinkExpirationOption>("none");
  const [shareMaxUsesInput, setShareMaxUsesInput] = useState("3");
  const [generatedShareUrl, setGeneratedShareUrl] = useState("");
  const [shareGenerationError, setShareGenerationError] = useState<
    string | null
  >(null);
  const [dropUploadProgress, setDropUploadProgress] =
    useState<DropUploadProgress | null>(null);
  const activeEditors = fixtureModeEnabled
    ? fixtureState.remotePeers.length + 1
    : 1;
  const [syncState, setSyncState] = useState<ScriptumTestState["syncState"]>(
    fixtureModeEnabled ? fixtureState.syncState : "reconnecting",
  );
  const [editorRuntimeError, setEditorRuntimeError] = useState<Error | null>(
    null,
  );
  const setPresencePeers = usePresenceStore((state) => state.setPeers);
  const pendingChanges = useSyncStore((state) => state.pendingChanges);
  const [cursor, setCursor] = useState(fixtureState.cursor);
  const [daemonWsBaseUrl] = useState(DEFAULT_DAEMON_WS_BASE_URL);
  const [webrtcSignalingUrl] = useState(DEFAULT_WEBRTC_SIGNALING_URL);
  const [webrtcProviderFactory] = useState<WebRtcProviderFactory | undefined>(
    () => resolveGlobalWebRtcProviderFactory(),
  );
  const isApplyingTimelineSnapshotRef = useRef(false);
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
  const editorRuntimeConfig = useMemo<EditorRuntimeConfig>(() => {
    const editorConfig = workspaceId
      ? workspaces.find((workspace) => workspace.id === workspaceId)?.config
          ?.editor
      : undefined;

    return {
      fontFamily: resolveEditorFontFamily(editorConfig?.fontFamily),
      tabSize: resolveEditorTabSize(editorConfig?.tabSize),
      lineNumbers: resolveEditorLineNumbers(editorConfig?.lineNumbers),
    };
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
  const activeInlineThread = useMemo(() => {
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
  const activeCommentPopoverThread = useMemo(
    () =>
      activeInlineThread
        ? toThreadWithMessages(activeInlineThread, documentId)
        : null,
    [activeInlineThread, documentId],
  );
  const pendingSyncUpdates = fixtureModeEnabled
    ? fixtureState.pendingSyncUpdates
    : pendingChanges;
  const reconnectProgress = fixtureModeEnabled
    ? fixtureState.reconnectProgress
    : null;
  const shareLinksEnabled =
    fixtureModeEnabled && fixtureState.shareLinksEnabled;
  const showEditorLoadingSkeleton =
    !fixtureModeEnabled && syncState === "reconnecting";
  const handleEditorDocContentChanged = (
    nextContent: string,
    isRemoteTransaction: boolean,
  ) => {
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
        nextEntries.splice(0, nextEntries.length - MAX_TIMELINE_SNAPSHOTS);
      }

      setTimelineIndex(nextEntries.length - 1);
      return nextEntries;
    });
  };
  const { collaborationProviderRef, editorHostRef, editorViewRef } =
    useScriptumEditor({
      commentRanges,
      daemonWsBaseUrl,
      editorRuntimeConfig,
      fixtureDocContent: fixtureState.docContent,
      fixtureModeEnabled,
      isApplyingTimelineSnapshotRef,
      onActiveSelectionChanged: setActiveSelection,
      onCursorChanged: setCursor,
      onDocContentChanged: handleEditorDocContentChanged,
      onDropUploadProgressChanged: setDropUploadProgress,
      onEditorReady: () => {
        setTimelineEntries([
          createTimelineSnapshotEntry(
            fixtureState.docContent,
            LOCAL_TIMELINE_AUTHOR,
          ),
        ]);
        setTimelineIndex(0);
      },
      onEditorRuntimeError: setEditorRuntimeError,
      onSyncStateChanged: setSyncState,
      realtimeE2eMode: REALTIME_E2E_MODE,
      roomId,
      typographyTheme: editorTypographyTheme,
      uploadFile: uploadDroppedFileAsDataUrl,
      webrtcProviderFactory,
      webrtcSignalingUrl,
    });

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

  const createInlineCommentThread = async (
    _workspaceId: string,
    _documentId: string,
    input: CreateCommentInput,
  ): Promise<{ thread: CommentThread; message: CommentMessage }> => {
    const nextMessage: InlineCommentMessage = {
      authorName: LOCAL_COMMENT_AUTHOR_NAME,
      authorUserId: LOCAL_COMMENT_AUTHOR_ID,
      bodyMd: input.message.trim(),
      createdAt: new Date(Date.now()).toISOString(),
      id: makeClientId("message"),
      isOwn: true,
    };
    const nextThread: InlineCommentThread = {
      endOffsetUtf16: input.anchor.endOffsetUtf16,
      id: makeClientId("thread"),
      messages: [nextMessage],
      startOffsetUtf16: input.anchor.startOffsetUtf16,
      status: "open",
    };

    persistCommentThreads((currentThreads) => [...currentThreads, nextThread]);
    const created = toThreadWithMessages(nextThread, documentId);
    return {
      message:
        created.messages[0] ?? toCommentMessage(nextMessage, nextThread.id),
      thread: created.thread,
    };
  };

  const replaceInlineCommentThread = (nextThread: InlineCommentThread) => {
    persistCommentThreads((currentThreads) => {
      const nextIndex = currentThreads.findIndex(
        (thread) => thread.id === nextThread.id,
      );
      if (nextIndex === -1) {
        return [...currentThreads, nextThread];
      }

      const updated = [...currentThreads];
      updated[nextIndex] = nextThread;
      return updated;
    });
  };

  const handleCommentPopoverThreadChange = (thread: ThreadWithMessages) => {
    replaceInlineCommentThread(toInlineCommentThread(thread));
  };

  const replyToInlineCommentThread = async (
    _workspaceId: string,
    threadId: string,
    bodyMd: string,
  ): Promise<CommentMessage> => {
    const nextMessage: InlineCommentMessage = {
      authorName: LOCAL_COMMENT_AUTHOR_NAME,
      authorUserId: LOCAL_COMMENT_AUTHOR_ID,
      bodyMd: bodyMd.trim(),
      createdAt: new Date(Date.now()).toISOString(),
      id: makeClientId("message"),
      isOwn: true,
    };

    persistCommentThreads((currentThreads) =>
      appendReplyToThread(currentThreads, threadId, nextMessage),
    );
    return toCommentMessage(nextMessage, threadId);
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

  const resolveInlineCommentThread = async (
    _workspaceId: string,
    threadId: string,
    _ifVersion: number,
  ): Promise<CommentThread> => {
    let updatedThread: InlineCommentThread | null = null;
    persistCommentThreads((currentThreads) => {
      const nextThreads = updateInlineCommentThreadStatus(
        currentThreads,
        threadId,
        "resolved",
      );
      updatedThread =
        nextThreads.find((thread) => thread.id === threadId) ?? null;
      return nextThreads;
    });

    if (!updatedThread) {
      throw new Error("Thread not found");
    }

    return toThreadWithMessages(updatedThread, documentId).thread;
  };

  const reopenInlineCommentThread = async (
    _workspaceId: string,
    threadId: string,
    _ifVersion: number,
  ): Promise<CommentThread> => {
    let updatedThread: InlineCommentThread | null = null;
    persistCommentThreads((currentThreads) => {
      const nextThreads = updateInlineCommentThreadStatus(
        currentThreads,
        threadId,
        "open",
      );
      updatedThread =
        nextThreads.find((thread) => thread.id === threadId) ?? null;
      return nextThreads;
    });

    if (!updatedThread) {
      throw new Error("Thread not found");
    }

    return toThreadWithMessages(updatedThread, documentId).thread;
  };

  const openShareDialog = () => {
    setGeneratedShareUrl("");
    setShareGenerationError(null);
    setShareDialogOpen(true);
  };

  const closeShareDialog = () => {
    setShareDialogOpen(false);
    setShareGenerationError(null);
  };

  const generateShareLink = async () => {
    if (!workspaceId) {
      const message =
        "Cannot generate a share link without an active workspace.";
      setShareGenerationError(message);
      toast.error(message);
      return;
    }

    if (typeof window === "undefined") {
      const message = "Cannot generate a share link outside the browser.";
      setShareGenerationError(message);
      toast.error(message);
      return;
    }

    try {
      setShareGenerationError(null);

      const resolvedTargetType: ShareLinkTargetType =
        shareTargetType === "document" && documentId
          ? "document"
          : "workspace";
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

      await Promise.resolve(storeShareLinkRecord(record));
      setGeneratedShareUrl(
        buildShareLinkUrl(record.token, window.location.origin),
      );
      toast.success(
        `Generated ${resolvedTargetType === "document" ? "document" : "workspace"} share link.`,
      );
    } catch (error) {
      const message =
        error instanceof Error && error.message.trim().length > 0
          ? error.message
          : "Failed to generate share link. Please try again.";
      setShareGenerationError(message);
      toast.error(message);
    }
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
  const timelineAuthorshipMap = useMemo(
    () => authorshipMapFromTimelineEntry(activeTimelineEntry),
    [activeTimelineEntry],
  );

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
            <ShareDialog
              documentId={documentId}
              generationError={shareGenerationError}
              generatedShareUrl={generatedShareUrl}
              onClose={closeShareDialog}
              onExpirationOptionChange={setShareExpirationOption}
              onGenerate={generateShareLink}
              onMaxUsesInputChange={setShareMaxUsesInput}
              onPermissionChange={setSharePermission}
              onTargetTypeChange={setShareTargetType}
              shareExpirationOption={shareExpirationOption}
              shareMaxUsesInput={shareMaxUsesInput}
              sharePermission={sharePermission}
              shareTargetType={shareTargetType}
              summaryPermissionLabel={sharePermissionLabel(sharePermission)}
            />
          ) : null}
        </section>
      ) : null}

      <section aria-label="Editor surface" data-testid="editor-surface">
        <h2>Editor</h2>
        <ErrorBoundary
          inline
          message="The editor crashed, but the rest of the document view is still available."
          reloadLabel="Reload editor"
          testId="editor-error-boundary"
          title="Editor failed to load"
        >
          <EditorRuntimeErrorThrower error={editorRuntimeError} />
          {showEditorLoadingSkeleton ? (
            <div data-testid="editor-loading-skeleton">
              <div aria-hidden="true" className={styles.editorLoadingRows}>
                <SkeletonBlock className={styles.editorLoadingLineShort} />
                <SkeletonBlock className={styles.editorLoadingLineLong} />
                <SkeletonBlock className={styles.editorLoadingLineMedium} />
              </div>
            </div>
          ) : null}
          {fixtureModeEnabled ? (
            <pre data-testid="editor-content">{fixtureState.docContent}</pre>
          ) : null}

          {dropUploadProgress ? (
            <output
              aria-live="polite"
              data-testid="drop-upload-progress"
              className={clsx(
                styles.dropUploadProgress,
                dropUploadProgress.phase === "completed" &&
                  dropUploadProgress.failedFiles > 0
                  ? styles.dropUploadProgressError
                  : styles.dropUploadProgressInfo,
              )}
            >
              {formatDropUploadProgress(dropUploadProgress)}
            </output>
          ) : null}

          <div className={styles.editorContainer}>
            <div
              data-testid="editor-host"
              ref={editorHostRef}
              className={styles.editorHost}
            />

            <CommentPopover
              activeThread={activeCommentPopoverThread}
              anchorTopPx={commentAnchorTop}
              createThread={createInlineCommentThread}
              documentId={documentId ?? "unknown-document"}
              onThreadChange={handleCommentPopoverThreadChange}
              reopenThread={reopenInlineCommentThread}
              replyToThread={replyToInlineCommentThread}
              resolveThread={resolveInlineCommentThread}
              selection={
                activeSelection
                  ? {
                      sectionId: null,
                      startOffsetUtf16: activeSelection.from,
                      endOffsetUtf16: activeSelection.to,
                      headSeq: timelineIndex,
                      selectedText: activeSelection.selectedText,
                    }
                  : null
              }
              workspaceId={workspaceId ?? "unknown-workspace"}
            />
          </div>
        </ErrorBoundary>
      </section>

      <section aria-label="Comment threads" data-testid="comment-threads">
        <h2>Comments</h2>
        {inlineCommentThreads.length === 0 ? (
          <p>No comments yet.</p>
        ) : (
          <ul>
            {inlineCommentThreads.map((thread) => (
              <li key={thread.id}>
                <div className={styles.threadHeader}>
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
                    className={styles.threadCollapsed}
                  >
                    <span
                      aria-hidden="true"
                      data-testid={`comment-thread-collapsed-dot-${thread.id}`}
                      className={styles.threadCollapsedDot}
                    />
                    Collapsed in margin
                  </div>
                ) : (
                  thread.messages.map((message) => (
                    <article key={message.id}>
                      <p className={styles.threadMessageMeta}>
                        <strong>{message.authorName}</strong>{" "}
                        <time dateTime={message.createdAt}>
                          {message.createdAt}
                        </time>
                      </p>
                      <p className={styles.threadMessageBody}>
                        {message.bodyMd}
                      </p>
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
                      className={clsx(
                        styles.attributionBadge,
                        peer.type === "agent"
                          ? styles.attributionBadgeAgent
                          : styles.attributionBadgeHuman,
                      )}
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
                  className={clsx(
                    styles.attributionBadge,
                    peer.type === "agent"
                      ? styles.attributionBadgeAgent
                      : styles.attributionBadgeHuman,
                  )}
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
        className={styles.historyPanel}
      >
        <DiffView
          authorshipMap={timelineAuthorshipMap}
          currentContent={latestTimelineEntry.content}
          historicalContent={activeTimelineEntry.content}
          viewMode={timelineViewMode}
        />
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
