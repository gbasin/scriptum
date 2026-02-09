const API_V1_PREFIX = "/v1";

function segment(value: string): string {
  return encodeURIComponent(value);
}

function workspaceBase(workspaceId: string): string {
  return `${API_V1_PREFIX}/workspaces/${segment(workspaceId)}`;
}

function workspaceDocumentBase(
  workspaceId: string,
  documentId: string,
): string {
  return `${workspaceBase(workspaceId)}/documents/${segment(documentId)}`;
}

export function authOAuthStart(): string {
  return `${API_V1_PREFIX}/auth/oauth/github/start`;
}

export function authOAuthCallback(): string {
  return `${API_V1_PREFIX}/auth/oauth/github/callback`;
}

export function authTokenRefresh(): string {
  return `${API_V1_PREFIX}/auth/token/refresh`;
}

export function authLogout(): string {
  return `${API_V1_PREFIX}/auth/logout`;
}

export function listWorkspaces(): string {
  return `${API_V1_PREFIX}/workspaces`;
}

export function createWorkspace(): string {
  return `${API_V1_PREFIX}/workspaces`;
}

export function getWorkspace(workspaceId: string): string {
  return workspaceBase(workspaceId);
}

export function updateWorkspace(workspaceId: string): string {
  return workspaceBase(workspaceId);
}

export function inviteToWorkspace(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/invites`;
}

export function acceptInvite(token: string): string {
  return `${API_V1_PREFIX}/invites/${segment(token)}/accept`;
}

export function listMembers(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/members`;
}

export function updateMember(workspaceId: string, userId: string): string {
  return `${workspaceBase(workspaceId)}/members/${segment(userId)}`;
}

export function removeMember(workspaceId: string, userId: string): string {
  return `${workspaceBase(workspaceId)}/members/${segment(userId)}`;
}

export function listDocuments(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/documents`;
}

export function createDocument(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/documents`;
}

export function getDocument(workspaceId: string, documentId: string): string {
  return workspaceDocumentBase(workspaceId, documentId);
}

export function updateDocument(
  workspaceId: string,
  documentId: string,
): string {
  return workspaceDocumentBase(workspaceId, documentId);
}

export function deleteDocument(
  workspaceId: string,
  documentId: string,
): string {
  return workspaceDocumentBase(workspaceId, documentId);
}

export function addTags(workspaceId: string, documentId: string): string {
  return `${workspaceDocumentBase(workspaceId, documentId)}/tags`;
}

export function searchDocuments(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/search`;
}

export function listComments(workspaceId: string, documentId: string): string {
  return `${workspaceDocumentBase(workspaceId, documentId)}/comments`;
}

export function createComment(workspaceId: string, documentId: string): string {
  return `${workspaceDocumentBase(workspaceId, documentId)}/comments`;
}

export function addCommentMessage(
  workspaceId: string,
  commentId: string,
): string {
  return `${workspaceBase(workspaceId)}/comments/${segment(commentId)}/messages`;
}

export function resolveComment(workspaceId: string, commentId: string): string {
  return `${workspaceBase(workspaceId)}/comments/${segment(commentId)}/resolve`;
}

export function reopenComment(workspaceId: string, commentId: string): string {
  return `${workspaceBase(workspaceId)}/comments/${segment(commentId)}/reopen`;
}

export function createShareLink(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/share-links`;
}

export function listShareLinks(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/share-links`;
}

export function updateShareLink(
  workspaceId: string,
  shareLinkId: string,
): string {
  return `${workspaceBase(workspaceId)}/share-links/${segment(shareLinkId)}`;
}

export function revokeShareLink(
  workspaceId: string,
  shareLinkId: string,
): string {
  return `${workspaceBase(workspaceId)}/share-links/${segment(shareLinkId)}`;
}

export function redeemShareLink(): string {
  return `${API_V1_PREFIX}/share-links/redeem`;
}

export function createAclOverride(
  workspaceId: string,
  documentId: string,
): string {
  return `${workspaceDocumentBase(workspaceId, documentId)}/acl-overrides`;
}

export function deleteAclOverride(
  workspaceId: string,
  documentId: string,
  overrideId: string,
): string {
  return `${workspaceDocumentBase(workspaceId, documentId)}/acl-overrides/${segment(
    overrideId,
  )}`;
}

export function createSyncSession(workspaceId: string): string {
  return `${workspaceBase(workspaceId)}/sync-sessions`;
}
