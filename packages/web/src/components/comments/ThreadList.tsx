import type { CommentMessage, CommentThread } from "@scriptum/shared";
import clsx from "clsx";
import { Fragment, type ReactNode, useEffect, useState } from "react";
import {
  addCommentMessage,
  reopenCommentThread,
  resolveCommentThread,
} from "../../lib/api-client";
import controls from "../../styles/Controls.module.css";
import styles from "./ThreadList.module.css";

export interface ThreadListProps {
  workspaceId: string;
  thread: CommentThread;
  messages: CommentMessage[];
  onMessageCreated?: (message: CommentMessage) => void;
  onThreadUpdated?: (thread: CommentThread) => void;
  replyToThread?: (
    workspaceId: string,
    threadId: string,
    bodyMd: string,
  ) => Promise<CommentMessage>;
  resolveThread?: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
  ) => Promise<CommentThread>;
  reopenThread?: (
    workspaceId: string,
    threadId: string,
    ifVersion: number,
  ) => Promise<CommentThread>;
}

const DEFAULT_ERROR_MESSAGE = "Comment request failed. Try again.";

function renderMarkdownInline(input: string): ReactNode[] {
  const tokenPattern = /(\*\*[^*]+\*\*|`[^`]+`|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g;
  const parts = input.split(tokenPattern).filter((part) => part.length > 0);

  return parts.map((part, index) => {
    if (part.startsWith("**") && part.endsWith("**") && part.length >= 4) {
      return <strong key={`strong-${index}`}>{part.slice(2, -2)}</strong>;
    }
    if (part.startsWith("`") && part.endsWith("`") && part.length >= 2) {
      return (
        <code className={styles.inlineCode} key={`code-${index}`}>
          {part.slice(1, -1)}
        </code>
      );
    }
    if (part.startsWith("*") && part.endsWith("*") && part.length >= 2) {
      return <em key={`em-${index}`}>{part.slice(1, -1)}</em>;
    }
    const linkMatch = part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    if (linkMatch) {
      return (
        <a
          className={styles.inlineLink}
          key={`link-${index}`}
          href={linkMatch[2]}
          rel="noreferrer"
          target="_blank"
        >
          {linkMatch[1]}
        </a>
      );
    }
    return <Fragment key={`text-${index}`}>{part}</Fragment>;
  });
}

export function renderMarkdownBody(markdownBody: string): ReactNode {
  const blocks = markdownBody.split(/\n{2,}/);
  return blocks.map((block, blockIndex) => {
    const lines = block.split("\n");
    return (
      <p className={styles.markdownParagraph} key={`block-${blockIndex}`}>
        {lines.map((line, lineIndex) => (
          <Fragment key={`line-${lineIndex}`}>
            {lineIndex > 0 ? <br /> : null}
            {renderMarkdownInline(line)}
          </Fragment>
        ))}
      </p>
    );
  });
}

export function ThreadList({
  workspaceId,
  thread,
  messages,
  onMessageCreated,
  onThreadUpdated,
  replyToThread = addCommentMessage,
  resolveThread = resolveCommentThread,
  reopenThread = reopenCommentThread,
}: ThreadListProps) {
  const [replyBody, setReplyBody] = useState("");
  const [pending, setPending] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [localThread, setLocalThread] = useState<CommentThread>(thread);

  useEffect(() => {
    setLocalThread(thread);
  }, [thread]);

  const canReply = localThread.status === "open";

  const submitReply = async () => {
    const bodyMd = replyBody.trim();
    if (!canReply || bodyMd.length === 0 || pending) {
      return;
    }

    setPending(true);
    setErrorMessage(null);
    try {
      const created = await replyToThread(workspaceId, localThread.id, bodyMd);
      onMessageCreated?.(created);
      setReplyBody("");
    } catch {
      setErrorMessage(DEFAULT_ERROR_MESSAGE);
    } finally {
      setPending(false);
    }
  };

  const transitionThread = async (nextStatus: "resolved" | "open") => {
    if (pending) {
      return;
    }

    setPending(true);
    setErrorMessage(null);
    try {
      const nextThread =
        nextStatus === "resolved"
          ? await resolveThread(
              workspaceId,
              localThread.id,
              localThread.version,
            )
          : await reopenThread(
              workspaceId,
              localThread.id,
              localThread.version,
            );
      setLocalThread(nextThread);
      onThreadUpdated?.(nextThread);
    } catch {
      setErrorMessage(DEFAULT_ERROR_MESSAGE);
    } finally {
      setPending(false);
    }
  };

  return (
    <section
      aria-label="Thread replies"
      className={styles.threadList}
      data-testid="thread-list"
    >
      <div className={styles.header}>
        <strong className={styles.title}>Thread</strong>
        <button
          className={clsx(
            controls.buttonBase,
            localThread.status === "resolved"
              ? controls.buttonSecondary
              : controls.buttonDanger,
          )}
          data-testid={
            localThread.status === "resolved"
              ? "thread-list-reopen"
              : "thread-list-resolve"
          }
          disabled={pending}
          onClick={() =>
            void transitionThread(
              localThread.status === "resolved" ? "open" : "resolved",
            )
          }
          type="button"
        >
          {localThread.status === "resolved" ? "Reopen" : "Resolve"}
        </button>
      </div>

      {localThread.status === "resolved" ? (
        <p className={styles.resolvedNote} data-testid="thread-list-resolved-note">
          This thread is resolved.
        </p>
      ) : null}

      {localThread.status === "open" && messages.length === 0 ? (
        <p className={styles.emptyState} data-testid="thread-list-empty">
          No replies yet.
        </p>
      ) : null}

      {localThread.status === "open" && messages.length > 0 ? (
        <ol className={styles.messageList} data-testid="thread-list-messages">
          {messages.map((message) => (
            <li className={styles.messageItem} key={message.id}>
              <div className={styles.messageMeta}>
                <strong>{message.author}</strong>
                <time dateTime={message.createdAt}>{message.createdAt}</time>
              </div>
              <div data-testid={`thread-list-message-${message.id}`}>
                {renderMarkdownBody(message.bodyMd)}
              </div>
            </li>
          ))}
        </ol>
      ) : null}

      {canReply ? (
        <>
          <label className={styles.replyLabel} htmlFor="thread-list-reply-input">
            Reply
          </label>
          <textarea
            className={controls.textArea}
            data-testid="thread-list-reply-input"
            id="thread-list-reply-input"
            onChange={(event) => setReplyBody(event.target.value)}
            rows={3}
            value={replyBody}
          />
        </>
      ) : null}

      {errorMessage ? (
        <p className={styles.errorMessage} data-testid="thread-list-error">
          {errorMessage}
        </p>
      ) : null}

      {canReply ? (
        <div className={styles.actions}>
          <button
            className={clsx(controls.buttonBase, controls.buttonPrimary)}
            data-testid="thread-list-reply-submit"
            disabled={pending || replyBody.trim().length === 0}
            onClick={() => void submitReply()}
            type="button"
          >
            Add reply
          </button>
        </div>
      ) : null}
    </section>
  );
}
