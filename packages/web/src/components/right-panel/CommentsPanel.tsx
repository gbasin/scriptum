import type { Document } from "@scriptum/shared";
import { useMemo } from "react";
import type { InlineCommentThread } from "../../lib/inline-comments";
import { commentsStoreKey } from "../../store/comments";
import styles from "./CommentsPanel.module.css";

interface CommentThreadEntry {
  documentId: string;
  documentPath: string;
  documentTitle: string;
  thread: InlineCommentThread;
}

export interface CommentsPanelProps {
  activeDocumentId: string | null;
  documents: readonly Document[];
  onThreadSelect?: (documentId: string, threadId: string) => void;
  threadsByDocumentKey: Record<string, InlineCommentThread[]>;
  workspaceId: string | null;
}

function parseTimestamp(iso: string): number {
  const timestamp = Date.parse(iso);
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function messagePreview(thread: InlineCommentThread): string {
  const latestMessage = thread.messages[thread.messages.length - 1];
  if (!latestMessage) {
    return "No messages yet.";
  }
  const normalized = latestMessage.bodyMd.replace(/\s+/g, " ").trim();
  if (normalized.length <= 120) {
    return normalized;
  }
  return `${normalized.slice(0, 117)}...`;
}

function latestAuthor(thread: InlineCommentThread): string {
  return thread.messages[thread.messages.length - 1]?.authorName ?? "Unknown";
}

function buildThreadEntries(
  workspaceId: string | null,
  documents: readonly Document[],
  activeDocumentId: string | null,
  threadsByDocumentKey: Record<string, InlineCommentThread[]>,
): CommentThreadEntry[] {
  if (!workspaceId) {
    return [];
  }

  const workspaceDocuments = documents.filter(
    (document) =>
      document.workspaceId === workspaceId && document.deletedAt === null,
  );

  const activeDocumentEntries = activeDocumentId
    ? (() => {
        const activeDocument = workspaceDocuments.find(
          (document) => document.id === activeDocumentId,
        );
        if (!activeDocument) {
          return [];
        }
        const threads =
          threadsByDocumentKey[
            commentsStoreKey(workspaceId, activeDocument.id)
          ] ?? [];
        return threads.map((thread) => ({
          documentId: activeDocument.id,
          documentPath: activeDocument.path,
          documentTitle: activeDocument.title,
          thread,
        }));
      })()
    : [];

  if (activeDocumentEntries.length > 0) {
    return activeDocumentEntries;
  }

  const entries: CommentThreadEntry[] = [];
  for (const document of workspaceDocuments) {
    const threads =
      threadsByDocumentKey[commentsStoreKey(workspaceId, document.id)] ?? [];
    for (const thread of threads) {
      entries.push({
        documentId: document.id,
        documentPath: document.path,
        documentTitle: document.title,
        thread,
      });
    }
  }
  return entries;
}

export function CommentsPanel({
  activeDocumentId,
  documents,
  onThreadSelect,
  threadsByDocumentKey,
  workspaceId,
}: CommentsPanelProps) {
  const entries = useMemo(
    () =>
      buildThreadEntries(
        workspaceId,
        documents,
        activeDocumentId,
        threadsByDocumentKey,
      ).sort((left, right) => {
        const leftTimestamp = parseTimestamp(
          left.thread.messages[left.thread.messages.length - 1]?.createdAt ??
            "",
        );
        const rightTimestamp = parseTimestamp(
          right.thread.messages[right.thread.messages.length - 1]?.createdAt ??
            "",
        );
        return rightTimestamp - leftTimestamp;
      }),
    [activeDocumentId, documents, threadsByDocumentKey, workspaceId],
  );

  return (
    <section
      aria-label="Comments panel"
      className={styles.root}
      data-testid="comments-panel"
    >
      {entries.length === 0 ? (
        <p className={styles.emptyState} data-testid="comments-panel-empty">
          No comments yet.
        </p>
      ) : (
        <ul className={styles.threadList} data-testid="comments-panel-list">
          {entries.map((entry) => (
            <li className={styles.threadListItem} key={entry.thread.id}>
              <button
                className={styles.threadButton}
                data-testid={`comments-panel-thread-${entry.thread.id}`}
                onClick={() =>
                  onThreadSelect?.(entry.documentId, entry.thread.id)
                }
                type="button"
              >
                <div className={styles.threadMeta}>
                  <strong>
                    {entry.thread.status === "resolved" ? "Resolved" : "Open"}
                  </strong>
                  <span>{latestAuthor(entry.thread)}</span>
                  <span>
                    {entry.thread.startOffsetUtf16}-
                    {entry.thread.endOffsetUtf16}
                  </span>
                </div>
                <p className={styles.threadPreview}>
                  {messagePreview(entry.thread)}
                </p>
                <p className={styles.threadDocument}>
                  {entry.documentTitle} · {entry.documentPath}
                </p>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
