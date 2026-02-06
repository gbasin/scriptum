import { describe, expect, it } from "vitest";
import {
  commentAnchorTopPx,
  commentRangesFromThreads,
  normalizeInlineCommentThreads,
  type InlineCommentThread,
} from "./document";

describe("document comment helpers", () => {
  it("normalizes list responses with nested thread + messages shape", () => {
    const normalized = normalizeInlineCommentThreads([
      {
        messages: [{ body_md: "Top level comment", id: "msg-1" }],
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
        messages: [{ bodyMd: "Top level comment", id: "msg-1" }],
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
});

