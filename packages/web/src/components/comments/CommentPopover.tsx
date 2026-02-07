import type { CommentMessage, CommentThread } from "@scriptum/shared";
import { useEffect, useState } from "react";
import {
  createComment,
  type CreateCommentInput,
} from "../../lib/api-client";
import { ThreadList } from "./ThreadList";

export interface InlineCommentSelection {
  sectionId: string | null;
  startOffsetUtf16: number;
  endOffsetUtf16: number;
  headSeq: number;
  selectedText: string;
}

export interface ThreadWithMessages {
  thread: CommentThread;
  messages: CommentMessage[];
}

export interface CommentPopoverProps {
  workspaceId: string;
  documentId: string;
  selection: InlineCommentSelection | null;
  anchorTopPx: number;
  activeThread: ThreadWithMessages | null;
  onThreadChange?: (thread: ThreadWithMessages) => void;
  createThread?: (
    workspaceId: string,
    documentId: string,
    input: CreateCommentInput,
  ) => Promise<{ thread: CommentThread; message: CommentMessage }>;
}

export interface CommentPopoverHighlightRange {
  from: number;
  threadId: string;
  to: number;
  status: "open" | "resolved";
}

export function highlightRangesFromThreads(
  threads: readonly ThreadWithMessages[],
): CommentPopoverHighlightRange[] {
  return threads.map((thread) => ({
    from: thread.thread.startOffsetUtf16,
    threadId: thread.thread.id,
    to: thread.thread.endOffsetUtf16,
    status: thread.thread.status,
  }));
}

export function CommentPopover({
  workspaceId,
  documentId,
  selection,
  anchorTopPx,
  activeThread,
  onThreadChange,
  createThread = createComment,
}: CommentPopoverProps) {
  const [isOpen, setOpen] = useState(false);
  const [pendingBody, setPendingBody] = useState("");
  const [pending, setPending] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [threadState, setThreadState] = useState<ThreadWithMessages | null>(
    activeThread,
  );

  useEffect(() => {
    setThreadState(activeThread);
  }, [activeThread]);

  if (!selection) {
    return null;
  }

  const isResolved = threadState?.thread.status === "resolved";

  const submitComment = async () => {
    const message = pendingBody.trim();
    if (message.length === 0 || pending || threadState) {
      return;
    }

    setPending(true);
    setErrorMessage(null);
    try {
      const result = await createThread(workspaceId, documentId, {
        anchor: {
          sectionId: selection.sectionId,
          startOffsetUtf16: selection.startOffsetUtf16,
          endOffsetUtf16: selection.endOffsetUtf16,
          headSeq: selection.headSeq,
        },
        message,
      });

      const nextThread: ThreadWithMessages = {
        thread: result.thread,
        messages: [result.message],
      };
      setThreadState(nextThread);
      onThreadChange?.(nextThread);
      setPendingBody("");
    } catch {
      setErrorMessage("Failed to create comment thread.");
    } finally {
      setPending(false);
    }
  };

  return (
    <>
      <button
        aria-label={isResolved ? "Resolved comment thread" : "Add comment"}
        data-testid="comment-margin-button"
        onClick={() => setOpen((current) => !current)}
        style={{
          alignItems: "center",
          background: isResolved ? "#f3f4f6" : "#fde68a",
          border: isResolved ? "1px solid #9ca3af" : "1px solid #f59e0b",
          borderRadius: "9999px",
          cursor: "pointer",
          display: "inline-flex",
          fontSize: "0.75rem",
          fontWeight: 600,
          minHeight: "1.5rem",
          minWidth: isResolved ? "1.5rem" : undefined,
          padding: isResolved ? "0.25rem" : "0.25rem 0.5rem",
          position: "absolute",
          right: "0.5rem",
          top: `${anchorTopPx}px`,
        }}
        type="button"
      >
        {isResolved ? (
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

      {isOpen ? (
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
            top: `${anchorTopPx + 32}px`,
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
            {selection.selectedText}
          </p>

          {threadState ? (
            <ThreadList
              messages={threadState.messages}
              onMessageCreated={(message) => {
                setThreadState((current) => {
                  if (!current) {
                    return current;
                  }
                  const next = { ...current, messages: [...current.messages, message] };
                  onThreadChange?.(next);
                  return next;
                });
              }}
              onThreadUpdated={(thread) => {
                setThreadState((current) => {
                  const next = { messages: current?.messages ?? [], thread };
                  onThreadChange?.(next);
                  return next;
                });
              }}
              thread={threadState.thread}
              workspaceId={workspaceId}
            />
          ) : (
            <>
              <label htmlFor="inline-comment-input">Comment</label>
              <textarea
                data-testid="comment-input"
                id="inline-comment-input"
                onChange={(event) => setPendingBody(event.target.value)}
                rows={3}
                style={{ display: "block", marginTop: "0.25rem", width: "100%" }}
                value={pendingBody}
              />
            </>
          )}

          {errorMessage ? (
            <p data-testid="comment-popover-error" style={{ color: "#b91c1c", fontSize: "0.75rem", margin: "0.5rem 0 0" }}>
              {errorMessage}
            </p>
          ) : null}

          <div
            style={{
              display: "flex",
              gap: "0.5rem",
              justifyContent: "flex-end",
              marginTop: "0.5rem",
            }}
          >
            <button onClick={() => setOpen(false)} type="button">
              Close
            </button>
            {!threadState ? (
              <button
                data-testid="comment-submit"
                disabled={pending || pendingBody.trim().length === 0}
                onClick={() => void submitComment()}
                type="button"
              >
                Add comment
              </button>
            ) : null}
          </div>
        </section>
      ) : null}
    </>
  );
}
