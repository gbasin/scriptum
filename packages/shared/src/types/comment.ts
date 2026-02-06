export interface CommentThread {
  id: string;
  docId: string;
  sectionId: string | null;
  startOffsetUtf16: number;
  endOffsetUtf16: number;
  status: "open" | "resolved";
  version: number;
  createdBy: string;
  createdAt: string;
  resolvedAt: string | null;
}

export interface CommentMessage {
  id: string;
  threadId: string;
  author: string;
  bodyMd: string;
  createdAt: string;
  editedAt: string | null;
}
