-- Workspace invites: token-based invitation to join a workspace.

CREATE TABLE workspace_invites (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    email           citext NOT NULL,
    role            text NOT NULL CHECK (role IN ('editor', 'viewer')),
    token_hash      bytea NOT NULL,
    invited_by      uuid NOT NULL REFERENCES users(id),
    expires_at      timestamptz NOT NULL,
    accepted_at     timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_workspace_invites_workspace ON workspace_invites (workspace_id)
    WHERE accepted_at IS NULL;
CREATE INDEX idx_workspace_invites_email ON workspace_invites (email)
    WHERE accepted_at IS NULL;
