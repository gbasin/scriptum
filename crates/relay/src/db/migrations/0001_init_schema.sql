-- Relay schema bootstrap (PostgreSQL 15+).
-- Mirrors SPEC.md "Relay Database Schema" tables and indexes.

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;

-- ============================================================
-- Users & Auth
-- ============================================================

CREATE TABLE users (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email           citext UNIQUE NOT NULL,
    display_name    text NOT NULL,
    password_hash   text NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE refresh_sessions (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users(id),
    token_hash      bytea UNIQUE NOT NULL,
    family_id       uuid NOT NULL,
    rotated_from    uuid NULL,
    expires_at      timestamptz NOT NULL,
    revoked_at      timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- ============================================================
-- Workspaces & Membership
-- ============================================================

CREATE TABLE workspaces (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            citext UNIQUE NOT NULL,
    name            text NOT NULL,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz NULL
);

CREATE TABLE workspace_members (
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    role            text NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    status          text NOT NULL CHECK (status IN ('active', 'invited', 'suspended')),
    joined_at       timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NULL,
    PRIMARY KEY (workspace_id, user_id)
);

-- ============================================================
-- Documents & Organization
-- ============================================================

CREATE TABLE documents (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    path            text NOT NULL,
    path_norm       text NOT NULL,
    title           text NULL,
    head_seq        bigint NOT NULL DEFAULT 0,
    etag            text NOT NULL,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    archived_at     timestamptz NULL,
    deleted_at      timestamptz NULL
);

CREATE UNIQUE INDEX idx_documents_workspace_path_unique
    ON documents (workspace_id, path_norm)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_documents_workspace_updated ON documents (workspace_id, updated_at DESC);
CREATE INDEX idx_documents_workspace_path ON documents (workspace_id, path_norm);

CREATE TABLE tags (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    name            text NOT NULL,
    color           text NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE document_tags (
    document_id     uuid NOT NULL REFERENCES documents(id),
    tag_id          uuid NOT NULL REFERENCES tags(id),
    PRIMARY KEY (document_id, tag_id)
);

CREATE TABLE backlinks (
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    src_doc_id      uuid NOT NULL REFERENCES documents(id),
    dst_doc_id      uuid NOT NULL REFERENCES documents(id),
    anchor_text     text NULL,
    PRIMARY KEY (workspace_id, src_doc_id, dst_doc_id)
);

-- ============================================================
-- Comments
-- ============================================================

CREATE TABLE comment_threads (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id        uuid NOT NULL REFERENCES workspaces(id),
    doc_id              uuid NOT NULL REFERENCES documents(id),
    section_id          text NULL,
    start_offset_utf16  int NULL,
    end_offset_utf16    int NULL,
    status              text NOT NULL CHECK (status IN ('open', 'resolved')) DEFAULT 'open',
    version             int NOT NULL DEFAULT 1,
    created_by_user_id  uuid NULL REFERENCES users(id),
    created_by_agent_id text NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    resolved_at         timestamptz NULL
);

CREATE INDEX idx_comment_threads_doc_status ON comment_threads (workspace_id, doc_id, status);

CREATE TABLE comment_messages (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    thread_id           uuid NOT NULL REFERENCES comment_threads(id),
    author_user_id      uuid NULL REFERENCES users(id),
    author_agent_id     text NULL,
    body_md             text NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    edited_at           timestamptz NULL
);

-- ============================================================
-- Sharing & Access Control
-- ============================================================

CREATE TABLE share_links (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    target_type     text NOT NULL CHECK (target_type IN ('workspace', 'document')),
    target_id       uuid NOT NULL,
    permission      text NOT NULL CHECK (permission IN ('view', 'edit')),
    token_hash      bytea NOT NULL,
    password_hash   text NULL,
    expires_at      timestamptz NULL,
    max_uses        int NULL,
    use_count       int NOT NULL DEFAULT 0,
    disabled        bool NOT NULL DEFAULT false,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz NULL
);

CREATE TABLE acl_overrides (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    doc_id          uuid NOT NULL REFERENCES documents(id),
    subject_type    text NOT NULL CHECK (subject_type IN ('user', 'agent', 'share_link')),
    subject_id      text NOT NULL,
    role            text NOT NULL CHECK (role IN ('editor', 'viewer')),
    expires_at      timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- ============================================================
-- CRDT Sync
-- ============================================================

CREATE TABLE yjs_update_log (
    workspace_id        uuid NOT NULL,
    doc_id              uuid NOT NULL,
    server_seq          bigint NOT NULL,
    client_id           uuid NOT NULL,
    client_update_id    uuid NOT NULL,
    payload             bytea NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, doc_id, server_seq),
    UNIQUE (workspace_id, doc_id, client_id, client_update_id)
);

CREATE INDEX idx_yjs_update_log_created ON yjs_update_log (workspace_id, doc_id, created_at DESC);

CREATE TABLE yjs_snapshots (
    workspace_id    uuid NOT NULL,
    doc_id          uuid NOT NULL,
    snapshot_seq    bigint NOT NULL,
    payload         bytea NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, doc_id, snapshot_seq)
);

-- ============================================================
-- Idempotency & Infrastructure
-- ============================================================

CREATE TABLE idempotency_keys (
    scope           text NOT NULL,
    idem_key        text NOT NULL,
    request_hash    bytea NOT NULL,
    response_status int NOT NULL,
    response_body   jsonb NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL,
    PRIMARY KEY (scope, idem_key)
);

-- ============================================================
-- Audit
-- ============================================================

CREATE TABLE audit_events (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NULL,
    actor_user_id   uuid NULL,
    actor_agent_id  text NULL,
    event_type      text NOT NULL,
    entity_type     text NOT NULL,
    entity_id       text NOT NULL,
    request_id      text NULL,
    ip_hash         bytea NULL,
    user_agent_hash bytea NULL,
    details         jsonb NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_events_workspace ON audit_events (workspace_id, created_at DESC);
