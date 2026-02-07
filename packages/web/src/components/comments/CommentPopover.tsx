import type { CommentMessage, CommentThread } from "@scriptum/shared";
import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { type CreateCommentInput, createComment } from "../../lib/api-client";
import controls from "../../styles/Controls.module.css";
import styles from "./CommentPopover.module.css";
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
  const marginButtonRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    setThreadState(activeThread);
  }, [activeThread]);

  useEffect(() => {
    marginButtonRef.current?.style.setProperty("top", `${anchorTopPx}px`);
    popoverRef.current?.style.setProperty("top", `${anchorTopPx + 32}px`);
  }, [anchorTopPx, isOpen]);

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
        className={clsx(
          styles.marginButton,
          isResolved ? styles.marginButtonResolved : styles.marginButtonOpen,
        )}
        data-testid="comment-margin-button"
        onClick={() => setOpen((current) => !current)}
        ref={marginButtonRef}
        type="button"
      >
        {isResolved ? (
          <span
            aria-hidden="true"
            className={styles.resolvedDot}
            data-testid="comment-margin-resolved-dot"
          />
        ) : (
          "Comment"
        )}
      </button>

      {isOpen ? (
        <section
          aria-label="Comment popover"
          className={styles.popover}
          data-testid="comment-popover"
          ref={popoverRef}
        >
          <p
            className={styles.selectionPreview}
            data-testid="comment-selection-preview"
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
                  const next = {
                    ...current,
                    messages: [...current.messages, message],
                  };
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
              <label className={styles.commentLabel} htmlFor="inline-comment-input">
                Comment
              </label>
              <textarea
                className={controls.textArea}
                data-testid="comment-input"
                id="inline-comment-input"
                onChange={(event) => setPendingBody(event.target.value)}
                rows={3}
                value={pendingBody}
              />
            </>
          )}

          {errorMessage ? (
            <p
              className={styles.errorMessage}
              data-testid="comment-popover-error"
            >
              {errorMessage}
            </p>
          ) : null}

          <div className={styles.actions}>
            <button
              className={clsx(controls.buttonBase, controls.buttonSecondary)}
              onClick={() => setOpen(false)}
              type="button"
            >
              Close
            </button>
            {!threadState ? (
              <button
                className={clsx(controls.buttonBase, controls.buttonPrimary)}
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
