import type { CommentMessage, CommentThread } from "@scriptum/shared";
import { Fragment, useEffect, useState, type ReactNode } from "react";
import {
  addCommentMessage,
  reopenCommentThread,
  resolveCommentThread,
} from "../../lib/api-client";

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
        <code
          key={`code-${index}`}
          style={{
            background: "#f3f4f6",
            borderRadius: "0.25rem",
            fontSize: "0.75rem",
            padding: "0.1rem 0.2rem",
          }}
        >
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
          key={`link-${index}`}
          href={linkMatch[2]}
          rel="noreferrer"
          style={{ color: "#1d4ed8" }}
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
      <p key={`block-${blockIndex}`} style={{ margin: "0 0 0.25rem" }}>
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
          ? await resolveThread(workspaceId, localThread.id, localThread.version)
          : await reopenThread(workspaceId, localThread.id, localThread.version);
      setLocalThread(nextThread);
      onThreadUpdated?.(nextThread);
    } catch {
      setErrorMessage(DEFAULT_ERROR_MESSAGE);
    } finally {
      setPending(false);
    }
  };

  return (
    <section aria-label="Thread replies" data-testid="thread-list">
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
          data-testid={
            localThread.status === "resolved"
              ? "thread-list-reopen"
              : "thread-list-resolve"
          }
          disabled={pending}
          onClick={() =>
            void transitionThread(localThread.status === "resolved" ? "open" : "resolved")
          }
          type="button"
        >
          {localThread.status === "resolved" ? "Reopen" : "Resolve"}
        </button>
      </div>

      {localThread.status === "resolved" ? (
        <p
          data-testid="thread-list-resolved-note"
          style={{ color: "#6b7280", fontSize: "0.75rem", margin: "0 0 0.5rem" }}
        >
          This thread is resolved.
        </p>
      ) : null}

      {localThread.status === "open" && messages.length === 0 ? (
        <p data-testid="thread-list-empty" style={{ color: "#64748b", fontSize: "0.75rem", margin: 0 }}>
          No replies yet.
        </p>
      ) : null}

      {localThread.status === "open" && messages.length > 0 ? (
        <ol
          data-testid="thread-list-messages"
          style={{ listStyle: "none", margin: "0 0 0.5rem", maxHeight: "12rem", overflowY: "auto", padding: 0 }}
        >
          {messages.map((message) => (
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
          <label htmlFor="thread-list-reply-input">Reply</label>
          <textarea
            data-testid="thread-list-reply-input"
            id="thread-list-reply-input"
            onChange={(event) => setReplyBody(event.target.value)}
            rows={3}
            style={{ display: "block", marginTop: "0.25rem", width: "100%" }}
            value={replyBody}
          />
        </>
      ) : null}

      {errorMessage ? (
        <p data-testid="thread-list-error" style={{ color: "#b91c1c", fontSize: "0.75rem", margin: "0.5rem 0 0" }}>
          {errorMessage}
        </p>
      ) : null}

      {canReply ? (
        <div style={{ display: "flex", justifyContent: "flex-end", marginTop: "0.5rem" }}>
          <button
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
