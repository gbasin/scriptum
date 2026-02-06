export interface ShareLink {
  id: string;
  targetType: "workspace" | "document";
  targetId: string;
  permission: "view" | "edit";
  expiresAt: string | null;
  maxUses: number | null;
  useCount: number;
  disabled: boolean;
  createdAt: string;
  revokedAt: string | null;
  urlOnce: string;
}

export interface SyncSession {
  sessionId: string;
  sessionToken: string;
  wsUrl: string;
  heartbeatIntervalMs: number;
  maxFrameBytes: number;
  resumeToken: string;
  resumeExpiresAt: string;
}
