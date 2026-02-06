import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  commentGutterExtension,
  commentHighlightExtension,
  createCollaborationProvider,
  livePreviewExtension,
  remoteCursorExtension,
  setCommentGutterRanges,
  setCommentHighlightRanges,
  type CommentDecorationRange,
} from "@scriptum/editor";
import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { StatusBar } from "../components/StatusBar";
import type { ScriptumTestState } from "../test/harness";

const DEFAULT_DAEMON_WS_BASE_URL =
  (import.meta.env.VITE_SCRIPTUM_DAEMON_WS_URL as string | undefined) ??
  "ws://127.0.0.1:39091/yjs";

const DEFAULT_TEST_STATE: ScriptumTestState = {
  fixtureName: "default",
  docContent: "# Fixture Document",
  cursor: { line: 0, ch: 0 },
  remotePeers: [],
  syncState: "synced",
  gitStatus: { dirty: false, ahead: 0, behind: 0 },
  commentThreads: [],
};

interface UnknownRecord {
  [key: string]: unknown;
}

interface InlineCommentMessage {
  bodyMd: string;
  id: string;
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

function asRecord(value: unknown): UnknownRecord | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  return value as UnknownRecord;
}

function readNumber(record: UnknownRecord, keys: readonly string[]): number | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return null;
}

function readString(record: UnknownRecord, keys: readonly string[]): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }
  return null;
}

function normalizeInlineCommentMessages(value: unknown): InlineCommentMessage[] {
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

    messages.push({
      bodyMd,
      id,
    });
  }

  return messages;
}

function normalizeInlineCommentThread(value: unknown): InlineCommentThread | null {
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
    record.messages ?? record.message ?? threadRecord.messages
  );

  return {
    endOffsetUtf16,
    id,
    messages,
    startOffsetUtf16,
    status,
  };
}

export function normalizeInlineCommentThreads(values: unknown[]): InlineCommentThread[] {
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
  threads: readonly InlineCommentThread[]
): CommentDecorationRange[] {
  return threads.map((thread) => ({
    from: thread.startOffsetUtf16,
    status: thread.status,
    threadId: thread.id,
    to: thread.endOffsetUtf16,
  }));
}

export function commentAnchorTopPx(line: number): number {
  if (!Number.isFinite(line)) {
    return 12;
  }
  return Math.max(12, (Math.max(1, Math.floor(line)) - 1) * 22 + 12);
}

function readFixtureState(): ScriptumTestState {
  if (typeof window === "undefined" || !window.__SCRIPTUM_TEST__) {
    return DEFAULT_TEST_STATE;
  }
  return window.__SCRIPTUM_TEST__.getState();
}

function makeClientId(prefix: string): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `${prefix}-${crypto.randomUUID()}`;
  }
  return `${prefix}-${Math.random().toString(16).slice(2)}`;
}

