import { beforeEach, describe, expect, it } from "vitest";
import {
  commentsStoreKey,
  useCommentsStore,
} from "./comments";

describe("comments store", () => {
  beforeEach(() => {
    useCommentsStore.getState().reset();
  });

  it("stores threads per workspace/document key", () => {
    useCommentsStore.getState().setDocumentThreads("ws-1", "doc-1", [
      {
        endOffsetUtf16: 12,
        id: "thread-1",
        messages: [
          {
            authorName: "Ada",
            bodyMd: "Needs follow-up",
            createdAt: "2026-01-01T00:00:00.000Z",
            id: "msg-1",
            isOwn: false,
          },
        ],
        startOffsetUtf16: 3,
        status: "open",
      },
    ]);

    expect(useCommentsStore.getState().threadsByDocumentKey).toEqual({
      [commentsStoreKey("ws-1", "doc-1")]: [
        {
          endOffsetUtf16: 12,
          id: "thread-1",
          messages: [
            {
              authorName: "Ada",
              bodyMd: "Needs follow-up",
              createdAt: "2026-01-01T00:00:00.000Z",
              id: "msg-1",
              isOwn: false,
            },
          ],
          startOffsetUtf16: 3,
          status: "open",
        },
      ],
    });
  });
});
