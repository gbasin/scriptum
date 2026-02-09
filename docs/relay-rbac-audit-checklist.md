# Relay RBAC Audit Checklist

Date: 2026-02-09
Issue: scriptum-14kp

## Scope reviewed
- `crates/relay/src/auth/middleware.rs`
- `crates/relay/src/api/auth.rs`
- `crates/relay/src/api/workspaces.rs`
- `crates/relay/src/api/documents.rs`
- `crates/relay/src/api/comments.rs`
- `crates/relay/src/api/members.rs`
- `crates/relay/src/api/search.rs`
- `crates/relay/src/api/mod.rs` (workspace/member/share-link routes + RBAC middleware)
- `crates/relay/src/ws/handler.rs` (sync session creation)

## Route checklist
- [x] Auth endpoints are public/token-only as expected.
  - `auth::router` is merged without bearer middleware and only handles OAuth/session token exchange.
- [x] Workspace create is authenticated and unrestricted by role.
  - `POST /v1/workspaces` requires bearer auth and uses caller identity.
- [x] Workspace read endpoints require workspace membership.
  - `GET /v1/workspaces/{id}` uses viewer-role middleware.
  - `GET /v1/workspaces` lists by `user_id` membership.
- [x] Workspace update requires owner role.
  - `PATCH /v1/workspaces/{id}` uses owner-role middleware.
- [x] Members endpoints enforce role tiers.
  - `GET /members` requires viewer role.
  - `PATCH /members/{member_id}` and `DELETE /members/{member_id}` require owner role.
  - `POST /invites` requires owner role.
  - `POST /invites/{token}/accept` requires bearer auth for accepting user.
- [x] Comments endpoints enforce viewer/editor split and workspace matching.
  - List: viewer+.
  - Create/reply/resolve/reopen: editor+.
  - Handler-level check validates token workspace against route workspace.
- [x] Sync session creation enforces workspace scope and membership.
  - `POST /v1/workspaces/{workspace_id}/sync-sessions` checks `workspace_id` match and viewer+ membership.
- [ ] Search endpoint lacks explicit RBAC enforcement in handler.
  - `GET /v1/workspaces/{id}/search` currently only requires bearer auth middleware.
  - Handler ignores authenticated workspace/user role and directly queries by path workspace id.
- [ ] Document delete policy does not match required owner-only matrix.
  - `DELETE /documents/{doc_id}` currently requires editor+ in `documents.rs`.
- [ ] ACL override management policy does not match required owner-only matrix.
  - `POST/DELETE /documents/{doc_id}/acl-overrides*` currently requires editor+.
- [ ] Share-link revoke/update policy does not match required owner-only matrix.
  - `PATCH/DELETE /share-links/{share_link_id}` currently run under editor-role middleware.
- [ ] Workspace API role middleware does not enforce token workspace scope.
  - `api/mod.rs` role middleware checks membership+role but does not validate JWT `workspace_id` against route `workspace_id`.
  - This differs from comments/documents/ws handlers that enforce explicit workspace match.
- [ ] Document-level ACL overrides are writable but not enforced on document access paths.
  - `documents.rs` includes CRUD for `acl_overrides`, but document read/write authorization checks only workspace role.

## Unauthenticated-access review
- Expected unauthenticated endpoints:
  - OAuth/auth routes in `auth.rs`.
  - Share-link redeem endpoint (`POST /v1/share-links/redeem`) is token-based and intentionally public.
- All other audited API routes require bearer auth middleware.

## Follow-up issues created
- `scriptum-18ai` - Enforce RBAC checks on search endpoint.
- `scriptum-2z4v` - Enforce document ACL overrides in auth decisions.
- `scriptum-39k8` - Align owner-only RBAC on destructive workspace routes.
- `scriptum-2im7` - Enforce JWT workspace scope in workspace API role middleware.