export function DocumentRoute() {
  const { workspaceId, documentId } = useParams();
  const [fixtureState, setFixtureState] = useState<ScriptumTestState>(() =>
    readFixtureState()
  );
  const fixtureModeEnabled =
    typeof window !== "undefined" && Boolean(window.__SCRIPTUM_TEST__);
  const [inlineCommentThreads, setInlineCommentThreads] = useState<InlineCommentThread[]>(
    () => normalizeInlineCommentThreads(readFixtureState().commentThreads)
  );
  const [activeSelection, setActiveSelection] = useState<ActiveTextSelection | null>(
    null
  );
  const [isCommentPopoverOpen, setCommentPopoverOpen] = useState(false);
  const [pendingCommentBody, setPendingCommentBody] = useState("");
  const activeEditors = fixtureModeEnabled
    ? fixtureState.remotePeers.length + 1
    : 1;
  const [syncState, setSyncState] = useState<ScriptumTestState["syncState"]>(
    fixtureModeEnabled ? fixtureState.syncState : "reconnecting"
  );
  const [cursor, setCursor] = useState(fixtureState.cursor);
  const [daemonWsBaseUrl] = useState(DEFAULT_DAEMON_WS_BASE_URL);
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const collaborationProviderRef = useRef<
    ReturnType<typeof createCollaborationProvider> | null
  >(null);
  const roomId = useMemo(
    () => `${workspaceId ?? "unknown-workspace"}:${documentId ?? "unknown-document"}`,
    [workspaceId, documentId]
  );
  const commentRanges = useMemo(
    () => commentRangesFromThreads(inlineCommentThreads),
    [inlineCommentThreads]
  );
  const commentAnchorTop = activeSelection
    ? commentAnchorTopPx(activeSelection.line)
    : 12;

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
      normalizeInlineCommentThreads(fixtureState.commentThreads)
    );
  }, [
    fixtureModeEnabled,
    fixtureState.commentThreads,
    fixtureState.cursor,
    fixtureState.syncState,
  ]);

  useEffect(() => {
    const host = editorHostRef.current;
    if (!host) {
      return;
    }

    host.innerHTML = "";
    const provider = createCollaborationProvider({
      connectOnCreate: false,
      room: roomId,
      url: daemonWsBaseUrl,
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

    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: fixtureState.docContent,
        extensions: [
          markdown(),
          livePreviewExtension(),
          commentHighlightExtension(),
          commentGutterExtension(),
          provider.extension(),
          remoteCursorExtension({ awareness: provider.provider.awareness }),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (!update.selectionSet) {
              return;
            }

            const mainSelection = update.state.selection.main;
            const line = update.state.doc.lineAt(mainSelection.head);
            setCursor({ ch: mainSelection.head - line.from, line: line.number - 1 });

            if (mainSelection.empty) {
              setActiveSelection(null);
              setCommentPopoverOpen(false);
              return;
            }

            const selectedText = update.state.sliceDoc(
              mainSelection.from,
              mainSelection.to
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

  const submitInlineComment = () => {
    if (!activeSelection) {
      return;
    }
    const messageBody = pendingCommentBody.trim();
    if (!messageBody) {
      return;
    }

    const nextThread: InlineCommentThread = {
      endOffsetUtf16: activeSelection.to,
      id: makeClientId("thread"),
      messages: [
        {
          bodyMd: messageBody,
          id: makeClientId("message"),
        },
      ],
      startOffsetUtf16: activeSelection.from,
      status: "open",
    };

    setInlineCommentThreads((currentThreads) => {
      const nextThreads = [...currentThreads, nextThread];
      if (fixtureModeEnabled) {
        window.__SCRIPTUM_TEST__?.setCommentThreads(nextThreads);
      }
      return nextThreads;
    });
    setPendingCommentBody("");
    setCommentPopoverOpen(false);
  };

  return (
    <section aria-label="Document workspace">
      <h1 data-testid="document-title">
        Document: {workspaceId ?? "unknown"}/{documentId ?? "unknown"}
      </h1>

      <section aria-label="Editor surface" data-testid="editor-surface">
        <h2>Editor</h2>
        {fixtureModeEnabled ? (
          <pre data-testid="editor-content">{fixtureState.docContent}</pre>
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
              data-testid="comment-margin-button"
              onClick={() => setCommentPopoverOpen((isOpen) => !isOpen)}
              style={{
                background: "#fde68a",
                border: "1px solid #f59e0b",
                borderRadius: "9999px",
                cursor: "pointer",
                fontSize: "0.75rem",
                fontWeight: 600,
                padding: "0.25rem 0.5rem",
                position: "absolute",
                right: "0.5rem",
                top: `${commentAnchorTop}px`,
              }}
              type="button"
            >
              Comment
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

              <label htmlFor="inline-comment-input">Comment</label>
              <textarea
                data-testid="comment-input"
                id="inline-comment-input"
                onChange={(event) => setPendingCommentBody(event.target.value)}
                rows={3}
                style={{ display: "block", marginTop: "0.25rem", width: "100%" }}
                value={pendingCommentBody}
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
                  onClick={() => setCommentPopoverOpen(false)}
                  type="button"
                >
                  Cancel
                </button>
                <button
                  data-testid="comment-submit"
                  onClick={submitInlineComment}
                  type="button"
                >
                  Add comment
                </button>
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
                <strong>{thread.status === "resolved" ? "Resolved" : "Open"}</strong>{" "}
                <span>
                  ({thread.startOffsetUtf16}-{thread.endOffsetUtf16})
                </span>
                {thread.messages.map((message) => (
                  <p key={message.id}>{message.bodyMd}</p>
                ))}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Presence stack" data-testid="presence-stack">
        <h2>Presence</h2>
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

      <StatusBar syncState={syncState} cursor={cursor} activeEditors={activeEditors} />
    </section>
  );
}
