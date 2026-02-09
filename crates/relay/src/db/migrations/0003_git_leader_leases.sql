-- Persistent git leader-election leases (one row per workspace).

CREATE TABLE git_leader_leases (
    workspace_id    uuid PRIMARY KEY,
    daemon_id       uuid NOT NULL,
    lease_id        uuid NOT NULL,
    acquired_at     timestamptz NOT NULL,
    expires_at      timestamptz NOT NULL,
    CHECK (expires_at > acquired_at)
);

CREATE INDEX idx_git_leader_leases_expires_at
    ON git_leader_leases (expires_at);

CREATE INDEX idx_git_leader_leases_daemon_id
    ON git_leader_leases (daemon_id);
