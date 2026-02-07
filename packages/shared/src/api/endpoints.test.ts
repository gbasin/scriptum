import { describe, expect, it } from "vitest";
import {
  acceptInvite,
  addCommentMessage,
  addTags,
  authLogout,
  authOAuthCallback,
  authOAuthStart,
  authTokenRefresh,
  createAclOverride,
  createComment,
  createDocument,
  createShareLink,
  createSyncSession,
  createWorkspace,
  deleteAclOverride,
  deleteDocument,
  getDocument,
  getWorkspace,
  inviteToWorkspace,
  listComments,
  listDocuments,
  listMembers,
  listWorkspaces,
  removeMember,
  reopenComment,
  resolveComment,
  searchDocuments,
  updateDocument,
  updateMember,
  updateShareLink,
  updateWorkspace,
} from "./endpoints";

describe("api endpoint builders", () => {
  it("builds all spec-defined endpoints with v1 prefix", () => {
    expect(authOAuthStart()).toBe("/v1/auth/oauth/github/start");
    expect(authOAuthCallback()).toBe("/v1/auth/oauth/github/callback");
    expect(authTokenRefresh()).toBe("/v1/auth/token/refresh");
    expect(authLogout()).toBe("/v1/auth/logout");
    expect(listWorkspaces()).toBe("/v1/workspaces");
    expect(createWorkspace()).toBe("/v1/workspaces");
    expect(getWorkspace("ws-1")).toBe("/v1/workspaces/ws-1");
    expect(updateWorkspace("ws-1")).toBe("/v1/workspaces/ws-1");
    expect(inviteToWorkspace("ws-1")).toBe("/v1/workspaces/ws-1/invites");
    expect(acceptInvite("token-1")).toBe("/v1/invites/token-1/accept");
    expect(listMembers("ws-1")).toBe("/v1/workspaces/ws-1/members");
    expect(updateMember("ws-1", "user-1")).toBe(
      "/v1/workspaces/ws-1/members/user-1",
    );
    expect(removeMember("ws-1", "user-1")).toBe(
      "/v1/workspaces/ws-1/members/user-1",
    );
    expect(listDocuments("ws-1")).toBe("/v1/workspaces/ws-1/documents");
    expect(createDocument("ws-1")).toBe("/v1/workspaces/ws-1/documents");
    expect(getDocument("ws-1", "doc-1")).toBe("/v1/workspaces/ws-1/documents/doc-1");
    expect(updateDocument("ws-1", "doc-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1",
    );
    expect(deleteDocument("ws-1", "doc-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1",
    );
    expect(addTags("ws-1", "doc-1")).toBe("/v1/workspaces/ws-1/documents/doc-1/tags");
    expect(searchDocuments("ws-1")).toBe("/v1/workspaces/ws-1/search");
    expect(listComments("ws-1", "doc-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1/comments",
    );
    expect(createComment("ws-1", "doc-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1/comments",
    );
    expect(addCommentMessage("ws-1", "comment-1")).toBe(
      "/v1/workspaces/ws-1/comments/comment-1/messages",
    );
    expect(resolveComment("ws-1", "comment-1")).toBe(
      "/v1/workspaces/ws-1/comments/comment-1/resolve",
    );
    expect(reopenComment("ws-1", "comment-1")).toBe(
      "/v1/workspaces/ws-1/comments/comment-1/reopen",
    );
    expect(createShareLink("ws-1")).toBe("/v1/workspaces/ws-1/share-links");
    expect(updateShareLink("ws-1", "share-1")).toBe(
      "/v1/workspaces/ws-1/share-links/share-1",
    );
    expect(createAclOverride("ws-1", "doc-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1/acl-overrides",
    );
    expect(deleteAclOverride("ws-1", "doc-1", "override-1")).toBe(
      "/v1/workspaces/ws-1/documents/doc-1/acl-overrides/override-1",
    );
    expect(createSyncSession("ws-1")).toBe("/v1/workspaces/ws-1/sync-sessions");
  });

  it("encodes path segments", () => {
    expect(getWorkspace("ws/team alpha")).toBe("/v1/workspaces/ws%2Fteam%20alpha");
    expect(getDocument("ws-1", "docs/path.md")).toBe(
      "/v1/workspaces/ws-1/documents/docs%2Fpath.md",
    );
    expect(acceptInvite("token+slash/value")).toBe(
      "/v1/invites/token%2Bslash%2Fvalue/accept",
    );
  });
});
