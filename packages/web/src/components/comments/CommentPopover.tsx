import { Popover } from "@base-ui-components/react/popover";
import type { CommentMessage, CommentThread } from "@scriptum/shared";
import clsx from "clsx";
import { useState } from "react";
import { type CreateCommentInput, createComment } from "../../lib/api-client";
import controls from "../../styles/Controls.module.css";
import styles from "./CommentPopover.module.css";
import { ThreadList, type ThreadListProps } from "./ThreadList";

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
  replyToThread?: ThreadListProps["replyToThread"];
  resolveThread?: ThreadListProps["resolveThread"];
  reopenThread?: ThreadListProps["reopenThread"];
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

function matchesSelection(
  thread: ThreadWithMessages,
  selection: InlineCommentSelection,
): boolean {
  return (
    thread.thread.startOffsetUtf16 === selection.startOffsetUtf16 &&
    thread.thread.endOffsetUtf16 === selection.endOffsetUtf16 &&
    (thread.thread.sectionId ?? null) === selection.sectionId
  );
}

export function CommentPopover({
  workspaceId,
  documentId,
  selection,
  anchorTopPx,
  activeThread,
  onThreadChange,
  createThread = createComment,
  replyToThread,
  resolveThread,
  reopenThread,
}: CommentPopoverProps) {
  const [isOpen, setOpen] = useState(false);
  const [pendingBody, setPendingBody] = useState("");
  const [pending, setPending] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [threadOverride, setThreadOverride] =
    useState<ThreadWithMessages | null>(null);

  if (!selection) {
    return null;
  }

  const localThread =
    threadOverride && matchesSelection(threadOverride, selection)
      ? threadOverride
      : null;
  const threadState =
    localThread &&
    (!activeThread || localThread.thread.id === activeThread.thread.id)
      ? localThread
      : activeThread;
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
      setThreadOverride(nextThread);
      onThreadChange?.(nextThread);
      setPendingBody("");
    } catch {
      setErrorMessage("Failed to create comment thread.");
    } finally {
      setPending(false);
    }
  };

  return (
    <Popover.Root
      modal="trap-focus"
      onOpenChange={(open) => setOpen(open)}
      open={isOpen}
    >
      <Popover.Trigger
        aria-label={isResolved ? "Resolved comment thread" : "Add comment"}
        className={clsx(
          styles.marginButton,
          isResolved ? styles.marginButtonResolved : styles.marginButtonOpen,
        )}
        data-testid="comment-margin-button"
        style={{ top: `${anchorTopPx}px` }}
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
      </Popover.Trigger>

      <Popover.Portal>
        <Popover.Positioner align="end" side="bottom" sideOffset={8}>
          <Popover.Popup
            aria-label="Comment popover"
            className={styles.popover}
            data-testid="comment-popover"
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
                  setThreadOverride((current) => {
                    const base = current ?? threadState;
                    if (!base) {
                      return current;
                    }

                    const next = {
                      ...base,
                      messages: [...base.messages, message],
                    };
                    onThreadChange?.(next);
                    return next;
                  });
                }}
                onThreadUpdated={(thread) => {
                  setThreadOverride((current) => {
                    const base = current ?? threadState;
                    const next = { messages: base?.messages ?? [], thread };
                    onThreadChange?.(next);
                    return next;
                  });
                }}
                reopenThread={reopenThread}
                replyToThread={replyToThread}
                resolveThread={resolveThread}
                thread={threadState.thread}
                workspaceId={workspaceId}
              />
            ) : (
              <>
                <label
                  className={styles.commentLabel}
                  htmlFor="inline-comment-input"
                >
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
              <Popover.Close
                className={clsx(controls.buttonBase, controls.buttonSecondary)}
              >
                Close
              </Popover.Close>
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
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
