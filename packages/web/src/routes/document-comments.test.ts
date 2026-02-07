import { describe, expect, it } from "vitest";
import {
  appendReplyToThread,
  commentAnchorTopPx,
  commentRangesFromThreads,
  normalizeInlineCommentThreads,
  updateInlineCommentThreadStatus,
  updateInlineCommentMessageBody,
  type InlineCommentThread,
} from "./document";

describe("document comment helpers", () => {
  it("normalizes list responses with nested thread + messages shape", () => {
    const normalized = normalizeInlineCommentThreads([
      {
        messages: [
          {
            author_name: "You",
            author_user_id: "local-user",
            body_md: "Top level comment",
            created_at: "2026-02-06T20:00:00.000Z",
            id: "msg-1",
          },
        ],
        thread: {
          end_offset_utf16: 14,
          id: "thread-1",
          start_offset_utf16: 3,
          status: "open",
        },
      },
    ]);

    expect(normalized).toEqual<InlineCommentThread[]>([
      {
        endOffsetUtf16: 14,
        id: "thread-1",
        messages: [
          {
            authorName: "You",
            authorUserId: "local-user",
            bodyMd: "Top level comment",
            createdAt: "2026-02-06T20:00:00.000Z",
            id: "msg-1",
            isOwn: true,
          },
        ],
        startOffsetUtf16: 3,
        status: "open",
      },
    ]);
  });

  it("builds codemirror decoration ranges from threads", () => {
    const ranges = commentRangesFromThreads([
      {
        endOffsetUtf16: 12,
        id: "thread-open",
        messages: [],
        startOffsetUtf16: 5,
        status: "open",
      },
      {
        endOffsetUtf16: 20,
        id: "thread-resolved",
        messages: [],
        startOffsetUtf16: 14,
        status: "resolved",
      },
    ]);

    expect(ranges).toEqual([
      {
        from: 5,
        status: "open",
        threadId: "thread-open",
        to: 12,
      },
      {
        from: 14,
        status: "resolved",
        threadId: "thread-resolved",
        to: 20,
      },
    ]);
  });

  it("computes a stable anchor offset for margin controls", () => {
    expect(commentAnchorTopPx(1)).toBe(12);
    expect(commentAnchorTopPx(3)).toBe(56);
    expect(commentAnchorTopPx(0)).toBe(12);
  });

  it("appends threaded replies to an existing thread", () => {
    const threads: InlineCommentThread[] = [
      {
        endOffsetUtf16: 12,
        id: "thread-1",
        messages: [],
        startOffsetUtf16: 5,
        status: "open",
      },
    ];

    const next = appendReplyToThread(threads, "thread-1", {
      authorName: "You",
      authorUserId: "local-user",
      bodyMd: "Follow-up",
      createdAt: "2026-02-06T20:01:00.000Z",
      id: "msg-2",
      isOwn: true,
    });

    expect(next[0]?.messages).toEqual([
      {
        authorName: "You",
        authorUserId: "local-user",
        bodyMd: "Follow-up",
        createdAt: "2026-02-06T20:01:00.000Z",
        id: "msg-2",
        isOwn: true,
      },
    ]);
  });

  it("only edits messages owned by current user", () => {
    const threads: InlineCommentThread[] = [
      {
        endOffsetUtf16: 12,
        id: "thread-1",
        messages: [
          {
            authorName: "You",
            authorUserId: "local-user",
            bodyMd: "Original",
            createdAt: "2026-02-06T20:01:00.000Z",
            id: "msg-own",
            isOwn: true,
          },
          {
            authorName: "Alex",
            authorUserId: "user-2",
            bodyMd: "Teammate note",
            createdAt: "2026-02-06T20:02:00.000Z",
            id: "msg-other",
            isOwn: false,
          },
        ],
        startOffsetUtf16: 5,
        status: "open",
      },
    ];

    const afterOwnEdit = updateInlineCommentMessageBody(
      threads,
      "thread-1",
      "msg-own",
      "Updated"
    );
    expect(afterOwnEdit[0]?.messages[0]?.bodyMd).toBe("Updated");

    const afterOtherEdit = updateInlineCommentMessageBody(
      afterOwnEdit,
      "thread-1",
      "msg-other",
      "Should not change"
    );
    expect(afterOtherEdit[0]?.messages[1]?.bodyMd).toBe("Teammate note");
  });

  it("resolves and reopens comment thread status", () => {
    const threads: InlineCommentThread[] = [
      {
        endOffsetUtf16: 20,
        id: "thread-1",
        messages: [],
        startOffsetUtf16: 10,
        status: "open",
      },
    ];

    const resolved = updateInlineCommentThreadStatus(
      threads,
      "thread-1",
      "resolved"
    );
    expect(resolved[0]?.status).toBe("resolved");

    const reopened = updateInlineCommentThreadStatus(
      resolved,
      "thread-1",
      "open"
    );
    expect(reopened[0]?.status).toBe("open");
  });

  it("returns unchanged thread statuses when id is missing", () => {
    const threads: InlineCommentThread[] = [
      {
        endOffsetUtf16: 20,
        id: "thread-1",
        messages: [],
        startOffsetUtf16: 10,
        status: "open",
      },
    ];

    const next = updateInlineCommentThreadStatus(
      threads,
      "missing-thread",
      "resolved"
    );
    expect(next).toEqual(threads);
  });
});
