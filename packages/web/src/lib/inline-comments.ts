import type { CommentDecorationRange } from "@scriptum/editor";
import type { CommentMessage } from "@scriptum/shared";
import type { ThreadWithMessages } from "../components/comments/CommentPopover";

export const LOCAL_COMMENT_AUTHOR_ID = "local-user";
export const LOCAL_COMMENT_AUTHOR_NAME = "You";
export const UNKNOWN_COMMENT_AUTHOR_NAME = "Unknown";
export const UNKNOWN_COMMENT_TIMESTAMP = "1970-01-01T00:00:00.000Z";

interface UnknownRecord {
  [key: string]: unknown;
}

export interface InlineCommentMessage {
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

export function toCommentMessage(
  message: InlineCommentMessage,
  threadId: string,
): CommentMessage {
  return {
    author: message.authorName,
    bodyMd: message.bodyMd,
    createdAt: message.createdAt,
    editedAt: null,
    id: message.id,
    threadId,
  };
}

function toInlineCommentMessage(message: CommentMessage): InlineCommentMessage {
  const isOwn = message.author === LOCAL_COMMENT_AUTHOR_NAME;
  return {
    authorName: message.author,
    authorUserId: isOwn ? LOCAL_COMMENT_AUTHOR_ID : undefined,
    bodyMd: message.bodyMd,
    createdAt: message.createdAt,
    id: message.id,
    isOwn,
  };
}

export function toThreadWithMessages(
  thread: InlineCommentThread,
  documentId: string | undefined,
): ThreadWithMessages {
  return {
    messages: thread.messages.map((message) =>
      toCommentMessage(message, thread.id),
    ),
    thread: {
      createdAt: thread.messages[0]?.createdAt ?? UNKNOWN_COMMENT_TIMESTAMP,
      createdBy: thread.messages[0]?.authorUserId ?? LOCAL_COMMENT_AUTHOR_ID,
      docId: documentId ?? "",
      endOffsetUtf16: thread.endOffsetUtf16,
      id: thread.id,
      resolvedAt:
        thread.status === "resolved"
          ? (thread.messages[thread.messages.length - 1]?.createdAt ??
            UNKNOWN_COMMENT_TIMESTAMP)
          : null,
      sectionId: null,
      startOffsetUtf16: thread.startOffsetUtf16,
      status: thread.status,
      version: 1,
    },
  };
}

export function toInlineCommentThread(
  threadWithMessages: ThreadWithMessages,
): InlineCommentThread {
  return {
    endOffsetUtf16: threadWithMessages.thread.endOffsetUtf16,
    id: threadWithMessages.thread.id,
    messages: threadWithMessages.messages.map((message) =>
      toInlineCommentMessage(message),
    ),
    startOffsetUtf16: threadWithMessages.thread.startOffsetUtf16,
    status: threadWithMessages.thread.status,
  };
}
