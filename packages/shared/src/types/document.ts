export interface Document {
  id: string;
  workspaceId: string;
  path: string;
  title: string;
  tags: string[];
  headSeq: number;
  etag: string;
  archivedAt: string | null;
  deletedAt: string | null;
  createdAt: string;
  updatedAt: string;
}
