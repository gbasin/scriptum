import { create } from "zustand";
import type { InlineCommentThread } from "../lib/inline-comments";

export function commentsStoreKey(workspaceId: string, documentId: string) {
  return `${workspaceId}:${documentId}`;
}

interface CommentsStoreState {
  reset: () => void;
  setDocumentThreads: (
    workspaceId: string,
    documentId: string,
    threads: readonly InlineCommentThread[],
  ) => void;
  threadsByDocumentKey: Record<string, InlineCommentThread[]>;
}

function cloneThreads(
  threads: readonly InlineCommentThread[],
): InlineCommentThread[] {
  return threads.map((thread) => ({
    ...thread,
    messages: thread.messages.map((message) => ({ ...message })),
  }));
}

export const useCommentsStore = create<CommentsStoreState>()((set) => ({
  threadsByDocumentKey: {},
  setDocumentThreads: (workspaceId, documentId, threads) => {
    const key = commentsStoreKey(workspaceId, documentId);
    set((currentState) => ({
      ...currentState,
      threadsByDocumentKey: {
        ...currentState.threadsByDocumentKey,
        [key]: cloneThreads(threads),
      },
    }));
  },
  reset: () => {
    set({ threadsByDocumentKey: {} });
  },
}));
